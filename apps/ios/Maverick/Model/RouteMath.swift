import Foundation


/// Pure geo math for GPS workouts — no platform types, fully unit-testable. A Swift port of Android
/// `RouteMath`: Haversine distance, pace, and precision-5 encoded polylines, so a route is identical on
/// both platforms.
enum RouteMath {

    /// One geographic point. Plain `Double` lat/lon — not a `CLLocationCoordinate2D` — so the math stays
    /// platform-free and testable without CoreLocation.
    struct LatLng: Equatable {
        let lat: Double
        let lon: Double
        init(_ lat: Double, _ lon: Double) { self.lat = lat; self.lon = lon }
    }

    private static let earthR = 6_371_000.0 // metres

    /// Great-circle distance between two points, in metres (Haversine). Matches Android exactly.
    static func haversineMeters(_ a: LatLng, _ b: LatLng) -> Double {
        let dLat = (b.lat - a.lat) * .pi / 180
        let dLon = (b.lon - a.lon) * .pi / 180
        let s = sin(dLat / 2) * sin(dLat / 2)
            + cos(a.lat * .pi / 180) * cos(b.lat * .pi / 180) * sin(dLon / 2) * sin(dLon / 2)
        return earthR * 2 * atan2(sqrt(s), sqrt(1 - s))
    }

    /// Total route length in metres — the sum of consecutive Haversine legs. 0 for < 2 points.
    static func totalMeters(_ points: [LatLng]) -> Double {
        guard points.count > 1 else { return 0 }
        var sum = 0.0
        for i in 1..<points.count { sum += haversineMeters(points[i - 1], points[i]) }
        return sum
    }

    /// Seconds per kilometre, or nil when distance is zero (pace undefined). Matches Android.
    static func paceSecPerKm(meters: Double, seconds: Double) -> Double? {
        meters <= 0 ? nil : seconds / (meters / 1000.0)
    }

    // MARK: Encoded Polyline Algorithm Format, precision 5

    /// Encode a route to a compact polyline string (Google precision-5 format). Identical output to
    /// Android `RouteMath.encode` for the same points, so the stored string is cross-platform.
    static func encode(_ points: [LatLng]) -> String {
        var out = ""
        var prevLat = 0, prevLon = 0
        for p in points {
            let lat = Int((p.lat * 1e5).rounded())
            let lon = Int((p.lon * 1e5).rounded())
            encodeSigned(lat - prevLat, into: &out)
            encodeSigned(lon - prevLon, into: &out)
            prevLat = lat
            prevLon = lon
        }
        return out
    }

    /// Decode a polyline string back to points. Bounds-checked: an untrusted / corrupt string yields the
    /// points it can parse and stops cleanly at the end (never reads past the buffer). Matches Android.
    static func decode(_ encoded: String) -> [LatLng] {
        var out: [LatLng] = []
        let chars = Array(encoded.unicodeScalars)
        var i = 0
        var lat = 0, lon = 0
        while i < chars.count {
            guard let dLat = decodeSigned(chars, &i) else { break }
            guard let dLon = decodeSigned(chars, &i) else { break }
            lat += dLat
            lon += dLon
            let p = LatLng(Double(lat) / 1e5, Double(lon) / 1e5)
            // Drop anything off the surface of the earth — a defensive gate on a decoded-from-disk value.
            if (-90...90).contains(p.lat) && (-180...180).contains(p.lon) { out.append(p) }
        }
        return out
    }

    private static func encodeSigned(_ v: Int, into out: inout String) {
        var value = v < 0 ? ~(v << 1) : (v << 1)
        while value >= 0x20 {
            out.unicodeScalars.append(Unicode.Scalar((0x20 | (value & 0x1f)) + 63)!)
            value >>= 5
        }
        out.unicodeScalars.append(Unicode.Scalar(value + 63)!)
    }

