package com.sennnen.mav

import java.io.File
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectorParityTest {
    @Test
    fun frozenConnectorParityReportsMeetMobileBudgets() {
        val expected = listOf(
            Expected(
                "whoop4",
                "dev.maverick.whoop4",
                14,
                "ea7e360add1365a2ca8e1f06bb5631cda25fda93c601bd90b6b6f000a22e4df0",
            ),
            Expected(
                "whoop5",
                "dev.maverick.whoop5",
                12,
                "7829241ae70b256eb84ab70a9b8a5eac44512009fcf15aba5967cb35df94221d",
            ),
        )
        expected.forEach { value ->
            val report = connectorFixture(value.family)
            assertEquals("mavconn-parity/v1", report.getString("schema"))
            assertEquals(value.connectorId, report.getString("connector_id"))
            assertEquals(value.artifactHash, report.getString("artifact_sha256"))
            assertEquals(value.fixtureCount, report.getInt("fixture_count"))

            val fixtures = report.getJSONArray("fixtures")
            val names = (0 until fixtures.length())
                .map { fixtures.getJSONObject(it) }
                .onEach { fixture ->
                    assertTrue(fixture.getLong("max_fuel_consumed") <= 5_000_000L)
                    assertTrue(fixture.getLong("peak_memory_bytes") <= 4L * 1024L * 1024L)
                }
                .map { it.getString("name") }
                .toSet()
            assertTrue(
                names.containsAll(
                    setOf("history-cursor-retry", "state-restart", "malformed-frame"),
                ),
            )
        }
    }

    private fun connectorFixture(family: String): JSONObject {
        var dir: File? = File(System.getProperty("user.dir"))
        while (dir != null) {
            val candidate = File(
                dir,
                "fixtures/connectors/${family}_parity_v1.expected.json",
            )
            if (candidate.exists()) return JSONObject(candidate.readText())
            dir = dir.parentFile
        }
        error("connector parity fixture not found above ${System.getProperty("user.dir")}")
    }

    private data class Expected(
        val family: String,
        val connectorId: String,
        val fixtureCount: Int,
        val artifactHash: String,
    )
}
