# Design-tokens lane — one source of truth for the Aura theme

The Aura design language — colors in light and dark, spacing, radii, the type scale — is
hand-written twice, in `apps/ios/Maverick/UI/AuraDesign.swift` and
`apps/android/.../ui/aura/AuraTheme.kt`, and nothing detects when the two drift. This lane
adds the minimal pipeline: one tokens file, a tiny generator emitting one Swift and one
Kotlin constants file, and a CI check that regenerating produces no diff. The extraction
audit itself — reading both theme files side by side and reconciling every divergence — is
most of the value.

Deliberately out of scope, decided on record: any wireframe-to-code or layout-generation
system. Two native shells that already share an information model need a shared palette, not
a shared renderer.

The lane exits when both apps consume generated token constants, the generator round-trips
clean in CI, and every divergence found during extraction has been resolved explicitly and
logged.

---

## Packet DT-P1: Tokens file and generator

**Owns:** new `tokens/aura.json`, new `tools/gen_design_tokens.py`, generated
`AuraTokens.generated.swift` and `AuraTokens.generated.kt` (checked in), the refactor of
`AuraDesign.swift` and `AuraTheme.kt` to consume the generated constants, the CI
regenerate-and-diff step.

**Must not touch:** view code beyond substituting constants; any layout or component
structure.

**Contract:** extract colors (light and dark), spacing, radii, and the type scale from both
hand-written theme files into `tokens/aura.json`. `tools/gen_design_tokens.py` is Python
stdlib only and deterministic: same input, byte-identical output. The generated files are
committed; CI regenerates and fails on `git diff --exit-code`. Every light/dark divergence
found between the two platforms during extraction is resolved explicitly — pick the intended
value, change the loser — and logged in this file's decision log; no divergence is silently
normalized.

**Tests first:** the generator has a golden test (fixture tokens in, pinned Swift and Kotlin
out); the CI diff check is observed red on a deliberately stale generated file before it
lands.

**Exit:** both platform test suites; the CI diff step green; `tools/check_docs.sh`.

**Status: done.** `tokens/aura.json` plus `tools/gen_design_tokens.py` (stdlib only, deterministic,
`--check` mode) generate the two committed constants files; the docs CI job regenerates and runs
`git diff --exit-code`. Both theme files consume the constants and neither holds a colour literal.
The drift check was observed red on a deliberately stale generated file and green after regenerating.

---

## Packet DT-P2: Straggler migration and documentation

**Owns:** remaining hard-coded theme values in the Aura views moved onto tokens; a short
pipeline section in `docs/platform.md`.

**Must not touch:** anything beyond theme-value substitution.

**Contract:** sweep the Aura view files on both platforms for hard-coded colors, spacing,
radii, and type values that duplicate a token; substitute the token constant. Document the
pipeline — where the tokens live, how to regenerate, what the CI check enforces — in
`docs/platform.md`. Nothing further; the lane ends here by design.

**Tests first:** platform suites stay green; a grep inventory of remaining literals is
attached to the decision log with a reason for each survivor (some literals are legitimately
local).

**Exit:** both platform suites; `tools/check_docs.sh`.

**Status: done.** The sweep found no straggler: `AuraDesign.swift` has zero remaining colour, spacing,
or radius literals, and `AuraTheme.kt`'s only surviving numbers are the Material3 typography ramp —
a Compose API's role names with no iOS counterpart — and the `0xFF000000` alpha mask in `hex()`.
Both are recorded as legitimate survivors in `docs/platform.md`.

---

## Decision log

- **The extraction audit found no divergence.** Reading both theme files side by side, every colour,
  spacing value, and radius already agreed exactly, in both schemes. Nothing had to be reconciled,
  and nothing was silently normalised. The lane's stated value was the audit; the audit's answer was
  that the drift had not happened yet, so the pipeline's value from here is that the next one fails
  CI instead of shipping.
- **The type scale is split on purpose.** Sizes are tokens; families and weights are not. Helvetica
  Neue on iOS against the platform sans on Android is a deliberate platform choice, and generating a
  font family would have forced one platform to be wrong.
