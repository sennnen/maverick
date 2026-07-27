package com.sennnen.mav.ui.aura

import com.sennnen.mav.ui.AuraZoneMath
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.sennnen.mav.analytics.RouteMath
import com.sennnen.mav.data.WorkoutRow
import com.sennnen.mav.ui.AppViewModel
import com.sennnen.mav.ui.ProfileStore
import com.sennnen.mav.ui.parseZonePercents
import java.text.DateFormat
import java.util.Date
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

// Workout detail flyout (Android port of AuraWorkoutDetailView /
// AuraWorkoutSummary in Strand/UI/AuraStrainView.swift): activity-strain hero,
// stats grid, on-device GPS route (drawn natively — no map SDK is vendored),
// time in zones, provenance line.

@Composable
fun AuraWorkoutDetailSheet(vm: AppViewModel, row: WorkoutRow, onDismiss: () -> Unit) {
    AuraSheet(title = row.sport, onDismiss = onDismiss, family = AuraFamily.EFFORT) {
        AuraWorkoutSummary(vm = vm, row = row)
    }
}

/** Shared scored-session summary (detail flyout + live-session end state). */
@Composable
fun AuraWorkoutSummary(vm: AppViewModel, row: WorkoutRow) {
    val p = Aura.palette
    val context = LocalContext.current
    val profile = remember { ProfileStore.from(context) }
    var zoneMinutes by remember(row.startTs) { mutableStateOf<List<Double>?>(null) }
    var route by remember(row.startTs) { mutableStateOf<List<RouteMath.LatLng>>(emptyList()) }

    val durMin = (row.durationS ?: (row.endTs - row.startTs).toDouble()) / 60.0
    val hrMax = AuraZoneMath.maxHr(profile.age, profile.hrMaxOverride)

    LaunchedEffect(row.startTs) {
        // Imported per-workout zone split wins; else derive minutes from the
        // strap's raw HR (same precedence the legacy detail used).
        val pct = parseZonePercents(row.zonesJSON)
        zoneMinutes = when {
            pct != null && durMin > 0 -> pct.map { durMin * it / 100.0 }
            else -> vm.workoutZoneMinutes(row.startTs, row.endTs)?.takeIf { it.sum() > 0 }
        }
        // On-device GPS route, when this session recorded one (#524).
        route = row.routePolyline
            ?.let { runCatching { RouteMath.decode(it) }.getOrNull() }
            ?.takeIf { it.size >= 2 }
            ?: emptyList()
    }

    // MARK: Hero
    AuraGlowTile(AuraFamily.EFFORT, padding = 22.dp, radius = 34.dp) {
        Column(Modifier.heightIn(min = 190.dp), verticalArrangement = Arrangement.spacedBy(18.dp)) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text("Activity strain", style = AuraType.label, color = p.ink.copy(alpha = 0.92f))
                Spacer(Modifier.weight(1f))
                Text(
                    auraLongDate(java.time.Instant.ofEpochSecond(row.startTs)
                        .atZone(java.time.ZoneId.systemDefault()).toLocalDate().toString()),
                    style = AuraType.caption, color = p.ink.copy(alpha = 0.6f),
                )
            }
            Text(AuraEffort.text(row.strain), style = AuraType.mega(76.sp), color = p.ink, maxLines = 1)
            AuraSlider(value = (row.strain ?: 0.0) / 100, glow = AuraFamily.EFFORT.glow)
        }
    }

    // MARK: Stats grid
    AuraDarkCard(padding = 20.dp) {
        val durText = auraHmText(durMin).takeIf { durMin > 0 } ?: "--"
        Row(horizontalArrangement = Arrangement.spacedBy(20.dp)) {
            AuraMiniStat(durText, "Duration", min(durMin / 120, 1.0), AuraFamily.EFFORT.glow(p.dark), modifier = Modifier.weight(1f))
            AuraMiniStat(row.avgHr?.toString() ?: "--", "Avg HR", (row.avgHr ?: 0) / 200.0, AuraFamily.HEART.glow(p.dark), unit = "bpm", modifier = Modifier.weight(1f))
        }
        Spacer(Modifier.padding(top = 22.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(20.dp)) {
            AuraMiniStat(row.maxHr?.toString() ?: "--", "Max HR", (row.maxHr ?: 0) / 200.0, AuraFamily.HEART.glow(p.dark), unit = "bpm", modifier = Modifier.weight(1f))
            AuraMiniStat(row.energyKcal?.roundToInt()?.toString() ?: "--", "Energy", (row.energyKcal ?: 0.0) / 800, AuraFamily.ENERGY.glow(p.dark), unit = "kcal", modifier = Modifier.weight(1f))
        }
        val d = row.distanceM
        if (d != null && d > 0) {
            Spacer(Modifier.padding(top = 22.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(20.dp)) {
                AuraMiniStat(
                    String.format(java.util.Locale.US, "%.2f", d / 1000), "Distance",
                    min(d / 15000, 1.0), AuraFamily.VITALS.glow(p.dark), unit = "km",
                    modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.weight(1f))
            }
        }
    }

    // MARK: Route (native polyline sketch — no map SDK vendored)
    if (route.isNotEmpty()) {
        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            AuraSectionHeader(title = "Route")
            AuraDarkCard {
                RouteSketch(route)
            }
        }
    }

    // MARK: Zones
    zoneMinutes?.let { minutes ->
        Column(verticalArrangement = Arrangement.spacedBy(14.dp)) {
            AuraSectionHeader(title = "Time in zones")
            AuraDarkCard {
                AuraZoneBars(minutes = minutes, hrMax = hrMax)
            }
        }
    }

    val fmt = DateFormat.getTimeInstance(DateFormat.SHORT)
    Text(
        "Source: ${row.source} · ${auraShortDate(java.time.Instant.ofEpochSecond(row.startTs).atZone(java.time.ZoneId.systemDefault()).toLocalDate().toString())} ${fmt.format(Date(row.startTs * 1000))}–${fmt.format(Date(row.endTs * 1000))}",
        style = AuraType.caption, color = p.ink.copy(alpha = 0.45f),
        modifier = Modifier.padding(horizontal = 4.dp),
    )
}

