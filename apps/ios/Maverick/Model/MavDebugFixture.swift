#if DEBUG

  import Foundation

  // Debug-only fixture days.
  //
  // This exists so a screen can be judged without a strap in the room, and it is fenced three ways:
  // the whole file is `#if DEBUG`, it is only ever consulted when nothing is connected, and every
  // surface it feeds renders a visible SAMPLE badge. A release build cannot reach a line of it.
  //
  // The shape mirrors what the core actually produces — one admitted analytic with a real band, and
  // the rest honestly unavailable with the core's own reason kinds — so the layout being judged is
  // the layout that ships, not a prettier one.

  enum MavDebugFixture {

    static let dayCount = 45

    /// Hand-shaped, deterministic daily variation. A trigonometric fixture looked like a demo
    /// waveform rather than a person and made an otherwise honest chart feel synthetic.
    private static let variabilityValues: [Double] = [
      59.8, 61.2, 58.9, 62.4, 64.1, 63.0, 60.7, 61.9, 65.2, 66.0,
      64.4, 62.8, 63.6, 67.1, 65.9, 64.8, 66.5, 68.0, 67.2, 65.1,
      63.9, 66.8, 69.2, 68.4, 67.6, 70.1, 68.9, 66.7, 67.8, 71.0,
      69.7, 68.3, 70.5, 72.1, 71.4, 69.0, 70.2, 73.0, 71.8, 72.6,
      74.1, 72.9, 73.5, 75.0, 73.0,
    ]

    /// Irregular score history used only for debug-only metrics the frozen daily row does not yet
    /// carry. Its last value is 82 so the chart and the current Recovery card agree.
    static let scoreHistory: [Double] = [
      72, 71, 73, 74, 76, 75, 74, 73, 72, 74,
      75, 77, 76, 74, 73, 75, 78, 79, 78, 77,
      75, 74, 76, 77, 79, 81, 80, 78, 77, 79,
      80, 82, 83, 82, 80, 79, 81, 82, 84, 85,
      84, 82, 81, 83, 82,
    ]

    static func snapshots(now: Date = Date()) -> [DailySnapshotReport] {
      (0..<dayCount).reversed().map { back in
        let date = Calendar.current.date(byAdding: .day, value: -back, to: now) ?? now
        let offset = dayCount - back
        let rmssd = variabilityValues[offset - 1]
        return DailySnapshotReport(
          day: Repository.localDayKey(date),
          dayIndex: Int64(offset),
          currentBpm: back == 0 ? 64 : nil,
          meanBpm: 68.4,
          hrSampleCount: 12_480,
          hrExcludedCount: 214,
          hrv: HrvReport(
            label: "pulse_rate_variability",
            meanIntervalMs: 862.0,
            rmssdMs: rmssd,
            sdnnMs: (rmssd * 0.72).rounded(toPlaces: 1),
            pnn50Percent: 18.4,
            sd1Ms: (rmssd * 0.71).rounded(toPlaces: 1),
            sd2Ms: (rmssd * 1.31).rounded(toPlaces: 1),
            alpha1: 1.04,
            intervalCount: 8_412,
            excludedCount: 118),
          hrvSpectrum: nil,
          readiness: ReadinessReport(
            tier: rmssd > 74 ? "primed" : (rmssd < 58 ? "suppressed" : "normal"),
            baseline7Ms: 66.0,
            normalLowMs: 55.0,
            normalHighMs: 78.0,
            overreachingWatch: false),
          availability: availability,
          algorithms: ["time_domain_interval_variability@1.0.0", "hr_feature@1.0.0"],
          snapshotHash: String(format: "fixture-%04d", offset))
      }
    }

    static func workouts(now: Date = Date()) -> [WorkoutRow] {
      let calendar = Calendar.current
      func session(
        daysBack: Int, hour: Int, minutes: Int, sport: String, avgHr: Int, maxHr: Int,
        strain: Double, energy: Double, zones: String
      ) -> WorkoutRow {
        let day = calendar.date(byAdding: .day, value: -daysBack, to: now) ?? now
        let startDate = calendar.date(
          bySettingHour: hour, minute: 0, second: 0, of: day) ?? day
        let start = Int(startDate.timeIntervalSince1970)
        return WorkoutRow(
          startTs: start,
          endTs: start + minutes * 60,
          sport: sport,
          source: "Sample",
          durationS: Double(minutes * 60),
          energyKcal: energy,
          avgHr: avgHr,
          maxHr: maxHr,
          strain: strain,
          zonesJSON: zones,
          notes: "Sample session")
      }
      return [
        session(
          daysBack: 0, hour: 17, minutes: 52, sport: "Strength", avgHr: 116, maxHr: 151,
          strain: 11.4, energy: 338, zones: "{\"z1\":18,\"z2\":34,\"z3\":29,\"z4\":15,\"z5\":4}"),
        session(
          daysBack: 1, hour: 7, minutes: 38, sport: "Running", avgHr: 146, maxHr: 176,
          strain: 14.8, energy: 462, zones: "{\"z1\":3,\"z2\":15,\"z3\":31,\"z4\":39,\"z5\":12}"),
        session(
          daysBack: 2, hour: 18, minutes: 42, sport: "Rowing", avgHr: 138, maxHr: 169,
          strain: 13.6, energy: 418, zones: "{\"z1\":5,\"z2\":21,\"z3\":35,\"z4\":31,\"z5\":8}"),
        session(
          daysBack: 3, hour: 18, minutes: 61, sport: "Cycling", avgHr: 132, maxHr: 163,
          strain: 12.7, energy: 521, zones: "{\"z1\":5,\"z2\":26,\"z3\":38,\"z4\":25,\"z5\":6}"),
        session(
          daysBack: 4, hour: 7, minutes: 45, sport: "Swimming", avgHr: 127, maxHr: 158,
          strain: 10.8, energy: 356, zones: "{\"z1\":9,\"z2\":33,\"z3\":36,\"z4\":18,\"z5\":4}"),
        session(
          daysBack: 5, hour: 12, minutes: 31, sport: "Walking", avgHr: 101, maxHr: 124,
          strain: 6.2, energy: 174, zones: "{\"z1\":48,\"z2\":38,\"z3\":12,\"z4\":2,\"z5\":0}"),
        session(
          daysBack: 6, hour: 8, minutes: 47, sport: "Yoga", avgHr: 88, maxHr: 111,
          strain: 4.8, energy: 128, zones: "{\"z1\":70,\"z2\":26,\"z3\":4,\"z4\":0,\"z5\":0}"),
      ]
    }

    /// A fixture link, so the device chip has a battery percentage and the device sheet has a
    /// paired state to render. Marked on the model like every other fixture surface.
    @MainActor
    static func apply(to live: LiveState, model: AppModel) {
      live.connected = true
      live.bonded = true
      live.heartRate = 64
      live.batteryPct = 41
      live.charging = false
      live.worn = true
      live.advertisingName = "MG"
      model.bpm = 64
      model.syncProgress = "Syncing history — 3 of 7 days"
    }

    /// The same reason *kinds* the core emits, so the unavailable cards under test are the real
    /// ones rather than a friendlier rewrite.
    private static let availability: [AnalyticAvailabilityReport] = [
      AnalyticAvailabilityReport(
        analytic: "time_domain_hrv", available: true, reason: nil, missingStreams: []),
      AnalyticAvailabilityReport(
        analytic: "recovery", available: false, reason: "algorithm_not_admitted",
        missingStreams: []),
      AnalyticAvailabilityReport(
        analytic: "sleep_performance", available: false, reason: "missing_streams",
        missingStreams: ["sleep_stage"]),
      AnalyticAvailabilityReport(
        analytic: "illness_risk", available: false, reason: "missing_streams",
        missingStreams: ["skin_temperature", "respiration"]),
      AnalyticAvailabilityReport(
        analytic: "cycle_phase", available: false, reason: "missing_streams",
        missingStreams: ["skin_temperature"]),
    ]
  }

  extension Double {
    fileprivate func rounded(toPlaces places: Int) -> Double {
      let divisor = pow(10.0, Double(places))
      return (self * divisor).rounded() / divisor
    }
  }

#endif
