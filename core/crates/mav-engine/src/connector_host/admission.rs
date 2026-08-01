//! Sample admission: the stream contract, scoring, clock placement, and commit accounting.

use super::*;

impl ConnectorHost {
    pub(super) fn commit_samples(
        &mut self,
        batch_id: BatchId,
        samples: &[WireSample],
        wall_time_ms: Option<i64>,
    ) -> Result<CommitAccounting> {
        let mut accounting = CommitAccounting {
            emitted: samples.len(),
            ..CommitAccounting::default()
        };
        let wall_ms = wall_time_ms.ok_or_else(|| {
            error(
                codes::CONNECTOR_HOST_SAMPLE_INVALID,
                "sample emission requires an explicit host wall time",
            )
        })?;
        let wall = WallTime::from_nanos(ms_to_ns(wall_ms)?);
        let mut provenance = Vec::with_capacity(samples.len());
        for (index, sample) in samples.iter().enumerate() {
            let (kind, unit) = stream_contract(&sample.stream)?;
            if sample.unit != unit {
                return Err(error(
                    codes::CONNECTOR_HOST_SAMPLE_INVALID,
                    "connector sample unit differs from the pipeline stream contract",
                ));
            }
            let device_ms = sample.device_time_ms.ok_or_else(|| {
                error(
                    codes::CONNECTOR_HOST_SAMPLE_INVALID,
                    "connector sample has no device timestamp",
                )
            })?;
            let sequence = u16::try_from(sample.sequence).map_err(|_| {
                error(
                    codes::CONNECTOR_HOST_SAMPLE_INVALID,
                    "connector sample sequence exceeds pipeline width",
                )
            })?;
            let metadata = MetadataId::new(metadata_id(
                self.config.session_id,
                batch_id.0,
                index as u64,
            ));
            let raw = RawSample {
                kind,
                device_time: DeviceTime::from_nanos(ms_to_ns(device_ms)?),
                seq: sequence,
                value: RawValue::Converted(sample.value_microunits as f64 / 1_000_000.0),
            };
            self.observe_produced(Stage::Decode, 1);
            let mut scored = mav_sqi::score_sample(&raw, metadata);
            let record = Provenance {
                metadata,
                source_stream: kind,
                quality: scored.quality.score,
                algorithm_id: "connector-abi-v1".to_owned(),
                algorithm_version: Version::new(1, 0, 0),
                sample_count: 1,
            };
            self.observe_produced(Stage::Sqi, 1);
            self.learn_clock(scored.device_time, wall);
            place_on_wall_with(&self.clock_map, &mut scored, wall);
            // Provenance describes a stored sample. Writing it for one the timeline already holds
            // leaves a row pointing at nothing.
            match self.timeline.insert(scored) {
                TimelineInsertOutcome::Inserted => provenance.push(record),
                TimelineInsertOutcome::Duplicate => accounting.duplicate += 1,
            }
        }
        let device = DeviceId::new(self.config.device_id);
        let ordered = self.timeline.drain_ordered();
        self.observe_produced(Stage::Timeline, ordered.len());
        let mut persisted = 0usize;
        self.store.in_transaction(|store| {
            for record in &provenance {
                store.upsert_provenance(record)?;
            }
            persisted = 0;
            for sample in &ordered {
                if store.insert_sample(device, sample)? == StoreInsertOutcome::Inserted {
                    persisted += 1;
                }
            }
            Ok(())
        })?;
        // The store's natural key is the durable dedup layer, so a sample the timeline accepted can
        // still be one the store already held.
        self.observe_produced(Stage::Store, persisted);
        accounting.persisted = persisted;
        accounting.duplicate += ordered.len().saturating_sub(persisted);
        if self.active_capture.as_deref() == Some("ecg") {
            let ecg_values: Vec<f64> = ordered
                .iter()
                .filter(|sample| sample.kind == StreamKind::Ecg)
                .map(|sample| sample.value.as_f64())
                .collect();
            if !ecg_values.is_empty() {
                accounting.stop_capture = self
                    .ecg_capture
                    .as_mut()
                    .ok_or_else(|| host_state("active ECG stream has no capture controller"))?
                    .ingest(&ecg_values, wall_ms)?;
            }
        }
        self.samples_persisted = self.samples_persisted.saturating_add(persisted as u64);
        self.samples_duplicate = self
            .samples_duplicate
            .saturating_add(accounting.duplicate as u64);
        Ok(accounting)
    }

