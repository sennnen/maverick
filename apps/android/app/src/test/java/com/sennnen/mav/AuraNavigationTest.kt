package com.sennnen.mav

import com.sennnen.mav.aura.AuraTab
import com.sennnen.mav.aura.swipeDestination
import org.junit.Assert.assertEquals
import org.junit.Test

class AuraNavigationTest {
    @Test
    fun decisiveHorizontalFlickMovesOneHub() {
        assertEquals(
            AuraTab.RECOVERY,
            swipeDestination(AuraTab.TODAY, dx = -90f, dy = 10f, threshold = 60f),
        )
    }

    @Test
    fun verticalOrWeakDragKeepsCurrentHub() {
        assertEquals(
            AuraTab.RECOVERY,
            swipeDestination(AuraTab.RECOVERY, dx = 80f, dy = 70f, threshold = 60f),
        )
        assertEquals(
            AuraTab.RECOVERY,
            swipeDestination(AuraTab.RECOVERY, dx = 40f, dy = 0f, threshold = 60f),
        )
    }

    @Test
    fun edgeTabsDoNotWrap() {
        assertEquals(
            AuraTab.TODAY,
            swipeDestination(AuraTab.TODAY, dx = 90f, dy = 0f, threshold = 60f),
        )
        assertEquals(
            AuraTab.SLEEP,
            swipeDestination(AuraTab.SLEEP, dx = -90f, dy = 0f, threshold = 60f),
        )
    }
}
