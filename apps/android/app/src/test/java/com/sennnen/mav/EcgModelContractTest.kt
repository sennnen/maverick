package com.sennnen.mav

import com.sennnen.mav.ecg.MavEcgClassifier
import java.security.MessageDigest
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class EcgModelContractTest {
    @Test
    fun selectedModelMatchesAdmittedHash() {
        val bytes = requireNotNull(
            javaClass.classLoader?.getResourceAsStream(MavEcgClassifier.MODEL_ASSET),
        ).use { it.readBytes() }
        val digest = MessageDigest.getInstance("SHA-256")
            .digest(bytes)
            .joinToString("") { "%02x".format(it) }
        assertEquals(
            "0be97329077d5d5d2791b8b7850baf8acaf8f12f96fdfad7bcdb4af37156ea21",
            digest,
        )
    }

    @Test
    fun floatingPointTensorContractIsFrozen() {
        assertArrayEquals(intArrayOf(1, 7_680, 1), MavEcgClassifier.INPUT_SHAPE)
        assertArrayEquals(intArrayOf(1, 3), MavEcgClassifier.OUTPUT_SHAPE)
    }

    @Test
    fun recoveredFloatOutputNormalizationIsBounded() {
        val alreadyNormalized = MavEcgClassifier.normalizeOutput(
            floatArrayOf(0.7f, 0.2f, 0.1f),
        )
        assertArrayEquals(floatArrayOf(0.7f, 0.2f, 0.1f), alreadyNormalized, 0.000_001f)

        val output = MavEcgClassifier.normalizeOutput(floatArrayOf(2f, 0f, -1f))
        assertEquals(3, output.size)
        assertTrue(output.all(Float::isFinite))
        assertEquals(1f, output.sum(), 0.000_001f)
    }
}
