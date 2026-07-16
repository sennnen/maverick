package com.sennnen.mav.ingest

import android.content.Context
import androidx.health.connect.client.HealthConnectClient
import androidx.health.connect.client.permission.HealthPermission
import androidx.health.connect.client.records.HeartRateRecord
import androidx.health.connect.client.records.RestingHeartRateRecord
import androidx.health.connect.client.records.SleepSessionRecord
import androidx.health.connect.client.records.StepsRecord

/**
 * Health Connect surface the settings sheet binds to (availability + permission contract).
 * Reading records into the core's ingest lane is planned; until then this only records consent —
 * no rows are written, so nothing can masquerade as strap telemetry.
 */
object HealthConnectImporter {

    private val READ_RECORDS = setOf(
        HeartRateRecord::class,
        RestingHeartRateRecord::class,
        SleepSessionRecord::class,
        StepsRecord::class,
    )

    val PERMISSIONS: Set<String> =
        READ_RECORDS.map { HealthPermission.getReadPermission(it) }.toSet()

    /** One of [HealthConnectClient.SDK_AVAILABLE] / SDK_UNAVAILABLE / provider-update-required. */
    fun sdkStatus(context: Context): Int = HealthConnectClient.getSdkStatus(context)
}
