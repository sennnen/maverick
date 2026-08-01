package com.sennnen.mav.ui.mav

// The haptic vocabulary, host side. See ADR-032.
//
// A signal is a *meaning*, never a byte pattern. The app asks for "the goal is complete"; the
// connected connector decides what its device does about that, because which characteristic to
// write and with what opcode is device knowledge and does not belong in this binary.
//
// The vocabulary is closed and owned here. A connector declares which of these it can render; it
// cannot invent a new one, because a signal the host cannot name is a signal the host cannot decide
// to send, rate-limit, or explain to the wearer.
//
// The Swift twin is `MavHaptics.swift`.

/** One signal in the closed vocabulary. */
sealed interface MavHapticSignal {
    /** The stable wire name, which is what a manifest declares and what the snapshot lists. */
    val id: String

    /** How the signal is described to the wearer, in the one place it is described. */
    val explanation: String

    /** A light tap: a distance marker passed, or a halfway point reached. */
    data object Milestone : MavHapticSignal {
        override val id = "milestone"
        override val explanation = "A light tap at each milestone"
    }

    /** A hard buzz: the end condition is met. */
    data object GoalComplete : MavHapticSignal {
        override val id = "goal_complete"
        override val explanation = "A strong buzz when you reach your goal"
    }

    /** A light tap confirming a strength set was recorded. */
    data object SetLogged : MavHapticSignal {
        override val id = "set_logged"
        override val explanation = "A light tap when a set is recorded"
    }

    /** A hard buzz: the rest timer is done, start the next set. */
    data object RestComplete : MavHapticSignal {
        override val id = "rest_complete"
        override val explanation = "A strong buzz when rest is over"
    }

    /** A zone boundary was crossed. The zone names itself in the pattern. */
    data class ZoneAlert(val zone: Int) : MavHapticSignal {
        override val id = "zone_alert_$zone"
        override val explanation = "A buzz when you cross into zone $zone"
    }

    companion object {
        val all: List<MavHapticSignal> =
            listOf(Milestone, GoalComplete, SetLogged, RestComplete) + (1..5).map { ZoneAlert(it) }
    }
}

/**
 * What the *connected* connector said it can do.
 *
 * Availability is negotiated rather than assumed. A strap that cannot buzz must never appear to
 * have agreed to: every setting built on a signal reads this first and renders the honest
 * unavailable state when the signal is absent.
 */
data class MavHapticSupport(val signals: Set<String>) {

    val canBuzz: Boolean get() = signals.isNotEmpty()

    fun supports(signal: MavHapticSignal): Boolean = signal.id in signals

    /**
     * The sentence shown where a haptic setting would have been. It names *why*, in the same voice
     * the unavailable-analytic component uses, rather than hiding the control and leaving the
     * wearer wondering where it went.
     */
    fun reason(deviceName: String?): String =
        if (deviceName.isNullOrEmpty()) {
            "No strap is connected, so there is nothing to buzz."
        } else {
            "$deviceName does not report a haptic motor, so it cannot buzz."
        }

    companion object {
        /**
         * Nothing declared. This is the current state of every shipped artifact — the Generic HR
         * Monitor has no haptic characteristic at all — and it stays the value until the
         * `haptics/v1` snapshot block from ADR-032 is plumbed through the core.
         */
        val None = MavHapticSupport(emptySet())
    }
}
