package com.sennnen.mav.ui.mav

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.R
import com.sennnen.mav.ml.MavAnalyticsSnapshot
import com.sennnen.mav.ml.MavSignal
import com.sennnen.mav.ml.MavApplicability
import com.sennnen.mav.ml.MavSignalState
import com.sennnen.mav.ml.MavUnavailable
import com.sennnen.mav.ui.AppViewModel

/**
 * What the on-device models are doing, and why anything absent is absent.
 *
 * The iOS twin is `MavAnalyticsView.swift`. This is a product surface, not a diagnostics screen:
 * it is reached from Today's "More" list beside the report, it is written for a wearer, and every
 * line on it is a fact about their data rather than about the build. It exists because most of the
 * zoo's outputs have no consumer yet — `docs/ml.md` withholds the sleep staging vocabulary and the
 * hypertension risk level, and the majority of models have no ported front-end — and the honest
 * answer to "what is this app doing with all those models" is a screen that says so, per signal,
 * in the wearer's terms.
 *
 * It is built from `MavKit` rather than from a bare `LazyColumn` for the same reason every other
 * pushed screen is: a signal that cannot run renders as [MavUnavailableCard], the same outlined
 * card an unavailable metric gets everywhere else, so absence looks the same wherever a reader
 * meets it.
 *
 * Copy lives in `strings.xml`. Nothing here renders a model output as a health reading; a signal
 * whose vocabulary is not admitted says it was computed and stops.
 */
@Composable
fun MavAnalyticsScreen(viewModel: AppViewModel, onBack: () -> Unit) {
    val snapshot by viewModel.analytics.collectAsStateWithLifecycle()

    MavDetailScaffold(stringResource(R.string.analytics_title), onBack) {
        MavTile {
            Text(
                stringResource(R.string.analytics_subtitle),
                style = MavType.body,
                color = MavTheme.palette.inkSecondary,
            )
        }

        if (snapshot.signals.isEmpty()) {
            MavUnavailableCard(
                name = stringResource(R.string.analytics_title),
                reason = stringResource(
                    if (snapshot.working) R.string.analytics_a11y_working
                    else R.string.analytics_empty,
                ),
            )
        } else {
            for (signal in snapshot.signals) {
                SignalCard(signal = signal, onRetry = viewModel::retryAnalytics)
            }
        }
    }
}

/**
 * One signal, as a card.
 *
 * Two shapes rather than one: a signal nothing can run is an *absence* and gets the outlined
 * unavailable card, which is how absence is drawn everywhere else in this app. Anything else is a
 * live card carrying its state and its coverage.
 */
@Composable
private fun SignalCard(signal: MavSignal, onRetry: () -> Unit) {
    val title = MavSignalCopy.title(LocalContext.current, signal.name)
    val state = signal.state

    if (state is MavSignalState.Unavailable) {
        MavUnavailableCard(name = title, reason = describe(state.reasons))
        return
    }

    val summary = describe(state)
    val coverage = stringResource(R.string.analytics_coverage, signal.runnable, signal.total)
    val spoken = stringResource(R.string.analytics_a11y_signal, title, summary, coverage)
    val palette = MavTheme.palette

    MavStatusCard {
        Column(
            // One announcement for the card's text, and it carries the coverage: merging without
            // it read the title and the state and then dropped the one number that says whether
            // this phone can do the work at all. `mergeDescendants` rather than
            // `clearAndSetSemantics`, which also erased the retry button from the tree and left a
            // TalkBack user with no way to reach it.
            modifier = Modifier
                .fillMaxWidth()
                .semantics(mergeDescendants = true) { contentDescription = spoken },
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(title, style = MavType.label, color = palette.ink)
                if (state is MavSignalState.Working) {
                    CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                }
            }
            Text(summary, style = MavType.body, color = palette.inkSecondary)
            Text(coverage, style = MavType.sub, color = palette.inkSecondary)
        }
        if (state is MavSignalState.Failed && state.retryable) {
            // Outside the merged block above, so the button stays its own focusable, activatable
            // node. Its label names the signal, because "Try again" alone means nothing once the
            // card's text has already been read out.
            val retryLabel = stringResource(R.string.analytics_a11y_retry, title)
            MavQuietButton(
                title = stringResource(R.string.analytics_retry),
                modifier = Modifier
                    .padding(top = 4.dp)
                    .semantics { contentDescription = retryLabel },
                onClick = onRetry,
            )
        }
    }
}

/**
 * One line of copy for one state.
 *
 * Composable so the strings come from resources rather than being built in Kotlin: these are the
 * sentences a wearer reads, and they have to be translatable and reviewable in one place.
 */
