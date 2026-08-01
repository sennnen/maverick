package com.sennnen.mav.ui.mav

import android.content.Intent
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import com.sennnen.mav.ecg.MavEcgPdfRenderer
import com.sennnen.mav.ecg.MavEcgReportContent
import java.io.File
import java.time.Duration
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import uniffi.mav_ffi.ConnectorCaptureCapability
import uniffi.mav_ffi.EcgCaptureReport
import uniffi.mav_ffi.EcgCheck
import uniffi.mav_ffi.EcgReportPayload
import uniffi.mav_ffi.EcgResultReport

/**
 * The ECG index: take a reading, see the latest one at a glance, reach the rest.
 *
 * Per ADR-034 the newest result leads with its own trace and the four checks, because a rhythm
 * word alone gives the wearer nothing to judge. Model probabilities are deliberately absent here
 * and live in the downloadable report: the screen answers "is this normal", the report carries
 * the numbers behind that answer.
 */
@Composable
fun MavEcgScreen(
    capabilities: List<ConnectorCaptureCapability>,
    capture: EcgCaptureReport?,
    results: List<EcgResultReport>,
    error: String?,
    loadPayload: suspend (ULong) -> EcgReportPayload?,
    onStart: () -> Unit,
    onStop: () -> Unit,
    onOpenResult: (ULong) -> Unit,
    onBack: () -> Unit,
) {
    MavDetailScaffold(title = "ECG", onBack = onBack) {
        val capturing = capture != null && capture.phase !in setOf("result", "failed", "cancelled")
        when {
            capturing -> CaptureCard(requireNotNull(capture), onStop)
            capabilities.any { it.stream == "ecg" } -> MavPrimaryButton(
                title = "Take a new ECG reading",
                detail = "30 seconds, finger on the electrode",
                onClick = onStart,
            )
            else -> MavUnavailableCard(
                "ECG capture",
                "Connect an ECG-capable device. Capture appears only after the hardware " +
                    "positively declares that capability.",
            )
        }

        if (capture?.phase == "failed" || capture?.phase == "cancelled") {
            MavTile {
                Text(
                    captureFailureText(capture.qualityReason),
                    style = MavType.body,
                    color = MavTheme.palette.inkSecondary,
                )
            }
        }

        if (error != null) {
            MavTile {
                Text(error, style = MavType.body, color = MaterialTheme.colorScheme.error)
            }
        }

        val latest = results.firstOrNull()
        if (latest != null) {
            var waveform by remember(latest.captureId) { mutableStateOf<List<Float>?>(null) }
            LaunchedEffect(latest.captureId) {
                waveform = runCatching { loadPayload(latest.captureId)?.waveform }.getOrNull()
            }
            MavTile(modifier = Modifier.clickable { onOpenResult(latest.captureId) }) {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(rhythmTitle(latest.rhythm), style = MavType.title)
                            Text(
                                relativeTime(latest.startedNs),
                                style = MavType.sub,
                                color = MavTheme.palette.inkSecondary,
                            )
                        }
                        Icon(MavIcons.chevronRight, contentDescription = null)
                    }
                    waveform?.let {
                        MavEcgTrace(it, latest.sourceRateHz.toInt(), height = 96.dp)
                    }
                    EcgChecklist(latest.checks, compact = true)
                }
            }
        }

        if (results.size > 1) {
            MavSectionHeader("Earlier readings")
            MavTile(padded = false) {
                Column {
                    results.drop(1).forEachIndexed { index, result ->
                        if (index > 0) MavDivider()
                        MavRow(
                            title = rhythmTitle(result.rhythm),
                            detail = resultDate(result.startedNs),
                            modifier = Modifier.clickable { onOpenResult(result.captureId) },
                            trailing = { Icon(MavIcons.chevronRight, contentDescription = null) },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun CaptureCard(capture: EcgCaptureReport, onStop: () -> Unit) {
    MavTile {
        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(ecgCaptureTitle(capture.phase), style = MavType.title)
                    Text(
                        captureDetail(capture),
                        style = MavType.body,
                        color = MavTheme.palette.inkSecondary,
                    )
                }
                CircularProgressIndicator(
                    progress = { capture.progressMilli.toFloat() / 1_000f },
                    color = MavTheme.palette.accent,
                )
            }
            if (capture.phase != "analysing") {
                LinearProgressIndicator(
                    progress = { capture.progressMilli.toFloat() / 1_000f },
                    modifier = Modifier
                        .fillMaxWidth()
                        .semantics {
                            contentDescription =
                                "ECG capture ${capture.progressMilli.toInt() / 10} percent"
                        },
                    color = MavTheme.palette.accent,
                )
                MavQuietButton("Cancel", onStop)
            }
        }
    }
}

/** The reading itself: trace first, then the verdict, the rate, and the report. */
@Composable
fun MavEcgResultScreen(
    result: EcgResultReport,
    loadPayload: suspend (ULong) -> EcgReportPayload?,
    onRemove: (ULong) -> Unit,
    onBack: () -> Unit,
) {
    val context = LocalContext.current
    var payload by remember(result.captureId) { mutableStateOf<EcgReportPayload?>(null) }
    var pdf by remember(result.captureId) { mutableStateOf<File?>(null) }
    var error by remember(result.captureId) { mutableStateOf<String?>(null) }

    LaunchedEffect(result.captureId) {
        runCatching {
            val loaded = requireNotNull(loadPayload(result.captureId))
            payload = loaded
            val data = MavEcgPdfRenderer.render(MavEcgReportContent(loaded))
            File(context.cacheDir, "ecg-reports").apply { mkdirs() }
                .resolve("Maverick-ECG-${result.captureId}.pdf")
                .also { it.writeBytes(data) }
        }.onSuccess { pdf = it }.onFailure { error = it.message }
    }

    MavDetailScaffold(title = "ECG details", onBack = onBack) {
        Text(
            recordedRange(result.startedNs, result.endedNs),
            style = MavType.sub,
            color = MavTheme.palette.inkSecondary,
        )
        payload?.let {
            MavEcgTrace(it.waveform, result.sourceRateHz.toInt(), height = 190.dp)
            Text(
                "Scroll to read the whole 30 seconds. One large square is 0.2 seconds.",
                style = MavType.sub,
                color = MavTheme.palette.inkSecondary,
            )
        }

        Text(rhythmTitle(result.rhythm), style = MavType.numeralMedium)
        EcgChecklist(result.checks, compact = false)

        MavTile {
            Text(rhythmExplanation(result.rhythm), style = MavType.body)
        }

        result.meanHeartRateBpm?.let { bpm ->
            MavTile {
                Column {
                    Text("$bpm", style = MavType.numeralMedium)
                    Text(
                        "AVG. HEART RATE",
                        style = MavType.label,
                        color = MavTheme.palette.inkSecondary,
                    )
                }
            }
        }

        if (pdf == null && error == null) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.Center) {
                CircularProgressIndicator(color = MavTheme.palette.accent)
            }
        } else if (pdf != null) {
            MavPrimaryButton(
                title = "Share your ECG report",
                detail = "A one-page PDF, including the model's own figures",
            ) {
                val uri = FileProvider.getUriForFile(
                    context,
                    "${context.packageName}.files",
                    requireNotNull(pdf),
                )
                context.startActivity(
                    Intent.createChooser(
                        Intent(Intent.ACTION_SEND).apply {
                            type = "application/pdf"
                            putExtra(Intent.EXTRA_STREAM, uri)
                            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                        },
                        "Share ECG report",
                    ),
                )
            }
        }

        MavTile {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(
                    "Taking regular readings makes a change easier to notice. A doctor can use " +
                        "an ECG to look at your rhythm properly.",
                    style = MavType.body,
                    color = MavTheme.palette.inkSecondary,
                )
                Text(
                    "This on-device result is provisional and is not a diagnosis. If you think " +
                        "you are having a medical emergency, call emergency services.",
                    style = MavType.body,
                    color = MavTheme.palette.inkSecondary,
                )
            }
        }

        MavQuietButton("Remove this ECG result") { onRemove(result.captureId) }

        if (error != null) {
            Text(
                requireNotNull(error),
                style = MavType.sub,
                color = MaterialTheme.colorScheme.error,
            )
        }
    }
}

