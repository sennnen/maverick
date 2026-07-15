---
name: golden-fixtures
description: >
  How to create and use golden fixtures: input bytes paired with expected JSON at a pipeline
  boundary, versioned by the algorithm versions that produced them. Load this before you add
  or change anything under fixtures/, when a decode or metric test needs an oracle, or when
  an algorithm version changed and a test has gone red against an old fixture.
---

# Golden fixtures

A golden fixture is a real input (capture bytes, or the output of the previous stage) paired
with the exact output a boundary should produce from it, saved as JSON. Fixtures exist at
every boundary of the pipeline: frame, sample, timeline, feature, and metric. They are how a
decoder or a metric is pinned to a known answer, and how iOS and Android are proven to agree,
since both run the same fixture through the same core and compare hashes.

## Naming and versioning

A fixture records the algorithm versions it was produced with. Put a version in the file name
(`rr_dedup_v1.json`, `v24_decode_v2.json`) and stamp the specific algorithm versions inside
the file alongside the expected output. The rule that makes fixtures trustworthy: **changing
an algorithm version means a new fixture file, never an edit to the old one.** The old fixture
stays as the record of what the old version did. `fixtures/README` holds the exact layout;
read it before you add a file.

## Generating one from a capture

1. Get a capture: hex lines, or a btsnoop subset, saved where the fixture will reference it.
2. Run it through the pipeline with `mav-replay`, which dumps every boundary to JSON. This is
   the same tool that stands in for hardware, so nothing about generating a fixture needs a
   strap.
3. Take the boundary you want as the expected output. Read it by hand and sanity-check it
   before trusting it: an HR in range, a gravity magnitude near 1 g, a timestamp in a
   plausible window. A fixture that captures a bug is worse than none.
4. Save it under `fixtures/<boundary>/` with the version in the name and the algorithm
   versions stamped inside, and reference it from the test.

## The hard rule

Fixtures are never hand-edited to make a test pass. When a test goes red against a fixture,
there are two honest outcomes: the code regressed, so fix the code; or the algorithm changed
on purpose, so bump its version and write a **new** fixture, leaving the old one in place.
Editing the expected values until the test is green destroys the only evidence the test had.

One more thing worth saying: a fixture proves the decoder reproduces a known output. It does
not prove the output is physiologically correct. That distinction lives in `docs/testing.md`,
and it is why a passing fixture is necessary but not sufficient for calling a metric validated.