@Composable
private fun describe(state: MavSignalState): String = when (state) {
    MavSignalState.Idle -> stringResource(R.string.analytics_state_idle)
    is MavSignalState.Working -> stringResource(
        R.string.analytics_state_working,
        state.done,
        state.total,
    )
    is MavSignalState.Ready ->
        if (!state.displayable) {
            // The model ran and its vocabulary is not admitted. Saying "up to date" here would
            // imply a reading exists to be up to date.
            stringResource(R.string.analytics_state_computed_not_shown)
        } else if (state.applicability == MavApplicability.DEGRADED) {
            stringResource(R.string.analytics_state_ready_partial)
        } else {
            stringResource(R.string.analytics_state_ready)
        }
    is MavSignalState.Stale -> stringResource(R.string.analytics_state_stale)
    // Answered, and answered about padding. The substitution says which fix is available:
    // readings outside the band this model accepts is a different sentence from no readings.
    is MavSignalState.Unfounded ->
        if (state.substitutions.contains("out_of_range")) {
            stringResource(R.string.analytics_state_unfounded_out_of_range)
        } else {
            stringResource(R.string.analytics_state_unfounded_missing)
        }
    MavSignalState.Deferred -> stringResource(R.string.analytics_state_deferred)
    is MavSignalState.Failed -> stringResource(R.string.analytics_state_failed)
    is MavSignalState.PermissionRequired ->
        stringResource(R.string.analytics_permission_required, state.permission)
    is MavSignalState.Unavailable -> describe(state.reasons)
}

/**
 * Why a signal cannot run. The first reason only: the causes are already collapsed to distinct
 * ones by the reducer, and a card that lists four is a card nobody reads.
 */
@Composable
private fun describe(reasons: List<MavUnavailable>): String =
    when (val reason = reasons.firstOrNull()) {
        null -> stringResource(R.string.analytics_state_idle)
        is MavUnavailable.MissingStreams ->
            stringResource(R.string.analytics_needs_sensor, reason.streams.joinToString(", "))
        is MavUnavailable.MissingProfile ->
            stringResource(R.string.analytics_needs_profile, reason.fields.joinToString(", "))
        is MavUnavailable.UpstreamUnavailable -> stringResource(
            R.string.analytics_needs_upstream,
            MavSignalCopy.title(LocalContext.current, reason.model),
        )
        is MavUnavailable.PreprocessingNotPorted ->
            stringResource(R.string.analytics_not_ported, reason.detail)
    }

/**
 * Every wearer-facing name this surface uses for a signal, in one place.
 *
 * Looked up by slug rather than derived from it: `"daytime_hrv"` title-cased is "Daytime Hrv", and
 * a title case applied to an acronym is how a product surface starts looking generated. An unknown
 * slug falls back to the derived form so a newly added signal is legible before its copy lands.
 * The iOS twin is `MavSignalCopy` in `MavAnalyticsView.swift`.
 */
object MavSignalCopy {
    /**
     * One entry per `Signal::name` in `pipeline.rs`. Written out rather than resolved through
     * `Resources.getIdentifier`, which resource shrinking cannot see through and which would
     * strip these strings out of a release build.
     */
    private val NAMES = mapOf(
        "activity" to R.string.analytics_signal_activity,
        "energy_expenditure" to R.string.analytics_signal_energy_expenditure,
        "step_eligibility" to R.string.analytics_signal_step_eligibility,
        "awake_heart_rate" to R.string.analytics_signal_awake_heart_rate,
        "daytime_hrv" to R.string.analytics_signal_daytime_hrv,
        "workout_heart_rate" to R.string.analytics_signal_workout_heart_rate,
        "cardiovascular" to R.string.analytics_signal_cardiovascular,
        "hypertension_risk" to R.string.analytics_signal_hypertension_risk,
        "sleep" to R.string.analytics_signal_sleep,
        "illness_risk" to R.string.analytics_signal_illness_risk,
        "cycle_awareness" to R.string.analytics_signal_cycle_awareness,
        "ppg_foundation" to R.string.analytics_signal_ppg_foundation,
    )

    fun title(context: android.content.Context, slug: String): String =
        NAMES[slug]?.let(context::getString)
            ?: slug.replace('_', ' ').replaceFirstChar { it.uppercase() }

    /**
     * Whether this slug has written-out copy. For the test that pins the list against the signals
     * the core can plan — a new one added in Rust should fail here rather than reach a wearer as a
     * title-cased slug.
     */
    internal fun knows(slug: String): Boolean = slug in NAMES

    /**
     * The one-line summary Today's entry row carries, so the link says something about this
     * wearer's phone rather than only naming a screen.
     */
    fun rowDetail(context: android.content.Context, snapshot: MavAnalyticsSnapshot): String = when {
        snapshot.working -> context.getString(R.string.analytics_a11y_working)
        snapshot.signals.isEmpty() -> context.getString(R.string.analytics_empty)
        else -> context.getString(
            R.string.analytics_coverage,
            snapshot.signals.sumOf { it.runnable },
            snapshot.signals.sumOf { it.total },
        )
    }
}
