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
                "generic_hr",
                "dev.maverick.generic-hr",
                3,
                "33a7c5141295ef9d5c030cb0706963291c2ce432f9c82a1375367056730c26f2",
                setOf("chest-strap-reports-electrical-intervals"),
            ),
            Expected(
                "whoop4",
                "dev.maverick.whoop4",
                16,
                "a51540d872b3262aa47ef64197f1c36d5cec5838d48cd47239294da6ec0d0f28",
            ),
            Expected(
                "whoop5",
                "dev.maverick.whoop5",
                14,
                "ac613682a7835833602c646ace549515c3ce80dd458fbb8feccedb56923a1944",
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
            assertTrue(names.containsAll(value.requiredFixtures))
        }
    }

    private fun connectorFixture(family: String): JSONObject {
        var dir: File? = File(requireNotNull(System.getProperty("user.dir")))
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
        /// The cases whose absence would mean a whole behaviour went untested. A device connector
        /// has to prove its history, restart and malformed-frame paths; a connector that speaks a
        /// standard profile has none of those and proves the claim it does make instead.
        val requiredFixtures: Set<String> = setOf(
            "history-cursor-retry",
            "state-restart",
            "malformed-frame",
        ),
    )
}
