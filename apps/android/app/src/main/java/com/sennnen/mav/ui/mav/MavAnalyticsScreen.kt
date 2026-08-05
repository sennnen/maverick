package com.sennnen.mav.ui.mav

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.R
import com.sennnen.mav.ml.MavSignal
import com.sennnen.mav.ml.MavSignalState
import com.sennnen.mav.ml.MavUnavailable
import com.sennnen.mav.ui.AppViewModel

/**
 * What the on-device models are doing, and why anything absent is absent.
 *
 * This is a product surface, not a diagnostics screen: it is reachable, it is written for a
 * wearer, and every line on it is a fact about their data rather than about the build. It exists
 * because most of the zoo's outputs have no consumer yet — `docs/ml.md` withholds the sleep
 * staging vocabulary and the hypertension risk level, and the majority of models have no ported
 * front-end — and the honest answer to "what is this app doing with forty-one models" is a
 * screen that says so, per signal, in the wearer's terms.
 *
 * Copy is draft and lives in `strings.xml`. Nothing here renders a model output as a health
 * reading; a signal whose vocabulary is not admitted says it was computed and stops.
 */
@Composable
fun MavAnalyticsScreen(viewModel: AppViewModel) {
    val snapshot by viewModel.analytics.collectAsStateWithLifecycle()

    LazyColumn(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Column(modifier = Modifier.padding(vertical = 16.dp)) {
                Text(stringResource(R.string.analytics_title), style = MavType.title)
                Text(
                    stringResource(R.string.analytics_subtitle),
                    style = MavType.caption,
                )
            }
        }
        if (snapshot.signals.isEmpty()) {
            item {
                Text(
                    stringResource(
                        if (snapshot.working) R.string.analytics_a11y_working
                        else R.string.analytics_empty,
                    ),
                    style = MavType.body,
                )
            }
        }
        items(snapshot.signals, key = { it.name }) { signal ->
            SignalRow(signal = signal, onRetry = viewModel::retryAnalytics)
        }
    }
}

@Composable
private fun SignalRow(signal: MavSignal, onRetry: () -> Unit) {
    val context = LocalContext.current
    val summary = describe(signal.state)
    val title = signal.name.replace('_', ' ').replaceFirstChar { it.uppercase() }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            // One announcement per row. Without this a screen reader reads the title, the state
            // and the coverage as three unrelated fragments and the wearer has to assemble them.
            .clearAndSetSemantics {
                contentDescription = context.getString(R.string.analytics_a11y_signal, title, summary)
            },
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(title, style = MavType.body)
            if (signal.state is MavSignalState.Working) {
                CircularProgressIndicator(modifier = Modifier.padding(4.dp))
            }
        }
        Text(summary, style = MavType.caption)
        Text(
            stringResource(R.string.analytics_coverage, signal.runnable, signal.total),
            style = MavType.caption,
        )
        if (signal.state is MavSignalState.Failed && signal.state.retryable) {
            TextButton(onClick = onRetry) { Text(stringResource(R.string.analytics_retry)) }
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
        if (state.displayable) stringResource(R.string.analytics_state_ready)
        // The model ran and its vocabulary is not admitted. Saying "up to date" here would
        // imply a reading exists to be up to date.
        else stringResource(R.string.analytics_state_computed_not_shown)
    is MavSignalState.Stale -> stringResource(R.string.analytics_state_stale)
    MavSignalState.Deferred -> stringResource(R.string.analytics_state_deferred)
    is MavSignalState.Failed -> stringResource(R.string.analytics_state_failed)
    is MavSignalState.PermissionRequired ->
        stringResource(R.string.analytics_permission_required, state.permission)
    is MavSignalState.Unavailable -> state.reasons.firstOrNull()?.let { describe(it) }
        ?: stringResource(R.string.analytics_state_idle)
}

@Composable
private fun describe(reason: MavUnavailable): String = when (reason) {
    is MavUnavailable.MissingStreams ->
        stringResource(R.string.analytics_needs_sensor, reason.streams.joinToString(", "))
    is MavUnavailable.MissingProfile ->
        stringResource(R.string.analytics_needs_profile, reason.fields.joinToString(", "))
    is MavUnavailable.UpstreamUnavailable ->
        stringResource(R.string.analytics_needs_upstream, reason.model)
    is MavUnavailable.PreprocessingNotPorted ->
        stringResource(R.string.analytics_not_ported, reason.detail)
}
