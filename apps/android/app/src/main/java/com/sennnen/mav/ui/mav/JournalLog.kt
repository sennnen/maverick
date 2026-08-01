package com.sennnen.mav.ui.mav

import android.content.Context

/** Small local journal bridge until the core journal contract lands on Android. */
object MavJournalLog {
    val questions = listOf(
        "Alcohol",
        "Late meal",
        "Hard training",
        "Travel",
        "Unusual stress",
        "Felt unwell",
    )

    private const val FILE = "mav_prefs"
    private const val PREFIX = "mav.journal."

    fun load(context: Context, day: String): Set<String> =
        context.applicationContext
            .getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .getStringSet(PREFIX + day, emptySet())
            ?.toSet()
            ?: emptySet()

    fun toggle(context: Context, day: String, question: String): Set<String> {
        val next = load(context, day).toMutableSet().apply {
            if (!add(question)) remove(question)
        }
        context.applicationContext
            .getSharedPreferences(FILE, Context.MODE_PRIVATE)
            .edit()
            .putStringSet(PREFIX + day, next)
            .apply()
        return next
    }
}