/** Equirectangular route sketch in the Starship accent, latest style: the
 *  on-device GPS trace drawn directly (no tiles, nothing leaves the phone). */
@Composable
private fun RouteSketch(route: List<RouteMath.LatLng>) {
    val p = Aura.palette
    Canvas(Modifier.fillMaxWidth().height(200.dp)) {
        val lats = route.map { it.lat }
        val lons = route.map { it.lon }
        val latMin = lats.min(); val latMax = lats.max()
        val lonMin = lons.min(); val lonMax = lons.max()
        val latSpan = max(latMax - latMin, 1e-6)
        val lonSpan = max(lonMax - lonMin, 1e-6)
        // Uniform scale (preserve shape), centred in the canvas with padding.
        val pad = 16.dp.toPx()
        val w = size.width - pad * 2
        val h = size.height - pad * 2
        val scale = min(w / lonSpan.toFloat(), h / latSpan.toFloat())
        val xOff = pad + (w - lonSpan.toFloat() * scale) / 2
        val yOff = pad + (h - latSpan.toFloat() * scale) / 2
        fun pt(ll: RouteMath.LatLng) = Offset(
            xOff + ((ll.lon - lonMin).toFloat() * scale),
            yOff + ((latMax - ll.lat).toFloat() * scale),
        )
        val path = Path().apply {
            val first = pt(route.first())
            moveTo(first.x, first.y)
            route.drop(1).forEach { val o = pt(it); lineTo(o.x, o.y) }
        }
        drawPath(
            path, p.accent,
            style = Stroke(width = 4.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round),
        )
        // Start / end markers.
        drawCircle(p.good, radius = 5.dp.toPx(), center = pt(route.first()))
        drawCircle(p.bad, radius = 5.dp.toPx(), center = pt(route.last()))
    }
}
