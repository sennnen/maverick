package com.sennnen.mav.ui.aura

import android.content.Context
import android.content.Intent
import android.graphics.Color as AColor
import android.graphics.Paint
import android.graphics.Typeface
import android.graphics.pdf.PdfDocument
import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Description
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.sennnen.mav.ui.AppViewModel
import java.io.File
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.util.Locale
import kotlin.math.max
import kotlin.math.roundToInt

// Weekly / Monthly Performance Assessment (Android port of
// Strand/UI/AuraReportsView.swift) — generated fully on-device from the
// existing day history, with a shareable PDF rendered natively via
// PdfDocument. No cloud.

private enum class ReportSpan(val label: String, val days: Int) {
    WEEKLY("Weekly", 7), MONTHLY("Monthly", 30)
}

@Composable
fun AuraReportsScreen(vm: AppViewModel, onClose: () -> Unit) {
    val p = Aura.palette
    val context = LocalContext.current
    val days by vm.recentDays.collectAsStateWithLifecycle()
    val workouts by vm.workouts.collectAsStateWithLifecycle()
    var span by remember { mutableStateOf(ReportSpan.WEEKLY) }
    var restSeries by remember { mutableStateOf<Map<String, Double>>(emptyMap()) }
    val factor = AuraEffort.displayFactor()

    LaunchedEffect(days) {
        vm.loadWorkouts()
        restSeries = runCatching {
            vm.repo.metricSeries("my-whoop", "sleep_performance", "0000-00-00", "9999-99-99")
        }.getOrDefault(emptyList()).associate { it.day to it.value }
    }

    val window = days.takeLast(span.days)
    val cutoff = System.currentTimeMillis() / 1000 - span.days * 86_400L
    val rows = workouts.filter { it.startTs >= cutoff }
    val topSport = rows.groupingBy { it.sport }.eachCount().maxByOrNull { it.value }?.key
    val restVals = window.mapNotNull { restSeries[it.day] }

    fun avg(v: List<Double>): Double? = if (v.isEmpty()) null else v.sum() / v.size

    val chargeAvg = avg(window.mapNotNull { it.recovery })
    val restAvg = avg(restVals)
    val effortAvg = avg(window.mapNotNull { it.strain })
    val hrvAvg = avg(window.mapNotNull { it.avgHrv })
    val rhrAvg = avg(window.mapNotNull { it.restingHr?.toDouble() })
    val sleepAvg = avg(window.mapNotNull { it.totalSleepMin })

    val title = if (span == ReportSpan.WEEKLY) "Weekly Performance Assessment"
    else "Monthly Performance Assessment"
    val pretty = DateTimeFormatter.ofPattern("d MMM")
    val subtitle = if (window.isEmpty()) "" else
        "${runCatching { LocalDate.parse(window.first().day).format(pretty) }.getOrDefault(window.first().day)} – " +
            runCatching { LocalDate.parse(window.last().day).format(pretty) }.getOrDefault(window.last().day)

    AuraScreen(lead = AuraFamily.CHARGE) {
        Column(Modifier.fillMaxSize().statusBarsPadding()) {
            AuraSheetBar(title = "Report", onClose = onClose)
            Column(
                Modifier
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = Aura.screenMargin)
                    .padding(bottom = 48.dp),
                verticalArrangement = Arrangement.spacedBy(Aura.sectionGap),
            ) {
                // Span picker
                Row(
                    Modifier.background(p.ink.copy(alpha = 0.08f), CircleShape).padding(3.dp),
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    ReportSpan.entries.forEach { s ->
                        val active = s == span
                        Text(
                            s.label, style = AuraType.caption,
                            color = if (active) Color.Black else p.ink.copy(alpha = 0.65f),
                            modifier = Modifier
                                .background(if (active) p.accent else Color.Transparent, CircleShape)
                                .clickable { span = s }
                                .padding(horizontal = 16.dp, vertical = 7.dp),
                        )
                    }
                }

                // Header
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(title, style = AuraType.heading(22.sp), color = p.ink)
                    Text(subtitle, style = AuraType.sub, color = p.ink.copy(alpha = 0.6f))
                }

                // Headline pillars, status-coloured
                Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                    PillarCell(
                        "Charge", chargeAvg?.roundToInt()?.toString() ?: "--", "%",
                        AuraStatus.recovery(chargeAvg), AuraFamily.CHARGE, Modifier.weight(1f),
                    )
                    PillarCell(
                        "Rest", restAvg?.roundToInt()?.toString() ?: "--", "%",
                        AuraStatus.sleep(restAvg), AuraFamily.REST, Modifier.weight(1f),
                    )
                    PillarCell(
                        "Effort", AuraEffort.text(effortAvg, factor), "",
                        AuraStatus.NONE, AuraFamily.EFFORT, Modifier.weight(1f),
                    )
                }

                // Averages grid
                AuraDarkCard(padding = 20.dp) {
                    Row(horizontalArrangement = Arrangement.spacedBy(18.dp)) {
                        AuraMiniStat(auraIntText(hrvAvg), "Avg HRV", (hrvAvg ?: 0.0) / 140, AuraFamily.CHARGE.glow(p.dark), unit = "ms", modifier = Modifier.weight(1f))
                        AuraMiniStat(auraIntText(rhrAvg), "Avg Resting HR", rhrAvg?.let { 1 - it / 100 } ?: 0.0, AuraFamily.HEART.glow(p.dark), unit = "bpm", modifier = Modifier.weight(1f))
                    }
                    Spacer(Modifier.padding(top = 20.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(18.dp)) {
                        AuraMiniStat(auraHmText(sleepAvg), "Avg Sleep", (sleepAvg ?: 0.0) / 540, AuraFamily.REST.glow(p.dark), modifier = Modifier.weight(1f))
                        AuraMiniStat(
                            "${rows.size}",
                            topSport?.let { "Workouts · mostly $it" } ?: "Workouts",
                            rows.size.toDouble() / max(window.size, 1),
                            AuraFamily.EFFORT.glow(p.dark), modifier = Modifier.weight(1f),
                        )
                    }
                }

                // Charge, day by day
                AuraDarkCard {
                    Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
                        Text("Charge, day by day", style = AuraType.heading(15.sp), color = p.ink)
                        AuraGraph(
                            points = window.mapNotNull { d -> d.recovery?.let { AuraPoint(d.day, it) } },
                            tint = AuraFamily.CHARGE.glow, unit = "%",
                            style = AuraGraphStyle.BARS, height = 90.dp,
                        )
                    }
                }

                Text(
                    "Generated on-device by Maverick · Approximate, not medical advice",
                    style = AuraType.caption, color = p.ink.copy(alpha = 0.4f),
                )

                // Export
                Row(
                    Modifier
                        .fillMaxWidth()
                        .background(p.accent, CircleShape)
                        .auraPressable {
                            exportReportPdf(
                                context, title, subtitle,
                                chargeAvg, restAvg, AuraEffort.text(effortAvg, factor),
                                hrvAvg, rhrAvg, sleepAvg, rows.size, topSport,
                                window.mapNotNull { d -> d.recovery?.let { AuraPoint(d.day, it) } },
                            )
                        }
                        .padding(vertical = 15.dp),
                    horizontalArrangement = Arrangement.spacedBy(10.dp, Alignment.CenterHorizontally),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(Icons.Filled.Description, contentDescription = null, tint = Color.Black, modifier = Modifier.size(18.dp))
                    Text("Export as PDF", style = AuraType.label, color = Color.Black)
                }
            }
        }
    }
}

