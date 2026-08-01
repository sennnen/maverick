package com.sennnen.mav.ui.mav

import android.content.Context
import android.content.SharedPreferences
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit

// Cycle tracking, and the arithmetic behind it. The iOS twin is Model/CycleLog.swift.
//
// Every number here is counted from period starts the user logged themselves. Nothing is inferred
// from a sensor: the core has a `cycle_phase` analytic that reads nightly skin temperature, and when
// that is admitted and available the screen shows it as well, clearly labelled as the core's. The
// two are never blended, because one is a fact about what someone typed and the other is a model
// output.
//
// This is not a medical device, it does not predict fertility, and it does not prevent pregnancy.
// That sentence appears on the screen, not just here.

object MavCycleLog {
    const val DISCLAIMER =
        "Estimates only, counted from the dates you logged. Maverick is not a medical device, " +
            "does not predict fertility, and does not prevent pregnancy."

    private const val FILE = "mav_prefs"
    private const val KEY = "mav.cycle.log"

    private fun prefs(context: Context): SharedPreferences =
        context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    fun load(context: Context): List<String> =
        prefs(context).getString(KEY, "")
            ?.split(",")
            ?.filter { it.isNotBlank() }
            ?.sorted()
            ?: emptyList()

    fun save(context: Context, starts: List<String>) {
        prefs(context).edit().putString(KEY, starts.sorted().joinToString(",")).apply()
    }

    fun logStart(context: Context, day: String): List<String> {
        val starts = load(context)
        if (starts.contains(day)) return starts
        val updated = (starts + day).sorted()
        save(context, updated)
        return updated
    }

    fun removeStart(context: Context, day: String): List<String> {
        val updated = load(context).filterNot { it == day }
        save(context, updated)
        return updated
    }
}

/** The derived view of a cycle log. Pure, so every rule below is a test rather than a screenshot. */
object MavCycle {
    private val formatter: DateTimeFormatter = DateTimeFormatter.ISO_LOCAL_DATE

    fun date(key: String): LocalDate? = runCatching { LocalDate.parse(key, formatter) }.getOrNull()

    fun key(date: LocalDate): String = date.format(formatter)

    /**
     * Whole days between two day keys, counted on the calendar rather than by dividing seconds, so
     * a daylight-saving boundary does not knock the count out by one.
     */
    fun days(from: String, to: String): Int? {
        val start = date(from) ?: return null
        val end = date(to) ?: return null
        return ChronoUnit.DAYS.between(start, end).toInt()
    }

    /** Cycle day, 1-based on the day of the last logged start on or before [day]. */
    fun cycleDay(starts: List<String>, day: String): Int? {
        val start = starts.lastOrNull { it <= day } ?: return null
        return days(start, day)?.plus(1)
    }

    /** Completed cycle lengths, oldest first. A cycle is complete only once the next one started. */
    fun completedLengths(starts: List<String>): List<Int> =
        starts.zipWithNext().mapNotNull { (a, b) -> days(a, b) }

    /**
     * The estimate is a range, from the user's own recent cycles, and it refuses to exist below
     * three completed cycles. Two points is not a pattern, and saying so beats a number.
     */
    fun nextPeriodRange(starts: List<String>): Pair<String, String>? {
        val lengths = completedLengths(starts).takeLast(6)
        if (lengths.size < 3) return null
        val lastStart = starts.lastOrNull() ?: return null
        val lastDate = date(lastStart) ?: return null
        val shortest = lengths.min()
        val longest = lengths.max()
        return key(lastDate.plusDays(shortest.toLong())) to key(lastDate.plusDays(longest.toLong()))
    }

    fun medianLength(starts: List<String>): Int? {
        val lengths = completedLengths(starts).sorted()
        if (lengths.isEmpty()) return null
        return lengths[lengths.size / 2]
    }

    /** How many more cycles are needed before an estimate exists. Null once there are enough. */
    fun cyclesNeeded(starts: List<String>): Int? {
        val have = completedLengths(starts).size
        return if (have >= 3) null else 3 - have
    }
}
