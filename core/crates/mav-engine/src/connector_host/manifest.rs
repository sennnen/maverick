//! What the signed manifest permits: declared services, characteristics, and capabilities.

use super::*;

impl ConnectorHost {
    pub(super) fn validate_declared(&self, body: &ActionBody) -> Result<()> {
        match body {
            ActionBody::StartScan {
                service_uuids,
                manufacturer_ids,
            } => {
                self.require_capability(TransportCapability::Scan)?;
                let declared_services = self.declared_service_uuids();
                if service_uuids
                    .iter()
                    .any(|uuid| !declared_services.contains(uuid))
                {
                    return Err(undeclared("scan names an undeclared service UUID"));
                }
                let declared_manufacturers: BTreeSet<u16> = self
                    .manifest
                    .device_families
                    .iter()
                    .filter_map(|family| family.manufacturer_id)
                    .collect();
                if manufacturer_ids
                    .iter()
                    .any(|id| !declared_manufacturers.contains(id))
                {
                    return Err(undeclared("scan names an undeclared manufacturer id"));
                }
            }
            ActionBody::Connect { address } => {
                self.require_capability(TransportCapability::Connect)?;
                if !self.advertised_addresses.contains(address) {
                    return Err(undeclared(
                        "connect address was not advertised in this session",
                    ));
                }
            }
            ActionBody::EnsurePaired => self.require_capability(TransportCapability::Pair)?,
            ActionBody::DiscoverServices => {
                self.require_capability(TransportCapability::Discover)?
            }
            ActionBody::Subscribe { characteristic_id }
            | ActionBody::Unsubscribe { characteristic_id } => {
                self.require_capability(TransportCapability::Subscribe)?;
                let characteristic = self.characteristic(characteristic_id)?;
                if !characteristic.properties.iter().any(|property| {
                    matches!(
                        property,
                        CharacteristicProperty::Notify | CharacteristicProperty::Indicate
                    )
                }) {
                    return Err(undeclared("characteristic is not subscribable"));
                }
            }
            ActionBody::Read { characteristic_id } => {
                self.require_capability(TransportCapability::Read)?;
                self.require_property(characteristic_id, CharacteristicProperty::Read)?;
            }
            ActionBody::Write {
                characteristic_id,
                confirmed,
                ..
            } => {
                self.require_capability(TransportCapability::Write)?;
                let characteristic = self.characteristic(characteristic_id)?;
                if characteristic.confirmed_write_required && !confirmed {
                    return Err(undeclared("characteristic requires confirmed writes"));
                }
                let required = if *confirmed {
                    CharacteristicProperty::Write
                } else {
                    CharacteristicProperty::WriteWithoutResponse
                };
                if !characteristic.properties.contains(&required) {
                    return Err(undeclared(
                        "characteristic does not allow the requested write",
                    ));
                }
            }
            ActionBody::DeclareCapabilities { streams } => {
                let declared: BTreeSet<&str> = self
                    .manifest
                    .capabilities
                    .iter()
                    .map(|capability| capability.stream.as_str())
                    .collect();
                if streams
                    .iter()
                    .any(|stream| !declared.contains(stream.as_str()))
                {
                    return Err(undeclared(
                        "connector declared an unsigned stream capability",
                    ));
                }
            }
            ActionBody::EmitSamples { samples, .. } => {
                for sample in samples {
                    let declared = self
                        .manifest
                        .capabilities
                        .iter()
                        .any(|capability| capability.stream == sample.stream);
                    if !declared {
                        return Err(undeclared("sample uses an undeclared stream"));
                    }
                    validate_sample(sample)?;
                }
            }
            ActionBody::StopScan
            | ActionBody::Disconnect
            | ActionBody::SetTimer { .. }
            | ActionBody::CancelTimer { .. }
            | ActionBody::StatePut { .. }
            | ActionBody::StateDelete { .. }
            | ActionBody::StateCommit
            | ActionBody::EmitDiagnostic { .. }
            | ActionBody::CompleteOperation { .. } => {}
        }
        Ok(())
    }

    pub(super) fn require_capability(&self, required: TransportCapability) -> Result<()> {
        if self
            .manifest
            .capabilities
            .iter()
            .any(|capability| capability.transport.contains(&required))
        {
            Ok(())
        } else {
            Err(undeclared(
                "transport capability is not signed in the manifest",
            ))
        }
    }

    pub(super) fn declared_service_uuids(&self) -> BTreeSet<String> {
        self.manifest
            .services
            .iter()
            .map(|service| service.uuid.clone())
            .chain(
                self.manifest
                    .device_families
                    .iter()
                    .flat_map(|family| family.service_uuids.clone()),
            )
            .collect()
    }

    pub(super) fn characteristic(&self, id: &str) -> Result<&CharacteristicDecl> {
        self.manifest
            .services
            .iter()
            .flat_map(|service| &service.characteristics)
            .find(|characteristic| characteristic.id == id)
            .ok_or_else(|| undeclared("action names an undeclared characteristic"))
    }

    pub fn characteristic_address(&self, id: &str) -> Option<(String, String)> {
        self.manifest.services.iter().find_map(|service| {
            service
                .characteristics
                .iter()
                .find(|characteristic| characteristic.id == id)
                .map(|characteristic| (service.uuid.clone(), characteristic.uuid.clone()))
        })
    }

    pub(super) fn require_property(
        &self,
        id: &str,
        property: CharacteristicProperty,
    ) -> Result<()> {
        if self.characteristic(id)?.properties.contains(&property) {
            Ok(())
        } else {
            Err(undeclared(
                "characteristic property is not signed in the manifest",
            ))
        }
    }
}
