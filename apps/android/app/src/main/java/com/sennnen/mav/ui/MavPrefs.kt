package com.sennnen.mav.ui

import android.content.Context
import android.content.SharedPreferences

/** Shared accessor for the onboarding / changelog flags (the macOS @AppStorage equivalent). */
object MavPrefs {
    const val NAME = "mav_prefs"
    const val KEY_ONBOARDED = "mav.onboarded"
    const val KEY_LAST_SEEN_CHANGELOG = "mav.lastSeenChangelogVersion"
    /** Terms-of-use version the user last accepted. Empty until the first-run gate is accepted; a
     *  material terms change bumps [Terms.CURRENT_VERSION] and re-prompts. Mirrors macOS @AppStorage. */
    const val KEY_ACCEPTED_TERMS_VERSION = "mav.acceptedTermsVersion"
    /** ISO-8601 timestamp of the last terms acceptance — the on-device consent record (version + when). */
    const val KEY_ACCEPTED_TERMS_AT = "mav.acceptedTermsAt"

    /** "Keep connected in the background", drives [com.sennnen.mav.ble.WhoopConnectionService]. Default on. */
    const val KEY_BACKGROUND_CONNECTION = "mav.backgroundConnection"

    /** "Continuous HRV capture", when on (AND background connection is on), Maverick holds the dense
     *  realtime HR stream armed even with no Live screen open, so the strap banks beat-to-beat R-R 24/7
     *  for far better overnight HRV/recovery/sleep. Uses more battery (continuous HR streaming). Default
     *  OFF. Drives [com.sennnen.mav.ble.WhoopBleClient.setKeepStreamForData] via [AppViewModel]. */
    const val KEY_CONTINUOUS_HRV = "mav.continuousHrv"

    /** "Overnight only" refinement of Continuous HRV capture (#927): when on (with [KEY_CONTINUOUS_HRV]),
     *  the dense realtime stream is armed only inside the nightly quiet-hours window (22:00 to 07:00 by
     *  default, wrap-aware, local wall time) instead of 24/7, roughly halving the battery cost. Default
     *  OFF, so existing Continuous HRV users keep the always-on behaviour with no migration. Read by
     *  [com.sennnen.mav.ble.WhoopBleClient] at every arm site (re-derived at arm time, never cached). */
    const val KEY_CONTINUOUS_HRV_OVERNIGHT = "mav.continuousHrvOvernight"

    /** The calendar day (yyyy-MM-dd) on which the morning-journal nudge was last shown, keeps the
     *  Sleep screen's "Good morning" sheet to at most once per day. */
    const val KEY_LAST_JOURNAL_PROMPT = "mav.lastJournalPromptDay"

    /** "Debug logging", when on, the strap log is also written to logcat (`adb`). Default OFF so a
     *  normal user never emits the connection log to the system log; the in-app ring buffer (and the
     *  "Share strap log" export) work regardless. See [com.sennnen.mav.ble.WhoopBleClient.debugLogcat]. */
    const val KEY_DEBUG_LOGGING = "mav.debugLogging"

    /** "Broadcast heart rate", when on, Maverick acts as a standard BLE Heart Rate peripheral (0x180D /
     *  0x2A37) and re-broadcasts the live strap HR so a gym treadmill / Zwift / Peloton can read it.
     *  LOCAL Bluetooth only, nothing leaves the device. Default OFF. Drives [com.sennnen.mav.ble.HrBroadcaster]
     *  via [AppViewModel]. Distinct from the WHOOP strap's own "broadcast HR" firmware config. */
    const val KEY_HR_BROADCAST = "mav.hrBroadcast"

    const val KEY_ANALYZE_WATERMARK = "mav.analyzeWatermark"

    fun of(context: Context): SharedPreferences =
        context.getSharedPreferences(NAME, Context.MODE_PRIVATE)

