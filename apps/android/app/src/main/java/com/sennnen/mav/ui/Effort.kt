package com.sennnen.mav.ui

/**
 * The Effort display factor for the user's scale: stored 0-100 Effort values are rescaled for
 * display only when the 0-21 WHOOP-style toggle is active. 1.0 leaves output unchanged.
 */
internal fun effortDisplayFactor(scale: EffortScale): Double =
    if (scale == EffortScale.WHOOP) UnitFormatter.EFFORT_SCALE_FACTOR else 1.0