    /// Decode one signed varint starting at `i`, advancing `i`. Returns nil if the buffer ends mid-value
    /// (a truncated / corrupt string) so the caller stops rather than over-reading.
    private static func decodeSigned(_ chars: [Unicode.Scalar], _ i: inout Int) -> Int? {
        var result = 0, shift = 0, b = 0
        repeat {
            guard i < chars.count else { return nil }
            b = Int(chars[i].value) - 63
            // A byte must be a valid polyline character (>= 63 once 63 is subtracted, i.e. >= 0).
            guard b >= 0 else { return nil }
            i += 1
            result |= (b & 0x1f) << shift
            shift += 5
            // Guard against a maliciously long run inflating `shift` past Int width.
            if shift > 35 { return nil }
        } while b >= 0x20
        return (result & 1) != 0 ? ~(result >> 1) : (result >> 1)
    }
}

// MARK: - TrackFilter (pure; Android parity)

/// A raw GPS reading before filtering. Mirrors Android `RawFix`.
struct RawFix: Equatable {
    let lat: Double
    let lon: Double
    let accuracyM: Double   // horizontal accuracy radius; < 0 from CoreLocation means "invalid"
    let tMs: Int64          // fix time, ms since epoch
}

/// Pure, stateful fix gate: drops low-accuracy fixes and physically-impossible jumps, returning the
/// accepted point or nil. Keeps the last accepted fix to gate the next. A direct port of Android
/// `TrackFilter` (so a weak-signal run admits the same legitimate fixes and rejects the same teleports).
final class TrackFilter {
    // 50 m is the realistic consumer-GPS gate during activity (Strava-class apps use ~50 m). The speed
    // gate below still rejects teleports, so the looser accuracy gate admits legitimate running fixes
    // without letting GPS jumps inflate the track. Identical thresholds to Android (#324).
    private let maxAccuracyM: Double
    private let maxSpeedMps: Double   // ~43 km/h; well above running, below GPS teleports
    private var last: RawFix?

    init(maxAccuracyM: Double = 50, maxSpeedMps: Double = 12) {
        self.maxAccuracyM = maxAccuracyM
        self.maxSpeedMps = maxSpeedMps
    }

    /// Accept a fix or reject it (nil). Rejects: an invalid / too-coarse accuracy, an out-of-range
    /// coordinate, or a jump from the last accepted fix faster than `maxSpeedMps`.
    func accept(_ fix: RawFix) -> RouteMath.LatLng? {
        // CoreLocation reports a negative horizontalAccuracy when the fix is invalid; treat that as a
        // drop, same as an over-coarse reading. (Android sees 0 for "no accuracy"; we treat < 0 as bad.)
        if fix.accuracyM < 0 || fix.accuracyM > maxAccuracyM { return nil }
        guard (-90...90).contains(fix.lat), (-180...180).contains(fix.lon) else { return nil }
        if let prev = last {
            let dt = Double(fix.tMs - prev.tMs) / 1000.0
            if dt > 0 {
                let d = RouteMath.haversineMeters(RouteMath.LatLng(prev.lat, prev.lon),
                                                  RouteMath.LatLng(fix.lat, fix.lon))
                if d / dt > maxSpeedMps { return nil }
            }
        }
        last = fix
        return RouteMath.LatLng(fix.lat, fix.lon)
    }
}

// MARK: - RouteStore (on-device side-store)

/// The route persisted for one finished workout: the encoded polyline + the GPS distance it implies.
/// A tiny `Codable` value, the unit a `RouteStore` keys by a workout's natural key.
struct WorkoutRoute: Equatable, Codable {
    /// Google precision-5 polyline of the captured route (`RouteMath.encode`).
    var polyline: String
    /// Total GPS distance in metres (`RouteMath.totalMeters` of the captured points).
    var distanceM: Double
}

/// On-device persistence for finished GPS routes, keyed by a workout's natural key (startTs + sport) so a
/// saved row can look its route back up. The shared `WhoopStore.WorkoutRow` carries no route column on
/// Apple, so the polyline lives here — mirroring how `moments` / `sleepMarks` / the active-workout
/// snapshot already persist to `UserDefaults`. Never leaves the device.
///
/// The codec (`encodeMap` / `decodeMap`) is pure (no `UserDefaults` dependency) so the persist round-trip
/// is unit-testable; `store` / `load` / `remove` just thread a `UserDefaults` through it. Bounded so a
/// long-lived install can't grow the blob without limit.
enum RouteStore {