    /** #836, the raw-HR fingerprint ("count:maxTs") the last COMPLETED idle rescore scored against. The
     *  15-min backstop tick skips when the current fingerprint equals this; cleared implicitly by any HR
     *  insert/delete (the fingerprint moves). Mirrors the Swift `analyzeWatermark` UserDefaults key. */
    fun analyzeWatermark(context: Context): String? =
        of(context).getString(KEY_ANALYZE_WATERMARK, null)

    fun setAnalyzeWatermark(context: Context, fingerprint: String) {
        of(context).edit().putString(KEY_ANALYZE_WATERMARK, fingerprint).apply()
    }

    /** Whether Maverick should hold the strap connection open via a foreground service. Default true. */
    fun backgroundConnection(context: Context): Boolean =
        of(context).getBoolean(KEY_BACKGROUND_CONNECTION, true)

    fun setBackgroundConnection(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_BACKGROUND_CONNECTION, enabled).apply()
    }

    /** Whether Maverick keeps the dense realtime HR stream armed 24/7 for continuous HRV capture. Default
     *  false. Only takes effect while [backgroundConnection] is also on. */
    fun continuousHrv(context: Context): Boolean =
        of(context).getBoolean(KEY_CONTINUOUS_HRV, false)

    fun setContinuousHrv(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_CONTINUOUS_HRV, enabled).apply()
    }

    /** Whether Continuous HRV capture arms the stream only inside the nightly window (#927). Default
     *  false = always-on, the pre-#927 behaviour. */
    fun continuousHrvOvernight(context: Context): Boolean =
        of(context).getBoolean(KEY_CONTINUOUS_HRV_OVERNIGHT, false)

    fun setContinuousHrvOvernight(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_CONTINUOUS_HRV_OVERNIGHT, enabled).apply()
    }

    /** Whether the strap log is mirrored to logcat. Default false (normal users don't log to adb). */
    fun debugLogging(context: Context): Boolean =
        of(context).getBoolean(KEY_DEBUG_LOGGING, false)

    fun setDebugLogging(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_DEBUG_LOGGING, enabled).apply()
    }

    /** Whether Maverick re-broadcasts its live HR as a standard BLE Heart Rate peripheral. Default OFF. */
    fun hrBroadcast(context: Context): Boolean =
        of(context).getBoolean(KEY_HR_BROADCAST, false)

    fun setHrBroadcast(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_HR_BROADCAST, enabled).apply()
    }

    /** "Buzz WHOOP 4" (#536): arm the strap's firmware alarm at the phone smart alarm's earliest wake
     *  time, so the strap buzzes first and the OS alarm fires at the hard deadline as backup. Default OFF. */
    const val KEY_BUZZ_WHOOP4_WITH_ALARM = "mav.buzzWhoop4WithAlarm"
    fun buzzWhoop4WithAlarm(context: Context): Boolean =
        of(context).getBoolean(KEY_BUZZ_WHOOP4_WITH_ALARM, false)

    fun setBuzzWhoop4WithAlarm(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_BUZZ_WHOOP4_WITH_ALARM, enabled).apply()
    }

    /** Launcher-icon preference (v3 "Titanium & Gold"). false = machined-titanium (.IconDefault,
     *  the default); true = blued/dark-blue titanium (.IconNavy). The actual swap is done by
     *  enabling exactly one of the two <activity-alias> entries via PackageManager, this bool just
     *  records the user's choice so the App Icon control reflects it across restarts. */
    const val KEY_APP_ICON_NAVY = "mav.appIconNavy"

    fun appIconNavy(context: Context): Boolean =
        of(context).getBoolean(KEY_APP_ICON_NAVY, false)

    fun setAppIconNavy(context: Context, navy: Boolean) {
        of(context).edit().putBoolean(KEY_APP_ICON_NAVY, navy).apply()
    }

    /** Imperial/Metric display preference (D#103). Display-only, stored data stays SI. The length/mass
     *  system is read by [UnitPrefs.system]; the temperature override (empty = "match the system") by
     *  [UnitPrefs.temperature]. Mirrors macOS @AppStorage("units.system" / "units.temperature"). */
    const val KEY_UNIT_SYSTEM = "units.system"
    const val KEY_TEMPERATURE_UNIT = "units.temperature"

    fun setUnitSystem(context: Context, system: UnitSystem) {
        of(context).edit().putString(KEY_UNIT_SYSTEM, system.raw).apply()
    }

    /** Persist the temperature override, or pass null to clear it back to "match the system". */
    fun setTemperatureUnit(context: Context, unit: TemperatureUnit?) {
        of(context).edit().apply {
            if (unit == null) remove(KEY_TEMPERATURE_UNIT) else putString(KEY_TEMPERATURE_UNIT, unit.raw)
        }.apply()
    }

    /** Health Connect periodic auto-sync (Samsung Health → Health Connect → Maverick). Default OFF.
     *  Interval in hours (default 12). Last successful sync as epoch millis (0 = never). */
    const val KEY_HC_AUTO_SYNC = "mav.hcAutoSync"
    const val KEY_HC_SYNC_HOURS = "mav.hcSyncHours"
    const val KEY_HC_LAST_SYNC = "mav.hcLastSync"

    fun hcAutoSync(context: Context): Boolean =
        of(context).getBoolean(KEY_HC_AUTO_SYNC, false)

    fun setHcAutoSync(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_HC_AUTO_SYNC, enabled).apply()
    }

    fun hcSyncHours(context: Context): Int =
        of(context).getInt(KEY_HC_SYNC_HOURS, 12)

    fun setHcSyncHours(context: Context, hours: Int) {
        of(context).edit().putInt(KEY_HC_SYNC_HOURS, hours).apply()
    }

    fun hcLastSync(context: Context): Long =
        of(context).getLong(KEY_HC_LAST_SYNC, 0L)

    fun setHcLastSync(context: Context, epochMs: Long) {
        of(context).edit().putLong(KEY_HC_LAST_SYNC, epochMs).apply()
    }

    /** Health Connect writeback (Maverick's computed metrics → HC, for other apps). Default OFF. */
    const val KEY_HC_WRITEBACK = "mav.hcWriteback"

    fun hcWriteback(context: Context): Boolean =
        of(context).getBoolean(KEY_HC_WRITEBACK, false)

    fun setHcWriteback(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_HC_WRITEBACK, enabled).apply()
    }

    /** #528, last HR sample epoch-second exported to Health Connect (0 = nothing exported yet). The
     *  HR share-back only emits samples newer than this, so each writeback is incremental. */
    const val KEY_HC_HR_FRONTIER = "mav.hcHrFrontierTs"

    fun hcHrFrontier(context: Context): Long =
        of(context).getLong(KEY_HC_HR_FRONTIER, 0L)

    fun setHcHrFrontier(context: Context, tsSec: Long) {
        of(context).edit().putLong(KEY_HC_HR_FRONTIER, tsSec).apply()
    }

    /** Smart alarm: arm the strap's firmware alarm to buzz at a wake time. Default off; default time 07:00. */
    const val KEY_SMART_ALARM = "mav.smartAlarmEnabled"
    const val KEY_SMART_ALARM_MINUTES = "mav.smartAlarmMinutes"

    fun smartAlarmEnabled(context: Context): Boolean =
        of(context).getBoolean(KEY_SMART_ALARM, false)

    fun setSmartAlarmEnabled(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_SMART_ALARM, enabled).apply()
    }

    /** Wake time as minutes since midnight (default 420 = 07:00). */
    fun smartAlarmMinutes(context: Context): Int =
        of(context).getInt(KEY_SMART_ALARM_MINUTES, 7 * 60)

    fun setSmartAlarmMinutes(context: Context, minutes: Int) {
        of(context).edit().putInt(KEY_SMART_ALARM_MINUTES, minutes).apply()
    }

    /** Weekdays the smart alarm fires on (Calendar.DAY_OF_WEEK: 1=Sun … 7=Sat). Empty = every day,      *  the backward-compatible default for anyone upgrading from before per-day scheduling (#539). Stored
     *  as a string set; only valid day numbers (1…7) are kept so a corrupted entry can't schedule a
     *  bogus day. Mirrors macOS `BehaviorStore.smartAlarmWeekdays`. */
    const val KEY_SMART_ALARM_WEEKDAYS = "mav.smartAlarmWeekdays"

    fun smartAlarmWeekdays(context: Context): Set<Int> =
        of(context).getStringSet(KEY_SMART_ALARM_WEEKDAYS, emptySet())
            ?.mapNotNull { it.toIntOrNull() }?.filter { it in 1..7 }?.toSet() ?: emptySet()

    fun setSmartAlarmWeekdays(context: Context, days: Set<Int>) {
        val clean = days.filter { it in 1..7 }.map { it.toString() }.toSet()
        of(context).edit().putStringSet(KEY_SMART_ALARM_WEEKDAYS, clean).apply()
    }

    /** Per-weekday wake-time OVERRIDES (reimpl of @MumiZed's PR #554): a map of Calendar.DAY_OF_WEEK
     *  (1=Sun…7=Sat) → minute-of-day. A day with no entry uses the default [smartAlarmMinutes]. Stored as
     *  a "dow:minute" string set; only valid days (1…7) and minutes [0,1440) survive a load, so a corrupt
     *  entry can never schedule a bogus time. Empty = no overrides (the pre-#554 behaviour). */
    const val KEY_SMART_ALARM_OVERRIDES = "mav.smartAlarmDayOverrides"

    fun smartAlarmDayOverrides(context: Context): Map<Int, Int> =
        of(context).getStringSet(KEY_SMART_ALARM_OVERRIDES, emptySet())
            ?.mapNotNull { entry ->
                val parts = entry.split(":")
                if (parts.size != 2) return@mapNotNull null
                val dow = parts[0].toIntOrNull() ?: return@mapNotNull null
                val min = parts[1].toIntOrNull() ?: return@mapNotNull null
                if (dow !in 1..7 || min !in 0 until 24 * 60) return@mapNotNull null
                dow to min
            }?.toMap() ?: emptyMap()

    fun setSmartAlarmDayOverrides(context: Context, overrides: Map<Int, Int>) {
        val clean = overrides
            .filterKeys { it in 1..7 }
            .filterValues { it in 0 until 24 * 60 }
            .map { (dow, min) -> "$dow:$min" }
            .toSet()
        of(context).edit().putStringSet(KEY_SMART_ALARM_OVERRIDES, clean).apply()
    }

    /** HR-zone haptic coaching: buzz the strap on entering the top zone (ease off) and, when the
     *  recovery buzz is on, on dropping back to Zone 1. Zone-based off the profile's HR-max; mirrors
     *  macOS. Coaching default off; recovery buzz default on (matches macOS's always-both behaviour).
     *  Reimplemented from @cbarrado's PR #350. */
    const val KEY_ZONE_COACHING = "mav.zoneCoaching"
    const val KEY_ZONE_COACH_RECOVERY = "mav.zoneCoachRecovery"

    fun zoneCoaching(context: Context): Boolean =
        of(context).getBoolean(KEY_ZONE_COACHING, false)

    fun setZoneCoaching(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_ZONE_COACHING, enabled).apply()
    }

    /** Whether to also buzz on recovering to Zone 1. Default ON (the macOS behaviour). */
    fun zoneCoachRecovery(context: Context): Boolean =
        of(context).getBoolean(KEY_ZONE_COACH_RECOVERY, true)

    fun setZoneCoachRecovery(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_ZONE_COACH_RECOVERY, enabled).apply()
    }

    /** Illness early-warning (banner + notification). Default ON, the watch has always run on
     *  Android, so this is an opt-OUT; macOS is opt-in (behavior.illnessWatch, default off). */
    const val KEY_ILLNESS_WATCH = "mav.illnessWatch"

    fun illnessWatch(context: Context): Boolean =
        of(context).getBoolean(KEY_ILLNESS_WATCH, true)

    fun setIllnessWatch(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_ILLNESS_WATCH, enabled).apply()
    }

    /** Cycle awareness (v5): read a coarse menstrual-cycle PHASE from the nightly skin-temperature
     *  shift. OPT-IN, default OFF (manual-first ethos), the Health hub's Cycle card only renders once
     *  this is on. Awareness only; never contraception / fertility / diagnosis. */
    const val KEY_CYCLE_TRACKING = "mav.cycleTracking"

    fun cycleTracking(context: Context): Boolean =
        of(context).getBoolean(KEY_CYCLE_TRACKING, false)

    fun setCycleTracking(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_CYCLE_TRACKING, enabled).apply()
    }

    /** Hydration tracking (MVP): an opt-in, on-device-only fluid log with a daily goal + quick-add
     *  buttons. OPT-IN, default OFF (manual-first ethos), the Today "Hydration" card and the detail
     *  feature only appear once this is on. Nothing is synced; the day total lives in the local
     *  metric-series store. */
    const val KEY_HYDRATION_TRACKING = "mav.hydrationTracking"

    fun hydrationTracking(context: Context): Boolean =
        of(context).getBoolean(KEY_HYDRATION_TRACKING, false)

    fun setHydrationTracking(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_HYDRATION_TRACKING, enabled).apply()
    }

    /** "Day-cycle background" (#698): the time-of-day scene (sunrise / day / dusk / night) behind the
     *  Today screen. Default ON, it's the v7 atmosphere. Some people find the moving scene distracting
     *  and want a plain dark canvas, so turning this off makes TodayScreen drop the SceneScreenBackground
     *  and fall back to the flat surface; the cards already sit on an opaque canvas, so they stay just as
     *  readable. Mirrors macOS @AppStorage("mav.showDayCycleBackground"). */
    const val KEY_SHOW_DAY_CYCLE_BACKGROUND = "mav.showDayCycleBackground"

    fun showDayCycleBackground(context: Context): Boolean =
        of(context).getBoolean(KEY_SHOW_DAY_CYCLE_BACKGROUND, true)

    fun setShowDayCycleBackground(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_SHOW_DAY_CYCLE_BACKGROUND, enabled).apply()
    }

    /** Card-surface opacity as a PERCENT (0 = fully see-through, 100 = solid; default 100). Drives the
     *  "Card transparency" setting — every frosted card (Heart Rate, Key Metrics, Recovery Vitals, …)
     *  reads it via [CardAppearance]. Only the glass surface fades; the card content stays readable. */
    const val KEY_CARD_OPACITY = "mav.cardOpacityPercent"

    fun cardOpacityPercent(context: Context): Int =
        of(context).getInt(KEY_CARD_OPACITY, 100).coerceIn(0, 100)

    fun setCardOpacityPercent(context: Context, percent: Int) {
        of(context).edit().putInt(KEY_CARD_OPACITY, percent.coerceIn(0, 100)).apply()
    }

    /** "Sky behind cards" (opt-in, default OFF): extend the day-cycle sky behind the WHOLE Today scroll
     *  (not just the top band) so the Card-transparency slider reveals it under every card. Pairs with
     *  [showDayCycleBackground] — no effect when the scene is off. Read once on Today entry. */
    const val KEY_SKY_BEHIND_CARDS = "mav.skyBehindCards"

    fun skyBehindCards(context: Context): Boolean =
        of(context).getBoolean(KEY_SKY_BEHIND_CARDS, false)

    fun setSkyBehindCards(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_SKY_BEHIND_CARDS, enabled).apply()
    }

    /** Coach on-device signals (v5): when ON, the opt-in BYO-key Coach's grounding context may include a
     *  SUMMARY-ONLY line of on-device correlations + Lab Book markers (no raw egress). A SECOND opt-in on
     *  top of the existing "let the coach use my data" consent. Default OFF, keeps the anonymity posture. */
    const val KEY_COACH_SIGNALS = "mav.coachSignals"

    fun coachSignals(context: Context): Boolean =
        of(context).getBoolean(KEY_COACH_SIGNALS, false)

    fun setCoachSignals(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_COACH_SIGNALS, enabled).apply()
    }

    /** The user's EDITED Coach system prompt. Empty/absent means "use the built-in default". A small,
     *  non-secret text key, read FRESH per request so an edit takes effect on the next message. Mirrors
     *  macOS/iOS UserDefaults "ai.systemPrompt". */
    const val KEY_COACH_SYSTEM_PROMPT = "mav.coachSystemPrompt"

    /** The stored prompt override, or empty string when nothing custom is set. */
    fun coachSystemPrompt(context: Context): String =
        of(context).getString(KEY_COACH_SYSTEM_PROMPT, "").orEmpty()

    /** Persist [prompt] as the prompt override; a blank value clears it (back to default). */
    fun setCoachSystemPrompt(context: Context, prompt: String) {
        val trimmed = prompt.trim()
        if (trimmed.isEmpty()) of(context).edit().remove(KEY_COACH_SYSTEM_PROMPT).apply()
        else of(context).edit().putString(KEY_COACH_SYSTEM_PROMPT, prompt).apply()
    }

    /** "Auto-detect workouts" (MVP, opt-in, on-device, NON-DESTRUCTIVE). When ON, Maverick scans the last
     *  day or two of strap HR for a sustained-elevated bout and surfaces ONE dismissible Today card
     *  suggesting you save it, it NEVER creates a workout on its own (the user taps Save). Default OFF;
     *  when off no detection runs and no card shows. Mirrors macOS/iOS @AppStorage("autoDetectWorkouts"). */
    const val KEY_AUTO_DETECT_WORKOUTS = "mav.autoDetectWorkouts"

    fun autoDetectWorkouts(context: Context): Boolean =
        of(context).getBoolean(KEY_AUTO_DETECT_WORKOUTS, false)

    fun setAutoDetectWorkouts(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_AUTO_DETECT_WORKOUTS, enabled).apply()
    }

    /** Last local day (ISO yyyy-MM-dd) an illness notification was posted, the once-a-day gate,
     *  persisted so the app-open and background-service call sites can't double-post. */
    const val KEY_ILLNESS_LAST_NOTIFIED_DAY = "mav.illnessLastNotifiedDay"

    fun illnessLastNotifiedDay(context: Context): String? =
        of(context).getString(KEY_ILLNESS_LAST_NOTIFIED_DAY, null)

    fun setIllnessLastNotifiedDay(context: Context, day: String) {
        of(context).edit().putString(KEY_ILLNESS_LAST_NOTIFIED_DAY, day).apply()
    }

    /** Battery alerts, low (≤15%) + charge-complete (100%) strap notifications (#368, thanks @ujix).
     *  Default ON; gated here and behind the OS notification permission. */
    const val KEY_BATTERY_ALERTS = "mav.batteryAlerts"

    fun batteryAlerts(context: Context): Boolean =
        of(context).getBoolean(KEY_BATTERY_ALERTS, true)

    fun setBatteryAlerts(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_BATTERY_ALERTS, enabled).apply()
    }

    /** Persisted once-per-crossing flags behind BatteryAlertPolicy, they survive process death so a
     *  battery hovering near a threshold fires exactly once per cycle (low re-arms above 25%, full
     *  re-arms below 100%). */
    const val KEY_BATTERY_LOW_ALERTED = "mav.batteryLowAlerted"
    const val KEY_BATTERY_FULL_ALERTED = "mav.batteryFullAlerted"

    fun batteryLowAlerted(context: Context): Boolean =
        of(context).getBoolean(KEY_BATTERY_LOW_ALERTED, false)

    fun setBatteryLowAlerted(context: Context, alerted: Boolean) {
        of(context).edit().putBoolean(KEY_BATTERY_LOW_ALERTED, alerted).apply()
    }

    fun batteryFullAlerted(context: Context): Boolean =
        of(context).getBoolean(KEY_BATTERY_FULL_ALERTED, false)

    fun setBatteryFullAlerted(context: Context, alerted: Boolean) {
        of(context).edit().putBoolean(KEY_BATTERY_FULL_ALERTED, alerted).apply()
    }

    /** Scheduled report notifications (#517), opt-in, default OFF, no AI. Two independent toggles:
     *  - [KEY_REPORT_MORNING]: a morning recap (Charge + Rest) posted once after a fresh night is
     *    processed. It is NOT alarm-precise, it lands when the next sync + analytics pass completes,
     *    so the copy is honest about timing.
     *  - [KEY_REPORT_WORKOUT]: a post-workout summary (Effort + duration + avg HR) posted when a newly
     *    synced workout is first seen. Same post-sync-timing caveat, a strap-only workout surfaces on
     *    the next history offload, not the instant the session ends.
     *  The dedupe state ([KEY_REPORT_MORNING_DAY] / [KEY_REPORT_LAST_WORKOUT_TS]) survives process death
     *  so the app-open and background call sites can't double-post. Mirrors the BatteryAlert/Illness gate
     *  idiom (a persisted "last fired" marker behind a pure policy object). */
    const val KEY_REPORT_MORNING = "mav.report.morningRecap"
    const val KEY_REPORT_WORKOUT = "mav.report.postWorkout"
    const val KEY_REPORT_MORNING_DAY = "mav.report.lastMorningDay"
    const val KEY_REPORT_LAST_WORKOUT_TS = "mav.report.lastWorkoutTs"

    fun morningReportEnabled(context: Context): Boolean =
        of(context).getBoolean(KEY_REPORT_MORNING, false)

    fun setMorningReportEnabled(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_REPORT_MORNING, enabled).apply()
    }

    fun postWorkoutReportEnabled(context: Context): Boolean =
        of(context).getBoolean(KEY_REPORT_WORKOUT, false)

    fun setPostWorkoutReportEnabled(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_REPORT_WORKOUT, enabled).apply()
    }

    /** Last local day (ISO yyyy-MM-dd) the morning recap was posted, the once-a-day gate. */
    fun reportMorningDay(context: Context): String? =
        of(context).getString(KEY_REPORT_MORNING_DAY, null)

    fun setReportMorningDay(context: Context, day: String) {
        of(context).edit().putString(KEY_REPORT_MORNING_DAY, day).apply()
    }

    /** Start-ts (epoch seconds) of the most recent workout already summarised, only a STRICTLY newer
     *  session fires again, so a re-sync of the same backlog never re-notifies. 0 = none yet. */
    fun reportLastWorkoutTs(context: Context): Long =
        of(context).getLong(KEY_REPORT_LAST_WORKOUT_TS, 0L)

    fun setReportLastWorkoutTs(context: Context, ts: Long) {
        of(context).edit().putLong(KEY_REPORT_LAST_WORKOUT_TS, ts).apply()
    }

    /** Caffeine late-intake nudge (PR#566, mvanhorn), opt-in, default OFF. When on, the Caffeine card
     *  shows a cutoff time (the latest you can have caffeine and still clear it below a target residual by
     *  bedtime) and flags an intake logged after that cutoff. [KEY_CAFFEINE_BEDTIME_MIN] is the user's
     *  bedtime as minutes-since-midnight (default 23:00) the cutoff is computed back from. On-device, no
     *  notification, a quiet inline hint, matching the manual-first caffeine card. */
    const val KEY_CAFFEINE_CUTOFF = "mav.caffeine.cutoffNudge"
    const val KEY_CAFFEINE_BEDTIME_MIN = "mav.caffeine.bedtimeMinutes"

    fun caffeineCutoffEnabled(context: Context): Boolean =
        of(context).getBoolean(KEY_CAFFEINE_CUTOFF, false)

    fun setCaffeineCutoffEnabled(context: Context, enabled: Boolean) {
        of(context).edit().putBoolean(KEY_CAFFEINE_CUTOFF, enabled).apply()
    }

    /** Bedtime as minutes since midnight the caffeine cutoff is reckoned back from (default 1380 = 23:00). */
    fun caffeineBedtimeMinutes(context: Context): Int =
        of(context).getInt(KEY_CAFFEINE_BEDTIME_MIN, 23 * 60)

    fun setCaffeineBedtimeMinutes(context: Context, minutes: Int) {
        of(context).edit().putInt(KEY_CAFFEINE_BEDTIME_MIN, minutes.coerceIn(0, 24 * 60 - 1)).apply()
    }

    /** Whether the one-shot #313 full-history Effort rescore has run. Set true once it completes so the
     *  on-upgrade pass that regenerates deep-history strain on the 0–100 axis never re-runs. */
    const val KEY_EFFORT_RESCORE_DONE = "mav.effortRescore.v313.done"

    fun effortRescoreDone(context: Context): Boolean =
        of(context).getBoolean(KEY_EFFORT_RESCORE_DONE, false)

    fun setEffortRescoreDone(context: Context) {
        of(context).edit().putBoolean(KEY_EFFORT_RESCORE_DONE, true).apply()
    }

    /** Whether the one-shot #547 implausible-timestamp heal has run. Set true once it completes so the
     *  on-upgrade purge of bad-strap-clock rows (far-past / future-dated) never re-runs. Re-running is
     *  harmless (the deletes are idempotent), but the flag avoids the work on every launch. */
    const val KEY_TS_HEAL_DONE = "mav.tsHeal.v547.done"

    fun tsHealDone(context: Context): Boolean =
        of(context).getBoolean(KEY_TS_HEAL_DONE, false)

    fun setTsHealDone(context: Context) {
        of(context).edit().putBoolean(KEY_TS_HEAL_DONE, true).apply()
    }

    /** #547 RE-POLLUTION re-arm: set true by the BLE layer when a sync's ingest gate dropped implausible
     *  (bad-clock) records, so the next analyze tick re-runs the purge even after [KEY_TS_HEAL_DONE] is set,      *  a wandering-clock strap re-sends bad-dated records across syncs, and may have banked similar garbage
     *  on an OLDER build whose gate was weaker. Cleared once the re-heal runs. */
    const val KEY_TS_HEAL_PENDING = "mav.tsHeal.v547.pending"

    fun tsHealPending(context: Context): Boolean =
        of(context).getBoolean(KEY_TS_HEAL_PENDING, false)

    fun setTsHealPending(context: Context, pending: Boolean) {
        of(context).edit().putBoolean(KEY_TS_HEAL_PENDING, pending).apply()
    }


    /** Wall-clock (unix seconds) of the last history offload that ran to HISTORY_COMPLETE. Persisted
     *  (reimpl of @tavelli's PR #556) so the Live screen's "Last synced N ago" SURVIVES a BLE-client
     *  recreation / process restart and stops reverting to "Never". 0 = never synced on this install. */
    const val KEY_LAST_SYNC_AT = "mav.lastSyncAtSec"

    fun lastSyncAt(context: Context): Long = of(context).getLong(KEY_LAST_SYNC_AT, 0L)

    fun setLastSyncAt(context: Context, epochSec: Long) {
        of(context).edit().putLong(KEY_LAST_SYNC_AT, epochSec).apply()
    }

    /** Last-known strap firmware string, persisted on connect so the debug export can name it OFFLINE
     *  (LiveState.strapFirmware is cleared on disconnect and gone in the scheduled/background export). */
    const val KEY_LAST_FIRMWARE = "mav.lastFirmware"

    fun lastFirmware(context: Context): String? = of(context).getString(KEY_LAST_FIRMWARE, null)

    fun setLastFirmware(context: Context, fw: String?) {
        of(context).edit().apply {
            if (fw.isNullOrBlank()) remove(KEY_LAST_FIRMWARE) else putString(KEY_LAST_FIRMWARE, fw)
        }.apply()
    }
}
