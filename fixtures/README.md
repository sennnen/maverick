# Fixtures

Golden fixtures are the arbiter in this repository. When a test and an implementation disagree,
the fixture wins, because a fixture is either a real capture from hardware or a value worked out
against a published reference, and the code is neither.

The rules are short. A fixture file records the algorithm versions it was produced with, in its
name and in its content. When an algorithm version changes, a new fixture file is created next to
the old one; the old file is never edited, because it is still the correct answer for the old
version. No fixture is ever edited by hand to make a failing test pass. If a fixture looks wrong,
the fix is to regenerate it from its source capture with `mav-replay` and to write down why in the
commit body.

The workflow for creating and updating fixtures lives in
[skills/golden-fixtures/SKILL.md](../skills/golden-fixtures/SKILL.md), and the wider test policy in
[docs/testing.md](../docs/testing.md).

This directory is empty at the start of the project apart from this file. The first fixtures land
with the Milestone 1 packets, generated from captured WHOOP traffic.

`connectors/` holds signed development artifacts and generated native-versus-Wasm parity reports;
its README records their hashes, provenance, and regeneration contract.
