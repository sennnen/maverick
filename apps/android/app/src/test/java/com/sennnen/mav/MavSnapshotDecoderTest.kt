package com.sennnen.mav

import java.io.File
import org.json.JSONException
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

class MavSnapshotDecoderTest {
    /** Decodes the shared canonical fixture the core pins and Swift decodes too (PL-P7 parity). */
    @Test
    fun decodesThePlatformFixtureExactly() {
        val fixture = platformFixture()
        val snapshot = MavSnapshotDecoder.decode(fixture.getString("json"), fixture.getString("hash"))

        assertEquals("0.1.0", snapshot.coreVersion)
        assertEquals(1, snapshot.storageSchema)
        assertEquals(1uL, snapshot.revision)
        assertEquals(1_752_600_500_000L, snapshot.asOfUnixMs)
        assertEquals("streaming", snapshot.connectionState)
        assertEquals("MG", snapshot.deviceName)
        assertNull(snapshot.batteryPercent)
        assertNull(snapshot.charging)
        assertEquals(1_752_600_500_000L, snapshot.lastSampleUnixMs)
        assertEquals(72, snapshot.currentBpm)
        assertEquals(72_000, snapshot.meanMilliBpm)
        assertEquals(1, snapshot.inRangeSamples)
        assertEquals(0, snapshot.excludedSamples)

        val prv = snapshot.prv
        assertNotNull(prv)
        prv!!
        assertEquals("pulse_rate_variability", prv.label)
        assertEquals("ppg", prv.intervalSource)
        assertEquals(828_000L, prv.meanIntervalMicros)
        assertEquals(67_454L, prv.rmssdMicros)
        assertEquals(46_583L, prv.sdnnMicros)
        assertEquals(2, prv.nn50Count)
        assertEquals(50_000L, prv.pnn50MilliPercent)
        assertEquals(5, prv.intervalCount)
        assertEquals(1, prv.excludedIntervalCount)
        assertEquals("time_domain_interval_variability", prv.algorithm)
        assertEquals("1.0.0", prv.algorithmVersion)
        assertEquals(3L, prv.provenanceId)

        assertNull(snapshot.prvUnavailableReason)
        assertEquals("Recovery model not admitted", snapshot.recoveryUnavailableReason)
        assertEquals(fixture.getString("hash"), snapshot.hash)
    }

    @Test
    fun missingRrStreamsMakePrvUnavailableWithTheExactReason() {
        val snapshot = MavSnapshotDecoder.decode(
            json = """
                {
                  "schema":"host-snapshot/v1",
                  "core_version":"0.1.0",
                  "storage_schema":1,
                  "revision":2,
                  "as_of_unix_ms":5,
                  "connection":{"state":"streaming","display_name":"MG"},
                  "session":null,
                  "analytics":{
                    "variability_label":null,
                    "availability":[
                      {"analytic":"time_domain_hrv","available":false,
                       "reason":{"kind":"missing_streams","streams":["rr_interval"]}}
                    ]
                  }
                }
            """.trimIndent(),
            hash = "abc",
        )

        assertNull(snapshot.prv)
        assertEquals("Needs rr_interval", snapshot.prvUnavailableReason)
    }

    private fun platformFixture(): JSONObject {
        var dir: File? = File(System.getProperty("user.dir"))
        while (dir != null) {
            val candidate = File(dir, "fixtures/platform/host_snapshot_v1.expected.json")
            if (candidate.exists()) return JSONObject(candidate.readText())
            dir = dir.parentFile
        }
        error("fixtures/platform/host_snapshot_v1.expected.json not found above ${System.getProperty("user.dir")}")
    }
    @Test
    fun decodesRequiredHostFieldsAndNullSession() {
        val snapshot = MavSnapshotDecoder.decode(
            json = """
                {
                  "schema":"host-snapshot/v1",
                  "core_version":"0.1.0",
                  "storage_schema":1,
                  "revision":7,
                  "as_of_unix_ms":9,
                  "connection":{"state":"disconnected","display_name":null},
                  "session":null,
                  "analytics":{
                    "availability":[
                      {
                        "analytic":"recovery",
                        "available":false,
                        "reason":{"kind":"algorithm_not_admitted"}
                      }
                    ]
                  }
                }
            """.trimIndent(),
            hash = "abc",
        )

        assertEquals("0.1.0", snapshot.coreVersion)
        assertEquals(1, snapshot.storageSchema)
        assertEquals(7uL, snapshot.revision)
        assertEquals("disconnected", snapshot.connectionState)
        assertNull(snapshot.deviceName)
        assertNull(snapshot.currentBpm)
        assertEquals("Recovery model not admitted", snapshot.recoveryUnavailableReason)
        assertEquals("abc", snapshot.hash)
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsUnknownSchema() {
        MavSnapshotDecoder.decode(
            json = """
                {
                  "schema":"host-snapshot/v2",
                  "core_version":"0.1.0",
                  "storage_schema":1,
                  "revision":1,
                  "connection":{"state":"disconnected","display_name":null},
                  "session":null
                }
            """.trimIndent(),
            hash = "abc",
        )
    }

    @Test(expected = JSONException::class)
    fun rejectsMissingRequiredConnection() {
        MavSnapshotDecoder.decode(
            json = """
                {
                  "schema":"host-snapshot/v1",
                  "core_version":"0.1.0",
                  "storage_schema":1,
                  "revision":1,
                  "session":null
                }
            """.trimIndent(),
            hash = "abc",
        )
    }
}
