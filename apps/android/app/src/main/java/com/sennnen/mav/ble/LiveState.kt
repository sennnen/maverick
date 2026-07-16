package com.sennnen.mav.ble

/**
 * Immutable snapshot of the live connection + biometric state.
 *
 * Direct port of Strand's `LiveState` (Strand/BLE/LiveState.swift), reduced to the fields the
 * Android UI consumes. Where the Swift app used an `@Published` ObservableObject with closures
 * (`onDoubleTap`, `onWristChange`), the Android port surfaces the most-recent physical input through
 * [lastEvent] and exposes wrist-wear through [worn]; the ViewModel reacts to changes in this flow.
 *
 *  - [connected]   GATT connection is up (CBPeripheral didConnect)
 *  - [bonded]      one confirmed write to the command char has been ACKed (the WHOOP "bond")
 *  - [heartRate]   most-recent plausible BPM (30..220) from the standard 0x2A37 profile OR the
 *                  custom REALTIME_DATA frame
 *  - [rr]          most-recent R-R intervals (ms); the standard profile is the reliable source
 *  - [batteryPct]  battery percent — 5/MG: 0x2A19 whole %; WHOOP 4: GET_BATTERY_LEVEL response u16/10
 *                  (the 4.0's 0x2A19 is a stub constant 100 and is ignored, #77)
 *  - [worn]        wrist-wear from WRIST_ON/WRIST_OFF events; defaults true (Swift parity) so
 *                  wear-gated features work before the first event lands
 *  - [lastEvent]   the most-recent strap EVENT string ("WRIST_ON(9)", "DOUBLE_TAP(14)", …)
 */