    /// Single `UserDefaults` key holding a JSON `[key: WorkoutRoute]` map. Namespaced like `moments`.
    static let defaultsKey = "noop.workoutRoutes"

    /// Cap on stored routes — newest kept, oldest evicted (keys sort by the leading startTs). A route is
    /// a handful of bytes, but this keeps the map from growing unboundedly across an install's lifetime.
    static let maxRoutes = 400

    /// Natural key for a session's route: "<startTs>|<sport>". Stable across launches; matches the
    /// `WorkoutRow` natural key the detail screen reads. Sport is included so two sessions that share a
    /// start second (different sports) never collide.
    static func key(startTs: Int, sport: String) -> String { "\(startTs)|\(sport)" }

    /// Encode a route map to JSON. Drops any entry whose distance isn't a finite, non-negative number
    /// FIRST — JSON has no representation for NaN/Inf, so a single corrupt distance would otherwise make
    /// `JSONEncoder` throw and take the WHOLE map down with it (silently losing every valid route). We
    /// sanitise per-entry so one bad row can never poison the blob. Returns nil only if encoding the
    /// already-clean map fails (never expected for this shape).
    static func encodeMap(_ map: [String: WorkoutRoute]) -> Data? {
        let clean = map.filter { $0.value.distanceM.isFinite && $0.value.distanceM >= 0 }
        return try? JSONEncoder().encode(clean)
    }

    /// Decode a route map from JSON, dropping any entry whose distance isn't a finite, non-negative
    /// number — never trust the persisted blob to be clean (belt-and-braces with `encodeMap`'s sanitise).
    /// nil/garbage yields an empty map.
    static func decodeMap(_ data: Data?) -> [String: WorkoutRoute] {
        guard let data, !data.isEmpty,
              let raw = try? JSONDecoder().decode([String: WorkoutRoute].self, from: data) else { return [:] }
        return raw.filter { $0.value.distanceM.isFinite && $0.value.distanceM >= 0 }
    }

    /// Read the whole stored map (bound-checked).
    static func loadMap(from defaults: UserDefaults = .standard) -> [String: WorkoutRoute] {
        decodeMap(defaults.data(forKey: defaultsKey))
    }

    /// The route for a finished workout, or nil if none was recorded.
    static func load(startTs: Int, sport: String, from defaults: UserDefaults = .standard) -> WorkoutRoute? {
        loadMap(from: defaults)[key(startTs: startTs, sport: sport)]
    }

    /// Persist `route` for a workout, evicting the oldest entries if the cap is exceeded. A no-op when the
    /// route has no usable polyline (so we never store an empty placeholder — honest "no route").
    static func store(_ route: WorkoutRoute, startTs: Int, sport: String,
                      into defaults: UserDefaults = .standard) {
        guard !route.polyline.isEmpty else { return }
        var map = loadMap(from: defaults)
        map[key(startTs: startTs, sport: sport)] = route
        if map.count > maxRoutes {
            // Keys lead with the startTs, so a lexicographic sort by the numeric prefix evicts the oldest.
            let ordered = map.keys.sorted { lhs, rhs in
                (Int(lhs.split(separator: "|").first ?? "") ?? 0)
                    < (Int(rhs.split(separator: "|").first ?? "") ?? 0)
            }
            for k in ordered.prefix(map.count - maxRoutes) { map.removeValue(forKey: k) }
        }
        guard let data = encodeMap(map) else { return }
        defaults.set(data, forKey: defaultsKey)
    }

    /// Remove a workout's route (used when a session is deleted; keeps the side-store from leaking).
    static func remove(startTs: Int, sport: String, from defaults: UserDefaults = .standard) {
        var map = loadMap(from: defaults)
        guard map.removeValue(forKey: key(startTs: startTs, sport: sport)) != nil,
              let data = encodeMap(map) else { return }
        defaults.set(data, forKey: defaultsKey)
    }
}
