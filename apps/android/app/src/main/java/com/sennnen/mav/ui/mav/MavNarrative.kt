package com.sennnen.mav.ui.mav

import com.sennnen.mav.BuildConfig

// The written half of Today. The iOS twin is Model/MavNarrative.swift.
//
// This is the one surface where placeholder copy is allowed, and it is fenced: sample text exists
// only in a debug build and is always badged on screen. A release build with no model wired shows
// the honest not-yet-generated state. On-device generation (Gemini Nano here, Foundation Models on
// iOS) and bring-your-own-key advisor chat are a later lane.

sealed interface MavNarrativeState {
    /** Written on-device from the day's own read models. */
    data class Generated(val title: String, val text: String) : MavNarrativeState

    /** Fixture copy. Debug builds only, always rendered behind a visible SAMPLE badge. */
    data class Sample(val title: String, val text: String) : MavNarrativeState

    /** Nothing has been generated. Says so. */
    data class Unavailable(val reason: String) : MavNarrativeState

    val headline: String?
        get() = when (this) {
            is Generated -> title
            is Sample -> title
            is Unavailable -> null
        }

    val bodyText: String?
        get() = when (this) {
            is Generated -> text
            is Sample -> text
            is Unavailable -> null
        }

    val isSample: Boolean get() = this is Sample
}

/** Keeping this an interface is what lets the model lane land later without touching a screen. */
interface MavNarrativeProvider {
    fun daily(day: String, rows: List<MavMetricRow>): MavNarrativeState
    fun trend(id: String, rows: List<MavMetricRow>): MavNarrativeState
}

object MavStubNarrativeProvider : MavNarrativeProvider {
    const val NOT_GENERATED_YET =
        "On-device summaries are not wired up yet. When they are, this is where the day gets " +
            "explained in words, written on your phone from your own data."

    override fun daily(day: String, rows: List<MavMetricRow>): MavNarrativeState =
        if (BuildConfig.MAV_SHOW_SYNTHETIC_DATA) {
            MavNarrativeState.Sample(
                title = "Your resting pulse is trending lower",
                text = "Your three-week average is down while overnight variability remains steady.",
            )
        } else {
            MavNarrativeState.Unavailable(NOT_GENERATED_YET)
        }

    override fun trend(id: String, rows: List<MavMetricRow>): MavNarrativeState = when {
        !BuildConfig.MAV_SHOW_SYNTHETIC_DATA -> MavNarrativeState.Unavailable(NOT_GENERATED_YET)
        id == "resilience" -> MavNarrativeState.Sample(
            "Recovery has stayed consistent over the past month.", "",
        )
        id == "cardio_load" -> MavNarrativeState.Sample(
            "Training load has increased gradually over eight weeks.", "",
        )
        else -> MavNarrativeState.Unavailable(NOT_GENERATED_YET)
    }
}