data class LiveState(
    val connected: Boolean = false,
    val bonded: Boolean = false,
    /** True ONLY when the link reached a GENUINE encrypted bond — the 5/MG CLIENT_HELLO ack, the WHOOP4
     *  confirmed-write bond, or a strap-reported BLE_BONDED event. NOT set by the live-HR shortcut that
     *  flips [bonded] true when HR streams over the unbonded standard profile on a 5/MG (#69) — so
     *  [bonded] can be true while this is false ("Live HR, not fully paired"). WHOOP 4 always reaches a
     *  genuine bond, so the two track together there. Port of macOS LiveState.encryptedBond. */
    val encryptedBond: Boolean = false,
    val heartRate: Int? = null,
    val rr: List<Int> = emptyList(),
    /** Rolling UI buffer of recent R-R intervals (capped, oldest dropped first). The standard BLE HR
     *  notification usually carries only one or two intervals per packet, so the Live console needs a
     *  short history to render a moving R-R strip / rolling RMSSD. Appended (never replaced) via
     *  [withRRIntervals]; emptied by [clearedBiometrics]. Twin of macOS LiveState.rrRecent (PR#191). */
    val rrRecent: List<Int> = emptyList(),
    val batteryPct: Double? = null,
    /** Strap firmware version captured during the connect handshake: WHOOP 4.0 reports `fw_harvard`
     *  (a.b.c.d) via REPORT_VERSION_INFO, WHOOP 5/MG reports `fw_version` via GET_HELLO. Shown on the
     *  Devices card. Null until the handshake response decodes. The Swift WhoopProtocol decodes the
     *  same fields; this is the Android send → state → UI wiring. */
    val strapFirmware: String? = null,
    /** Charging flag from BATTERY_LEVEL events — wire observation: u8 bit0 (4.0 @26 / 5.0 @30,
     *  ~every 8 min on captured links). Flag only; battery % keeps its family source (#77).
     *  Cleared on disconnect so a stale flag can't outlive the link. Twin of macOS
     *  LiveState.charging. */
    val charging: Boolean? = null,
    /** Wrist-wear from WRIST_ON/WRIST_OFF events. Defaults TRUE to match the macOS LiveState (Swift
     *  parity) — assume worn until the strap says otherwise. (Was false, which made the UI show
     *  "Worn: Off" forever when no WRIST_ON event arrived — issue #18.) */
    val worn: Boolean = true,
    val lastEvent: String? = null,
    /** The strap's current BLE advertising name (the WHOOP 4.0 device name from the OS), captured on
     *  connect. Drives the "Rename strap" card in Settings → Strap. Null until connected. */
    val advertisingName: String? = null,
    /** Status of the last strap-rename attempt (sent / validation reason), surfaced in Settings → Strap.
     *  Replaced by the next attempt. Twin of macOS LiveState.renameStatus. */
    val renameStatus: String? = null,
    /** True while actively scanning for the strap (so the UI can show "Searching…"). */
    val scanning: Boolean = false,
    /** Human-readable reason for the current state (why it can't connect, what to try). */
    val statusNote: String? = null,
    /** A WHOOP 5/MG strap was found. It connects and its battery reads, but live data needs an
     *  MG secure handshake that isn't supported yet — so the UI explains that honestly instead of
     *  showing the generic "charge it and put it on" checklist. */
    val whoop5Detected: Boolean = false,
    /** True while a historical offload session is running, so screens can say "Syncing strap
     *  history…" instead of presenting half-loaded data as final (#77). */
    val backfilling: Boolean = false,
    /** Chunks acked during the current offload session — an honest progress signal (total pending is
     *  unknowable from the protocol, so no percent). Republished every ~10 chunks: the foreground
     *  service re-posts its notification on EVERY LiveState emission, so per-chunk would spam it. */
    val syncChunksThisSession: Int = 0,
    /** Wall-clock (unix seconds) of the last offload that ran to HISTORY_COMPLETE, or null if none
     *  this process. For a cloud-free app this is the honest "is sync actually working?" answer — the
     *  UI renders it as a relative "Last synced N ago". (PR #85) */
    val lastSyncAt: Long? = null,
    /** Set when an offload ended abnormally (strap went quiet mid-sync / idle-watchdog fired), so a
     *  stalled history download isn't silent. Cleared on the next successful HISTORY_COMPLETE. (PR #85) */
    val lastSyncError: String? = null,
    /** Set when a connect attempt fails because the strap wiped its Bluetooth bond — a firmware reset,
     *  or the official WHOOP app re-bonding it. The OS still holds a now-stale bond, so retrying the
     *  direct connect just re-fails. Carries an actionable forget+re-pair guide; cleared on the next
     *  successful connect. Parity with macOS LiveState.reconnectGuide (5/MG firmware reset, 2026-06). */
    val reconnectGuide: String? = null,
    /** Set when a WHOOP 5/MG strap keeps REFUSING the encrypted bond on connect (the strap is still
     *  bonded to the official WHOOP app, so a fresh just-works bond can't start). Carries concrete
     *  pairing-mode guidance; published once the refusal streak reaches two and cleared on a genuine
     *  bond or a fresh user-initiated connect. Parity with macOS LiveState.pairingHint (#78). The same
     *  text is mirrored into [statusNote] so the existing Live status surface shows it with no UI change. */
    val pairingHint: String? = null,
    /** EXPERIMENTAL R22 telemetry (#174): how many of the 15 enable_r22 SET_CONFIG flags the strap has
     *  ACKed since the last "Send enable sequence" tap. 15 = the strap accepted the whole sequence (it
     *  returns a COMMAND_RESPONSE per flag — hardware-confirmed). Reset per attempt + per session.
     *  Twin of macOS LiveState.r22FlagsAccepted. */
    val r22FlagsAccepted: Int = 0,
    /** Count of type-0x2F records seen this session OUTSIDE our own history offload. #494 showed these are
     *  historical-offload data (e.g. another BLE client pulling the strap's backlog over the shared notify
     *  channel), NOT a separate live R22 stream — type-0x2F is only ever the historical offload. Kept as a
     *  diagnostic counter, not a "deep stream unlocked" signal. Twin of macOS LiveState.deepPacketsThisSession. (#174) */
    val deepPacketsThisSession: Int = 0,
    /** #580: TRUE when a connected WHOOP 5/MG is streaming live HR fine but its firmware hands over NO
     *  history offload (it acks SEND_HISTORICAL_DATA but emits zero type-0x2F frames). The home/Settings
     *  surface then reads "connected, history sync experimental on 5.0" instead of a sync error, and the
     *  120s liveness bounce backs off so a healthy link isn't disconnected/rescanned every ~2 min. Set
     *  once empty offloads are SUSTAINED; cleared on connect or once the strap banks real records. Twin of
     *  macOS LiveState.historySyncExperimental. */
    val historySyncExperimental: Boolean = false,
) {
    /** Set the fresh-packet [rr] AND append the valid intervals onto the bounded [rrRecent] rolling
     *  buffer (oldest fall off first). Non-positive sentinels are dropped from the rolling buffer.
     *  Twin of macOS LiveState.setRRIntervals (PR#191). */
    fun withRRIntervals(intervals: List<Int>, recentLimit: Int = 60): LiveState {
        val valid = intervals.filter { it > 0 }
        if (valid.isEmpty()) return copy(rr = intervals)
        val merged = rrRecent + valid
        val capped = if (merged.size > recentLimit) merged.takeLast(recentLimit) else merged
        return copy(rr = intervals, rrRecent = capped)
    }

    /** Blank all live biometric readouts (HR + R-R + the rolling buffer) so a stale heart rate or R-R
     *  strip can't outlive the link. Applied on disconnect alongside the charging/bond clears. Twin of
     *  macOS LiveState.clearBiometrics (PR#191). */
    fun clearedBiometrics(): LiveState = copy(heartRate = null, rr = emptyList(), rrRecent = emptyList())
}