@Composable
private fun PillarCell(
    label: String,
    value: String,
    unit: String,
    status: AuraStatus,
    family: AuraFamily,
    modifier: Modifier = Modifier,
) {
    val p = Aura.palette
    val tint = if (status == AuraStatus.NONE) family.glow else status.color
    Column(
        modifier
            .background(p.card, RoundedCornerShape(20.dp))
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(label, style = AuraType.caption, color = p.ink.copy(alpha = 0.65f))
        Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(value, style = AuraType.number(30.sp), color = tint, maxLines = 1)
            if (unit.isNotEmpty() && value != "--") {
                Text(
                    unit, style = AuraType.caption, color = p.ink.copy(alpha = 0.55f),
                    modifier = Modifier.padding(bottom = 5.dp),
                )
            }
        }
    }
}

// MARK: - Native PDF render (PdfDocument, on-device; shared via the existing FileProvider)

private fun exportReportPdf(
    context: Context,
    title: String,
    subtitle: String,
    chargeAvg: Double?,
    restAvg: Double?,
    effortText: String,
    hrvAvg: Double?,
    rhrAvg: Double?,
    sleepAvgMin: Double?,
    workoutCount: Int,
    topSport: String?,
    chargePoints: List<AuraPoint>,
) {
    runCatching {
        val pageW = 616
        val pageH = 800
        val doc = PdfDocument()
        val page = doc.startPage(PdfDocument.PageInfo.Builder(pageW, pageH, 1).create())
        val c = page.canvas
        c.drawColor(AColor.BLACK)

        val ink = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = AColor.WHITE }
        val dim = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = AColor.argb(150, 255, 255, 255) }
        val margin = 36f
        var y = 64f

        ink.typeface = Typeface.create(Typeface.SANS_SERIF, Typeface.BOLD)
        ink.textSize = 22f
        c.drawText(title, margin, y, ink)
        y += 22f
        dim.textSize = 12f
        c.drawText(subtitle, margin, y, dim)
        y += 40f

        // Pillars
        val jade = AColor.rgb(0x14, 0xC0, 0x78)
        val ocean = AColor.rgb(0x3E, 0x7B, 0xFF)
        val magenta = AColor.rgb(0xF5, 0x2E, 0x9C)
        val big = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            typeface = Typeface.create("sans-serif-thin", Typeface.NORMAL)
            textSize = 34f
        }
        val cellW = (pageW - margin * 2) / 3f
        listOf(
            Triple("Charge", chargeAvg?.roundToInt()?.let { "$it%" } ?: "--", jade),
            Triple("Rest", restAvg?.roundToInt()?.let { "$it%" } ?: "--", ocean),
            Triple("Effort", effortText, magenta),
        ).forEachIndexed { i, (label, value, colr) ->
            val x = margin + cellW * i
            dim.textSize = 11f
            c.drawText(label, x, y, dim)
            big.color = colr
            c.drawText(value, x, y + 36f, big)
        }
        y += 84f

        // Averages
        dim.textSize = 12f
        val sleepText = sleepAvgMin?.takeIf { it > 0 }?.let {
            val t = it.roundToInt(); "${t / 60}h ${t % 60}m"
        } ?: "--"
        listOf(
            "Avg HRV: ${hrvAvg?.roundToInt() ?: "--"} ms",
            "Avg Resting HR: ${rhrAvg?.roundToInt() ?: "--"} bpm",
            "Avg Sleep: $sleepText",
            "Workouts: $workoutCount${topSport?.let { " · mostly $it" } ?: ""}",
        ).forEach { line ->
            c.drawText(line, margin, y, dim)
            y += 20f
        }
        y += 24f

        // Charge day-by-day bars
        dim.textSize = 12f
        c.drawText("Charge, day by day", margin, y, dim)
        y += 12f
        val chartH = 120f
        val chartW = pageW - margin * 2
        val barPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = jade }
        if (chargePoints.isNotEmpty()) {
            val bw = chartW / chargePoints.size
            chargePoints.forEachIndexed { i, pt ->
                val h = (pt.value.coerceIn(0.0, 100.0) / 100.0 * chartH).toFloat()
                c.drawRoundRect(
                    margin + i * bw + bw * 0.2f, y + chartH - h,
                    margin + i * bw + bw * 0.8f, y + chartH,
                    3f, 3f, barPaint,
                )
            }
        } else {
            c.drawText("Not enough data in this range yet", margin, y + 40f, dim)
        }
        y += chartH + 40f

        val generatedOn = LocalDate.now().format(DateTimeFormatter.ofPattern("MMM d, yyyy", Locale.US))
        dim.textSize = 10f
        c.drawText(
            "Generated on-device by Maverick · $generatedOn · Approximate, not medical advice",
            margin, y, dim,
        )

        doc.finishPage(page)
        val dir = File(context.cacheDir, "reports").apply { mkdirs() }
        val file = File(dir, "Maverick-${title.replace(" ", "-")}.pdf")
        file.outputStream().use { doc.writeTo(it) }
        doc.close()

        val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "application/pdf"
            putExtra(Intent.EXTRA_STREAM, uri)
            putExtra(Intent.EXTRA_SUBJECT, title)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        context.startActivity(Intent.createChooser(send, "Share report"))
    }.onFailure {
        Toast.makeText(context, "Couldn't build the report: ${it.message}", Toast.LENGTH_LONG).show()
    }
}
