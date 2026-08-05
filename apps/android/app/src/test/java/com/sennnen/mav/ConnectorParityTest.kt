package com.sennnen.mav

import java.io.File
import java.security.MessageDigest
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
                18,
                "d3dae33eb0849f6eec489473d5ddd38ff39506e74ec40c6ca57a2b513491a145",
            ),
            Expected(
                "whoop5",
                "dev.maverick.whoop5",
                16,
                "a37e0acdaf161ad1a94fd81d65be9c0572285124a3ee17e262b1bf492b86a7b5",
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

    /**
     * Every report must name the SHA-256 of the artifact sitting beside it.
     *
     * The frozen hashes above cannot tell a legitimate connector release from a report that has
     * drifted away from its own bytes — both look like one changed constant. This computes the
     * hash instead, so the two failures are distinguishable: if this passes and the frozen list
     * does not, a new release was vendored and the list needs bumping against the signed
     * registry; if this fails, the report and the artifact disagree and neither can be trusted.
     *
     * It was written because the list went stale in exactly that way. `whoop4` moved 1.0.2 to
     * 1.0.3 and `whoop5` 1.0.5 to 1.0.7 when ECG capture landed, the artifacts and reports were
     * regenerated together and correctly, and the frozen constants were updated to two values
     * that matched neither the old release nor the new one.
     */
    @Test
    fun everyReportNamesTheHashOfTheArtifactBesideIt() {
        for (family in listOf("generic_hr", "whoop4", "whoop5")) {
            val report = connectorFixture(family)
            val artifact = connectorArtifact(family)
            val digest = MessageDigest.getInstance("SHA-256")
                .digest(artifact.readBytes())
                .joinToString("") { "%02x".format(it) }
            assertEquals(
                "${artifact.name} hashes to $digest, its report claims another artifact",
                report.getString("artifact_sha256"),
                digest,
            )
            assertEquals(
                "$family reports a fixture_count its own fixture list does not have",
                report.getJSONArray("fixtures").length(),
                report.getInt("fixture_count"),
            )
        }
    }

    private fun connectorArtifact(family: String): File = resolve("${family}_v1.mavconn")

    private fun connectorFixture(family: String): JSONObject {
        return JSONObject(resolve("${family}_parity_v1.expected.json").readText())
    }

    /** Walk up from the module directory until `fixtures/connectors/` comes into view. */
    private fun resolve(name: String): File {
        var dir: File? = File(requireNotNull(System.getProperty("user.dir")))
        while (dir != null) {
            val candidate = File(dir, "fixtures/connectors/$name")
            if (candidate.exists()) return candidate
            dir = dir.parentFile
        }
        error("$name not found above ${System.getProperty("user.dir")}")
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
