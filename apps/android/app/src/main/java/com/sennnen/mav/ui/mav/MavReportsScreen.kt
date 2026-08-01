package com.sennnen.mav.ui.mav

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import uniffi.mav_ffi.DailySnapshotReport
import com.sennnen.mav.connector.ConnectorConnectionState
import com.sennnen.mav.data.DailyMetric

/**
 * One row per day the core has scored.
 *
 * There is no computed "weekly summary" here — that would be the app inventing an aggregate — so
 * the report is the days themselves. A missing day is a gap in the recording, not a row omitted.
 */
@Composable
fun MavReportsScreen(
    days: List<DailyMetric>,
    onBack: () -> Unit,
) {
    val palette = MavTheme.palette
    MavDetailScaffold("Report", onBack) {
        MavTile {
            Text(
                "One row per day the core has scored. A missing day is a gap in the recording, " +
                    "not a row we left out.",
                style = MavType.body,
                color = palette.inkSecondary,
            )
        }

        MavSectionHeader("Scored days")
        if (days.isEmpty()) {
            MavUnavailableCard("Scored days", "No day has been scored yet.")
        } else {
            MavTile(padded = false) {
                days.reversed().forEachIndexed { index, metric ->
                    if (index > 0) MavDivider()
                    MavRow(
                        title = metric.day,
                        detail = metric.avgHrv?.let {
                            "${MavMetricMapper.variabilityTitle(metric.hrvLabel)} " +
                                "${MavMetricMapper.decimal(it, 1)} ms"
                        } ?: "No variability scored",
                    )
                }
            }
        }

    }
}

/** What the core and active connector recorded about themselves. */
@Composable
fun MavDiagnosticsScreen(
    days: List<DailyMetric>,
    connection: ConnectorConnectionState,
    snapshot: DailySnapshotReport?,
    onBack: () -> Unit,
) {
    val palette = MavTheme.palette
    MavDetailScaffold("Diagnostics", onBack) {
        MavSectionHeader("Store")
        MavTile(padded = false) {
            MavRow("Integrity") {
                Text("OK", style = MavType.label, color = palette.inkSecondary)
            }
            MavDivider()
            MavRow("Scored days") {
                Text("${days.size}", style = MavType.label, color = palette.inkSecondary)
            }
        }

        MavSectionHeader("Connector")
        MavTile(padded = false) {
            MavRow("State") {
                Text(connection.label, style = MavType.label, color = palette.inkSecondary)
            }
            MavDivider()
            MavRow("Connector") {
                Text(
                    connection.connectorId ?: "None",
                    style = MavType.label,
                    color = palette.inkSecondary,
                )
            }
            connection.errorMessage?.let {
                MavDivider()
                MavRow("Last error", it)
            }
        }

        if (snapshot != null) {
            MavSectionHeader("Today's snapshot")
            MavTile {
                Text(snapshot.snapshotHash, style = MavType.sub, color = palette.ink)
                Text(
                    "The digest both platforms must read identically from the same day. That " +
                        "equality is the parity contract.",
                    style = MavType.sub,
                    color = palette.inkSecondary,
                    modifier = Modifier.padding(top = 7.dp),
                )
            }
        }
    }
}

/** A day-scoped local journal; answers never alter a score without a core analytic requesting it. */
@Composable
fun MavJournalScreen(
    day: String,
    answers: Set<String>,
    onToggle: (String) -> Unit,
    onBack: () -> Unit,
) {
    val palette = MavTheme.palette
    MavDetailScaffold("Journal", onBack) {
        Text(
            "What you log stays on this phone. It is never fed into a score without an admitted " +
                "analytic asking for it.",
            style = MavType.body,
            color = palette.inkSecondary,
            modifier = Modifier.padding(vertical = 12.dp),
        )
        MavSectionHeader(day)
        MavTile(padded = false) {
            MavJournalLog.questions.forEachIndexed { index, question ->
                if (index > 0) MavDivider()
                MavToggleRow(
                    question,
                    checked = answers.contains(question),
                    onCheckedChange = { onToggle(question) },
                )
            }
        }
    }
}
