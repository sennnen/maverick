---
name: doc-gardening
description: >
  A periodic pass that finds documentation which has drifted from the code: dead
  cross-references, protocol claims with no code or fixture behind them, stale file paths, and
  any divergence between CLAUDE.md and AGENTS.md. Load this when doing a documentation sweep,
  during M8 hardening, when asked to "check the docs" or "garden the docs", or when you suspect
  a doc is lying about the code.
---

# Doc gardening

Both codebases Maverick learned from shipped documentation that lied about the code. One had a
wrong gen5 frame layout and a UUID typo in prose while the code was right; the other had an
attribution file pointing at a protocol file that never existed and a command alias someone
invented and never implemented. A doc that contradicts the code is worse than no doc, because
an agent will trust it and build on a false premise. This pass is how Maverick keeps that from
setting in. Run it on a cadence, not once.

The mindset: treat every documented claim as a debt that has to be backed by something in the
repository. If a sentence in `docs/` cannot be traced to code or a fixture, it is a candidate
for deletion or a citation, not a thing to leave standing because it sounds right.

## The checks

1. **Dead cross-references.** Every file path, crate name, and inter-doc link should resolve.
   `tools/check_docs.sh` catches the mechanical breaks; read for the ones a script cannot see,
   like a link that resolves but points at the wrong section.

2. **Protocol claims cite evidence.** Every fact in `docs/protocol/whoop.md` should name a code
   location or a fixture, and carry its confidence tag. A bare protocol claim with no citation
   is a bug in the doc; either find the evidence and cite it, or mark it as a guess.

3. **Stale paths and names.** After a rename or a crate split, hunt for references to the old
   name that survived. These are the quiet ones, since nothing breaks at build time.

4. **CLAUDE.md equals AGENTS.md.** They must be the same bytes. `check_docs.sh` enforces it,
   but if you edited one by hand, confirm you edited both.

5. **Confidence tags still honest.** A fact tagged as code-inferred that has since been checked
   against a real capture should have moved up, and a claim contradicted by a new fixture
   should move down or out. The hardware-epoch checklist in `docs/protocol/whoop.md` is where
   the code-inferred facts wait for a real strap; keep it current.

## Output

File targeted fixes, one concern at a time, the same way any other change is made. Where a doc
and the code disagree and you genuinely cannot tell which is correct, flag it and say so
rather than guessing; a confident wrong correction is the exact failure this skill exists to
stop.
