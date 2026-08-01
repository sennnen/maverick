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
                "17f1ee6eee7eea6cd2a03fbcb8c9eada80ae0f4a5a39cab708a949ff5251041a",
                setOf("chest-strap-reports-electrical-intervals"),
            ),
            Expected(
                "whoop4",
                "dev.maverick.whoop4",
                16,
                "e5f625b8cd4645cb0b09e69ae9ef5ce496293bab5e944d102284ab4af2a45989",
            ),
            Expected(
                "whoop5",
                "dev.maverick.whoop5",
                16,
                "3062689f5278ae2c2d0c6a744a854badae7f91d172da518670394aa8fee83632",
                setOf(
                    "history-cursor-retry",
                    "state-restart",
                    "malformed-frame",
                    "mg-ecg-capture",
                    "non-mg-ecg-fails-closed",
                ),
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
