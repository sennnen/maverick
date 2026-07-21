package com.sennnen.mav.ble

/**
 * Device-neutral live presentation state. Protocol state stays inside the active connector and
 * transport state stays in the generic host; this record contains only facts the UI can render.
 */
data class LiveState(
    val connected: Boolean = false,
    val bonded: Boolean = false,
    val heartRate: Int? = null,
    val batteryPct: Double? = null,
    val charging: Boolean? = null,
    val worn: Boolean = true,
    val advertisingName: String? = null,
    val scanning: Boolean = false,
    val statusNote: String? = null,
)