/**
 * The four checks, exactly as the core derived them. A check the reading cannot support says so
 * rather than showing a tick it has not earned.
 */
@Composable
private fun EcgChecklist(checks: List<EcgCheck>, compact: Boolean) {
    if (checks.isEmpty()) return
    val palette = MavTheme.palette
    // The high- and low-rate checks say the same thing when there is no rate to judge, so a
    // reading without one showed "Heart rate not measured" twice. Collapse repeats rather than
    // dropping a check: the pair is still two findings, it just has one thing to say.
    val visible = checks.filterIndexed { index, check ->
        index == 0 || checkLabel(check) != checkLabel(checks[index - 1])
    }
    Column(verticalArrangement = Arrangement.spacedBy(if (compact) 6.dp else 10.dp)) {
        visible.forEach { check ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    when {
                        !check.known -> MavIcons.unknown
                        check.passed -> MavIcons.check
                        else -> MavIcons.alert
                    },
                    contentDescription = null,
                    tint = when {
                        !check.known -> palette.inkSecondary
                        check.passed -> palette.accent
                        else -> MaterialTheme.colorScheme.error
                    },
                    modifier = Modifier.size(if (compact) 16.dp else 20.dp),
                )
                Text(
                    checkLabel(check),
                    style = if (compact) MavType.sub else MavType.body,
                    color = palette.ink,
                    modifier = Modifier.padding(start = 10.dp),
                )
            }
        }
    }
}