    /// Anchor the session's clock map the first time an implausible device time arrives beside a
    /// known host wall time. One anchor is enough: within a session the device clock advances
    /// one-for-one with wall time, so the whole run shifts by a single offset and the gaps between
    /// samples survive. A device whose clock is fine anchors nothing.
    pub(super) fn learn_clock(&mut self, device: DeviceTime, capture: WallTime) {
        if WallTime::from_nanos(device.as_nanos()).is_plausible() {
            return;
        }
        if self.clock_map.to_wall(device).is_some() {
            return;
        }
        self.clock_map = anchor_from(device, capture);
    }

    /// True when the duplicate total has at least doubled since the last journal entry.
    pub(super) fn duplicates_worth_journalling(&mut self) -> bool {
        let total = self.samples_duplicate;
        if total >= self.duplicates_journalled_at.saturating_mul(2).max(1) {
            self.duplicates_journalled_at = total;
            true
        } else {
            false
        }
    }

    pub(super) fn record_duplicates(
        &self,
        accounting: &CommitAccounting,
        wall_time_ms: Option<i64>,
    ) -> Result<()> {
        self.store.record_error(
            &error(
                codes::CONNECTOR_HOST_SAMPLE_DUPLICATE,
                format!(
                    "{} of {} emitted samples were already held ({} persisted); \
                     {} duplicates this session",
                    accounting.duplicate,
                    accounting.emitted,
                    accounting.persisted,
                    self.samples_duplicate
                ),
            ),
            wall_time_ms.unwrap_or_default().saturating_mul(1_000_000),
        )
    }
}

pub(super) fn stream_contract(value: &str) -> Result<(StreamKind, &'static str)> {
    match value {
        "heart-rate" => Ok((StreamKind::HeartRate, "beats-per-minute")),
        // Two interval streams because the physiological event differs, and only the electrical
        // one may be called heart-rate variability. A connector declaring the wrong one is
        // mislabelling the wearer's data, so the name is part of the contract.
        "rr-interval" => Ok((StreamKind::RrInterval, "milliseconds")),
        "pulse-interval" => Ok((StreamKind::PulseInterval, "milliseconds")),
        // The WHOOP AFE is an 18-bit converter with a +/-10 mV range, so a connector that knows its
        // device can and must deliver calibrated millivolts. An ECG in raw counts cannot carry a
        // calibration pulse or a 10 mm/mV report without asserting a scale nobody established.
        "ecg" => Ok((StreamKind::Ecg, "millivolts")),
        "red-ppg" => Ok((StreamKind::RedPpg, "counts")),
        "infrared-ppg" => Ok((StreamKind::InfraredPpg, "counts")),
        "ambient-light" => Ok((StreamKind::AmbientLight, "counts")),
        "ppg" => Ok((StreamKind::Ppg, "counts")),
        "optical-raw" => Ok((StreamKind::OpticalRaw, "counts")),
        "imu" => Ok((StreamKind::Imu, "milli-g")),
        "gyro" => Ok((StreamKind::Gyro, "milli-degrees-per-second")),
        "gravity" => Ok((StreamKind::Gravity, "milli-g")),
        "skin-temp" => Ok((StreamKind::SkinTemp, "degrees-celsius")),
        "skin-temp-raw" => Ok((StreamKind::SkinTempRaw, "counts")),
        "spo2-raw" => Ok((StreamKind::Spo2Raw, "counts")),
        "spo2-percent" => Ok((StreamKind::Spo2Percent, "percent")),
        "resp-raw" => Ok((StreamKind::RespRaw, "counts")),
        "battery-soc" => Ok((StreamKind::BatterySoc, "percent")),
        "step-count" => Ok((StreamKind::StepCount, "count")),
        "activity-class" => Ok((StreamKind::ActivityClass, "code")),
        "skin-contact" => Ok((StreamKind::SkinContact, "boolean")),
        "signal-quality" => Ok((StreamKind::SignalQuality, "percent")),
        "wrist-state" => Ok((StreamKind::WristState, "boolean")),
        "sleep-state-raw" => Ok((StreamKind::SleepStateRaw, "code")),
        _ => Err(error(
            codes::CONNECTOR_HOST_SAMPLE_INVALID,
            "connector sample stream is not admitted by the pipeline",
        )),
    }
}

pub(super) fn validate_sample(sample: &WireSample) -> Result<()> {
    let (_, unit) = stream_contract(&sample.stream)?;
    if sample.unit != unit {
        return Err(error(
            codes::CONNECTOR_HOST_SAMPLE_INVALID,
            "connector sample unit differs from the pipeline stream contract",
        ));
    }
    if sample.device_time_ms.is_none() || sample.sequence > u32::from(u16::MAX) {
        return Err(error(
            codes::CONNECTOR_HOST_SAMPLE_INVALID,
            "connector sample timestamp or sequence is outside pipeline bounds",
        ));
    }
    Ok(())
}
