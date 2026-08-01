package com.sennnen.mav.ui.mav

// The sport catalogue.
//
// This used to be a nested Pair literal inside the start screen, which meant the screen was the
// only thing that could answer "does this sport have a route?" — and so nothing asked. The confirm
// screen needs exactly that, because offering GPS on a rowing erg is noise and withholding it on a
// trail run is a missing feature.
//
// The Swift twin is `MavSportCatalog.swift` and carries the same list in the same order. A sport
// that exists on one platform only is a parity break, so the two are asserted against each other by
// name in the parity tests.

/** One activity the wearer can start. */
data class MavSport(
    val name: String,
    val detail: String,
    /**
     * Whether a route and a distance mean anything for this activity. Drives the GPS option and
     * whether `Distance` is offered as an end condition.
     */
    val isDistance: Boolean = false,
    /**
     * Strength is logged rather than timed, so it has no end condition and no zone target — it
     * leaves the cardio flow entirely and opens the logger.
     */
    val isStrength: Boolean = false,
)

/** A named group of sports, in the order the start screen shows them. */
data class MavSportCategory(val title: String, val sports: List<MavSport>)

object MavSportCatalog {

    val categories: List<MavSportCategory> = listOf(
        MavSportCategory(
            "Strength",
            listOf(
                MavSport("Strength training", "Routines, exercises, sets and rest", isStrength = true),
                MavSport("Functional fitness", "Circuits and mixed resistance"),
            ),
        ),
        MavSportCategory(
            "Run & walk",
            listOf(
                MavSport("Outdoor run", "GPS run", isDistance = true),
                MavSport("Treadmill", "Indoor run"),
                MavSport("Walking", "Outdoor or indoor walk", isDistance = true),
                MavSport("Hiking", "Trail and elevation", isDistance = true),
            ),
        ),
        MavSportCategory(
            "Cardio",
            listOf(
                MavSport("Cycling", "Indoor or outdoor", isDistance = true),
                MavSport("Swimming", "Pool or open water"),
                MavSport("Rowing", "Erg or water"),
                MavSport("Elliptical", "Indoor cardio"),
            ),
        ),
        MavSportCategory(
            "Mind & body",
            listOf(
                MavSport("Yoga", "Yoga practice"),
                MavSport("Pilates", "Mat or reformer"),
                MavSport("Mobility", "Movement and recovery"),
            ),
        ),
        MavSportCategory(
            "Sports",
            listOf(
                MavSport("Football", "Training or match"),
                MavSport("Tennis", "Singles or doubles"),
                MavSport("Basketball", "Training or game"),
                MavSport("Boxing", "Bag, pads or sparring"),
            ),
        ),
        MavSportCategory("Other", listOf(MavSport("Other activity", "Anything else"))),
    )

    val all: List<MavSport> = categories.flatMap { it.sports }

    /** Look a sport up by the name a stored config or a running session carries. */
    fun sport(named: String): MavSport? = all.firstOrNull { it.name == named }
}