private fun checkLabel(check: EcgCheck): String = when (check.id) {
    "afib" -> when {
        !check.known -> "Atrial fibrillation not assessed"
        check.passed -> "AFib not detected"
        else -> "AFib detected"
    }
    "high_heart_rate" -> when {
        !check.known -> "Heart rate not measured"
        check.passed -> "High heart rate not detected"
        else -> "High heart rate detected"
    }
    "low_heart_rate" -> when {
        !check.known -> "Heart rate not measured"
        check.passed -> "Low heart rate not detected"
        else -> "Low heart rate detected"
    }
    "sinus_rhythm" -> if (check.passed) {
        "Normal sinus rhythm detected"
    } else {
        "Normal sinus rhythm not detected"
    }
    else -> check.id
}

private fun rhythmExplanation(rhythm: String): String = when (rhythm) {
    "sinus_rhythm" ->
        "A normal sinus rhythm means the heart is beating in a uniform pattern, with the upper " +
            "and lower chambers in sync. It does not guarantee that you are well. If you feel " +
            "unwell or have symptoms, speak to a doctor."
    "atrial_fibrillation" ->
        "This reading looks like atrial fibrillation, where the upper chambers beat irregularly. " +
            "It is a provisional software result on a single-lead recording, not a diagnosis. " +
            "Take it to a doctor, and seek urgent care if you feel unwell."
    else ->
        "This reading did not match a normal sinus rhythm or atrial fibrillation. That covers a " +
            "wide range, including ordinary variation and recordings the model cannot place. " +
            "Share the report with a doctor if you are concerned."
}

internal fun ecgCaptureTitle(phase: String): String = when (phase) {
    "calibrating" -> "Checking signal"
    "recording" -> "Recording ECG"
    "analysing" -> "Analysing on device"
    else -> "ECG"
}

private fun captureFailureText(reason: String?): String = when (reason) {
    "no_signal" ->
        "No ECG signal arrived. Check the strap is worn and your finger is on the electrode, " +
            "then try again."
    "calibration_timeout" ->
        "The signal never settled enough to record. Press your finger firmly on the electrode, " +
            "rest your arms, and try again."
    "contact" -> "Contact was lost. Keep a finger on the electrode for the whole reading."
    "motion" -> "There was too much movement. Rest your arms and stay still, then try again."
    "cancelled" -> "Reading cancelled."
    else -> "The reading did not complete. Try again."
}

private fun captureDetail(capture: EcgCaptureReport): String = when (capture.phase) {
    "calibrating" -> if (capture.qualityReason == null) {
        "Signal looks good. Keep still."
    } else {
        "Adjust contact and keep still. Recording has not started yet."
    }
    "recording" -> {
        val seconds = if (capture.targetSamples > 0u) {
            capture.recordedSamples.toInt() * 30 / capture.targetSamples.toInt()
        } else {
            0
        }
        "$seconds of 30 seconds"
    }
    "analysing" -> "The admitted model is running locally. No recording leaves the phone."
    else -> ""
}

internal fun rhythmTitle(rhythm: String): String = when (rhythm) {
    "sinus_rhythm" -> "Normal sinus rhythm"
    "atrial_fibrillation" -> "Atrial fibrillation"
    else -> "Other rhythm"
}

private fun resultDate(nanoseconds: Long): String =
    DateTimeFormatter.ofPattern("d MMM uuuu, HH:mm")
        .withZone(ZoneId.systemDefault())
        .format(instantOf(nanoseconds))

private fun recordedRange(startNs: Long, endNs: Long): String {
    val zone = ZoneId.systemDefault()
    val day = DateTimeFormatter.ofPattern("d MMM uuuu").withZone(zone)
    val clock = DateTimeFormatter.ofPattern("HH:mm:ss").withZone(zone)
    return "${day.format(instantOf(startNs))}  ${clock.format(instantOf(startNs))} – " +
        clock.format(instantOf(endNs))
}

private fun relativeTime(nanoseconds: Long): String {
    val elapsed = Duration.between(instantOf(nanoseconds), Instant.now())
    val minutes = elapsed.toMinutes()
    return when {
        minutes < 1 -> "Just now"
        minutes < 60 -> "$minutes minute${plural(minutes)} ago"
        elapsed.toHours() < 24 -> "${elapsed.toHours()} hour${plural(elapsed.toHours())} ago"
        elapsed.toDays() < 7 -> "${elapsed.toDays()} day${plural(elapsed.toDays())} ago"
        else -> resultDate(nanoseconds)
    }
}

private fun plural(value: Long) = if (value == 1L) "" else "s"

private fun instantOf(nanoseconds: Long): Instant =
    Instant.ofEpochSecond(nanoseconds / 1_000_000_000, nanoseconds % 1_000_000_000)
