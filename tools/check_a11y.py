#!/usr/bin/env python3
"""Mechanical accessibility gate for the Terrain design language.

Stdlib only, and it reads `tokens/aura.json` rather than either generated file, so a scheme
that only exists on one platform cannot hide here.

What it checks today is contrast, computed rather than asserted: every ink weight against every
surface it is allowed to sit on, in both schemes, including the surfaces a status tint or the
photography scrim composites into. The UX lane's accessibility contract states the ratios; this
file is where failing them stops a commit.

    tools/check_a11y.py            report every pair and exit non-zero on a failure
    tools/check_a11y.py --quiet    print only failures
    tools/check_a11y.py PATH       check a different token file, so the gate itself is testable

Ratios come from WCAG 2.2 (relative luminance, (L1+0.05)/(L2+0.05)):

    primary ink on any surface          >= 7.0   (AAA body text)
    secondary ink                       >= 4.5   (AA body text)
    focus ring against its surface      >= 3.0   (AA non-text)

There are exactly two ink weights, and that is a finding rather than a preference: a third,
lighter weight cannot clear 4.5:1 on these surfaces, and every string in this app carries
information, so there is nothing a sub-4.5:1 weight could legitimately be used for. Hierarchy
comes from size, case, and tracking. Adding an `inkTertiary` token puts this file red.

The lane's remaining accessibility rules - labels, roles, traversal, Dynamic Type, chart
summaries - are enforced by the platform suites, because they need a rendered view and a grep
cannot see them.
"""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TOKENS = ROOT / "tokens" / "aura.json"

SCHEMES = ("dark", "light")

# The surfaces text is allowed to sit on. `bg` is the canvas, the rest are cards.
SURFACES = ("bg", "card", "cardEdge", "sunken")

# A metric tint washes a card's surface, so the ink sitting on it is really sitting on the
# composite. Every wash is checked as its own surface. These name metric identity, not a verdict:
# status is carried by the baseline bar and the word beside it, never by the surface colour.
TINTED = (
    "tintCharge",
    "tintRest",
    "tintEffort",
    "tintHeart",
    "tintEnergy",
    "tintVitals",
    "tintCycle",
    "tintNeutral",
)

INK_MINIMUM = {"ink": 7.0, "inkSecondary": 4.5}
FOCUS_MINIMUM = 3.0

# A family glow is a data mark - a chart line, a bar, a glyph - so it is held to the WCAG
# non-text ratio against the card it is drawn on rather than to a body-text ratio.
GLOW_MINIMUM = 3.0

# `photoVeil` composites over a photograph nobody has seen yet, so the only honest check is the
# worst case the veil can produce. Ink is light on dark and dark on light, so the hardest
# photograph is the one that pushes the veiled surface toward the ink: white on dark, black on
# light. Anything that passes here passes for every possible landscape.
WORST_CASE_PHOTO = {"dark": (255, 255, 255), "light": (0, 0, 0)}

# `photoScrim` is the other half of that story: a hero card carries WHITE copy directly on the
# photograph in both schemes, so the hardest photograph for it is a white one regardless of scheme.
# This is what stops the scrim being lightened for looks until the headline stops being readable.
WHITE_COPY = (255, 255, 255)
SCRIM_PHOTO = (255, 255, 255)

# Any ink weight the token file grows later is held to the body-text ratio too, so a faint
# weight cannot be introduced without this gate seeing it.
INK_TOKEN_PREFIX = "ink"
UNLISTED_INK_MINIMUM = 4.5


def strip_comments(value):
    if isinstance(value, dict):
        return {k: strip_comments(v) for k, v in value.items() if k != "$comment"}
    return value


def rgb(hex_string: str) -> tuple:
    value = int(hex_string, 16)
    return ((value >> 16) & 0xFF, (value >> 8) & 0xFF, value & 0xFF)


def composite(fg: tuple, alpha: float, bg: tuple) -> tuple:
    """Source-over, which is what both platforms do when they draw a translucent fill."""
    return tuple(fg[i] * alpha + bg[i] * (1.0 - alpha) for i in range(3))


def luminance(colour: tuple) -> float:
    channels = []
    for raw in colour:
        c = raw / 255.0
        channels.append(c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4)
    return 0.2126 * channels[0] + 0.7152 * channels[1] + 0.0722 * channels[2]


def ratio(a: tuple, b: tuple) -> float:
    la, lb = luminance(a), luminance(b)
    lighter, darker = max(la, lb), min(la, lb)
    return (lighter + 0.05) / (darker + 0.05)


def ink_minimums(alphas: dict) -> dict:
    """Every `ink*` alpha token is checked, whether or not it was listed above."""
    minimums = dict(INK_MINIMUM)
    for name in alphas:
        if name.startswith(INK_TOKEN_PREFIX) and name not in minimums:
            minimums[name] = UNLISTED_INK_MINIMUM
    return minimums


