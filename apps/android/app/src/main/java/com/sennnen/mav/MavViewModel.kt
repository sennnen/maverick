package com.sennnen.mav

/** UI-facing runtime state: the decoded host snapshot, or a coded failure. */
sealed interface MavAppState {
    data object Loading : MavAppState
    data class Ready(val snapshot: MavSnapshot) : MavAppState
    data class Failed(
        val code: String,
        val message: String,
    ) : MavAppState
}
