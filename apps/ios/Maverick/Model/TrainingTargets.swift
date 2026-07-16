import Foundation

/// Weekly time-in-zone targets (§8) — rule-based and honest, no ML. Starts from a
/// polarized 80/20 split over a weekly volume the user actually trains at (never a
/// fantasy plan), then applies two adjustments the spec calls for:
///
/// 1. **Behaviour**: a Z3/Z4-heavy history ("grey zone" training) shifts target share
///    toward Z2 — the classic polarized-training correction.
/// 2. **Recovery**: a low recent Charge trend halves the Z4/Z5 targets and banks the
///    time into Z1/Z2 — train, but easy.
///
/// Pure: history/recovery in, minute targets out. The Strain hub renders them on the
/// shared zone bars with a "to go" line, which is the nudge surface (§9) folded into
/// an always-visible, hub-editable card instead of dismissible pop-ups.
enum TrainingTargets {

    /// Base share of weekly cardio time per zone (Z1…Z5), polarized 80/20.
    static let baseShare: [Double] = [0.25, 0.55, 0.10, 0.07, 0.03]

    /// Weekly minute floor/ceiling: at least the WHO-ish 150 easy minutes, and never
    /// more than a 2× stretch of what the user actually does (a target should pull,
    /// not mock).
    static let floorMinutes = 150.0

    /// Compute this week's per-zone minute targets.
    /// - Parameters:
    ///   - recentWeeks: per-week zone minutes (Z1…Z5) for up to the last ~4 FULL weeks,
    ///     any order. Empty = no history, fall back to the floor volume.
    ///   - recoveryAvg: mean Charge over the recent window (0–100), nil when unknown.
    static func weeklyTargets(recentWeeks: [[Double]], recoveryAvg: Double?) -> [Double] {
        // Volume: what they actually train, clamped to sane bounds.
        let weeklyTotals = recentWeeks.map { $0.reduce(0, +) }.filter { $0 > 0 }
        let observed = weeklyTotals.isEmpty ? 0 : weeklyTotals.reduce(0, +) / Double(weeklyTotals.count)
        let volume = min(max(observed, floorMinutes), floorMinutes * 4)

        var share = baseShare

        // Behaviour adjustment: grey-zone bias → more Z2 in the plan.
        let totalAll = recentWeeks.flatMap { $0 }.reduce(0, +)
        if totalAll > 0 {
            let greyShare = recentWeeks.reduce(0.0) { acc, week in
                acc + (week[safe: 2] ?? 0) + (week[safe: 3] ?? 0)
            } / totalAll
            if greyShare > 0.3 {
                share[1] += 0.08          // push Z2
                share[2] -= 0.05          // pull the grey zone back
                share[3] -= 0.03
            }
        }

        // Recovery adjustment: consistently low Charge → halve the hard zones.
        if let r = recoveryAvg, r < 50 {
            let cut = (share[3] + share[4]) / 2
            share[3] /= 2
            share[4] /= 2
            share[0] += cut * 0.4
            share[1] += cut * 0.6
        }

        return share.map { ($0 * volume).rounded() }
    }

    /// The single most useful "to go" sentence for a week in progress, or nil when the
    /// week is on track (all targets met) or nothing meaningful remains. Prefers the
    /// LOW-zone gap — easy volume is the target people actually under-fill.
    static func nudgeLine(done: [Double], targets: [Double]) -> String? {
        guard done.count == 5, targets.count == 5 else { return nil }
        for i in [1, 0, 2, 3, 4] where targets[i] > 0 {   // Z2 first, then Z1, then up
            let gap = targets[i] - done[i]
            guard gap >= 5 else { continue }              // <5 min to go isn't a nudge
            let mins = Int(gap.rounded())
            return "\(mins) min of Zone \(i + 1) to go this week."
        }
        return nil
    }

    /// Bucket workout rows into per-week zone minutes (Z1…Z5), keyed by the week
    /// containing each row's start. `weekOf` uses the user's calendar week.
    static func weeklyZoneMinutes(rows: [WorkoutRow], calendar: Calendar = .current)
        -> [Date: [Double]] {
        var out: [Date: [Double]] = [:]
        for row in rows {
            guard let pct = WorkoutZones.percents(row.zonesJSON) else { continue }
            let durMin = (row.durationS ?? Double(row.endTs - row.startTs)) / 60
            guard durMin > 0 else { continue }
            let start = Date(timeIntervalSince1970: TimeInterval(row.startTs))
            guard let week = calendar.dateInterval(of: .weekOfYear, for: start)?.start else { continue }
            var mins = out[week] ?? [Double](repeating: 0, count: 5)
            for i in 0..<5 { mins[i] += durMin * pct[i] / 100 }
            out[week] = mins
        }
        return out
    }
}