def check(tokens: dict, quiet: bool) -> list:
    failures = []
    colors = tokens["colors"]
    alphas = tokens["alphaColors"]
    minimums = ink_minimums(alphas)

    for scheme in SCHEMES:
        ink_full = rgb(colors["ink"][scheme])

        for surface_name in SURFACES:
            base = rgb(colors[surface_name][scheme])

            # Every surface, plain and then washed by each status tint.
            variants = [(surface_name, base)]
            if surface_name != "bg":
                for tint_name in TINTED:
                    tint_hex, tint_alpha = alphas[tint_name][scheme]
                    variants.append(
                        (
                            f"{surface_name}+{tint_name}",
                            composite(rgb(tint_hex), tint_alpha, base),
                        )
                    )

            # The atmosphere behind a tab root composites onto the canvas, so the ink sitting on
            # a tab root is really sitting on canvas-plus-bloom. Checked at full bloom strength,
            # which is the worst case any pixel of the gradient can reach.
            if surface_name == "bg":
                for bloom_name in ("bloomTop", "bloomBottom"):
                    bloom_hex, bloom_alpha = alphas[bloom_name][scheme]
                    variants.append(
                        (
                            f"bg+{bloom_name}",
                            composite(rgb(bloom_hex), bloom_alpha, base),
                        )
                    )

            # The veiled photograph is a surface like any other, and its worst case is fixed
            # rather than guessed, so it is checked once per scheme against the canvas only.
            if surface_name == "bg":
                veil_hex, veil_alpha = alphas["photoVeil"][scheme]
                variants.append(
                    (
                        "photoVeil(worst)",
                        composite(
                            rgb(veil_hex), veil_alpha, WORST_CASE_PHOTO[scheme]
                        ),
                    )
                )

            # The hero scrim, checked once per scheme against a white photograph.
            if surface_name == "bg":
                scrim_hex, scrim_alpha = alphas["photoScrim"][scheme]
                scrimmed = composite(rgb(scrim_hex), scrim_alpha, SCRIM_PHOTO)
                value = ratio(WHITE_COPY, scrimmed)
                ok = value >= INK_MINIMUM["inkSecondary"]
                if not ok:
                    failures.append(
                        f"{scheme}: white copy on photoScrim(worst) is "
                        f"{value:.2f}:1, needs {INK_MINIMUM['inkSecondary']}:1"
                    )
                if not quiet:
                    mark = "ok  " if ok else "FAIL"
                    print(f"  {mark} {scheme:5} {'white copy':13} on {'photoScrim(worst)':22} {value:5.2f}:1")

            for variant_name, surface in variants:
                for ink_name, minimum in minimums.items():
                    if ink_name == "ink":
                        ink = ink_full
                    else:
                        ink_hex, ink_alpha = alphas[ink_name][scheme]
                        ink = composite(rgb(ink_hex), ink_alpha, surface)
                    value = ratio(ink, surface)
                    ok = value >= minimum
                    if not ok:
                        failures.append(
                            f"{scheme}: {ink_name} on {variant_name} is "
                            f"{value:.2f}:1, needs {minimum}:1"
                        )
                    if not quiet:
                        mark = "ok  " if ok else "FAIL"
                        print(f"  {mark} {scheme:5} {ink_name:13} on {variant_name:22} {value:5.2f}:1")

            focus = rgb(colors["focus"][scheme])
            focus_ratio = ratio(focus, base)
            if focus_ratio < FOCUS_MINIMUM:
                failures.append(
                    f"{scheme}: focus ring on {surface_name} is "
                    f"{focus_ratio:.2f}:1, needs {FOCUS_MINIMUM}:1"
                )
            if not quiet:
                mark = "ok  " if focus_ratio >= FOCUS_MINIMUM else "FAIL"
                print(f"  {mark} {scheme:5} {'focus':13} on {surface_name:22} {focus_ratio:5.2f}:1")

        # Family glows carry data, so each is checked against the card it is drawn on. This is
        # what stops a family hue being chosen for looks alone and disappearing in one scheme.
        card = rgb(colors["card"][scheme])
        for family in tokens["families"]["order"]:
            glow = rgb(tokens["families"][family]["glow"][scheme])
            glow_ratio = ratio(glow, card)
            ok = glow_ratio >= GLOW_MINIMUM
            if not ok:
                failures.append(
                    f"{scheme}: {family} glow on card is "
                    f"{glow_ratio:.2f}:1, needs {GLOW_MINIMUM}:1"
                )
            if not quiet:
                mark = "ok  " if ok else "FAIL"
                print(f"  {mark} {scheme:5} {family + ' glow':13} on {'card':22} {glow_ratio:5.2f}:1")

    return failures


def check_schema(tokens: dict) -> list:
    """Every colour token resolves in both schemes, so a token cannot be added dark-only."""
    failures = []
    for name, pair in tokens["colors"].items():
        for scheme in SCHEMES:
            if scheme not in pair:
                failures.append(f"colors.{name} has no {scheme} value")
    for name, pair in tokens["alphaColors"].items():
        for scheme in SCHEMES:
            if scheme not in pair:
                failures.append(f"alphaColors.{name} has no {scheme} value")
    for family in tokens["families"]["order"]:
        if family not in tokens["families"]:
            failures.append(f"families.order names {family}, which has no entry")
            continue
        for role in ("glow", "edge"):
            for scheme in SCHEMES:
                if scheme not in tokens["families"][family][role]:
                    failures.append(f"families.{family}.{role} has no {scheme} value")
    return failures


def main() -> int:
    args = sys.argv[1:]
    quiet = "--quiet" in args
    paths = [Path(a) for a in args if not a.startswith("-")]
    source = paths[0] if paths else TOKENS
    tokens = strip_comments(json.loads(source.read_text()))

    # Schema first and alone: a token missing a scheme would make every ratio below a KeyError
    # rather than a message, and a gate that crashes reads as a broken tool, not a broken palette.
    failures = check_schema(tokens)
    if not failures:
        if not quiet:
            print("check_a11y: contrast")
        failures += check(tokens, quiet)

    if failures:
        print("", file=sys.stderr)
        for failure in failures:
            print(f"check_a11y: {failure}", file=sys.stderr)
        return 1
    print("check_a11y: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
