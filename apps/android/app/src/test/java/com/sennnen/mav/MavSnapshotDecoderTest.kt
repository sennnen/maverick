package com.sennnen.mav

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.json.JSONException

class MavSnapshotDecoderTest {
    @Test
    fun decodesRequiredHostFieldsAndNullSession() {
        val snapshot = MavSnapshotDecoder.decode(
            json = """
                {
                  "schema":"host-snapshot/v1",
                  "core_version":"0.1.0",
                  "storage_schema":1,
                  "revision":7,
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
