package com.sennnen.mav

/** UI-facing runtime state: the opened core runtime, or a coded failure. */
sealed interface MavAppState {
    data object Loading : MavAppState
    data object Ready : MavAppState
    data class Failed(
        val code: String,
        val message: String,
    ) : MavAppState
}
