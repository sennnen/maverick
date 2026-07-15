---
name: work-packet
description: >
  How to execute one work packet from docs/plans/active/M*.md end to end: read it, write
  the failing tests first, implement until green, run the gates, log the decision, commit.
  Load this before you start a packet, or when a task references a packet id like M1-P3 or
  says "implement this packet", "pick up the next packet", or "finish the packet".
---

# Executing a work packet

A packet is the unit of agent work. One packet is one commit. Do not start coding from a
milestone description or a loose instruction; find the packet, or write one first.

## Steps

1. **Read the packet in full.** It is a block in `docs/plans/active/M*.md` with these fields:
   `Owns` (the files this packet may create or change), `Must not touch`, `Contract` (the
   exact signatures and types it implements or consumes, already frozen), `Tests first` (the
   tests to write and what each asserts), `Exit` (the commands that must pass), and `Notes`
   (gotchas and references into `docs/`). Hold the `Owns` list in your head; it is the fence.

2. **Read the documents it references before writing anything.** If the packet points at
   `docs/pipeline.md` or `docs/protocol/whoop.md`, read those sections now. The contract in
   the packet is the frozen surface; the docs are why it looks the way it does.

3. **Write the listed tests first and watch them fail.** Run them and confirm red. A test that
   is green before the code exists is a test that cannot fail, which is a defect here. Each
   test asserts a computed value or an exact error, never `assert!(true)` or a bare call.

4. **Implement until the tests are green, staying inside the owned files.** If you find you
   need to change a file another packet owns, stop. That is not a thing you route around
   locally. See the escalation rule below.

5. **Run the full gate.** All of these must pass, not just the crate you touched:

       cargo test --workspace
       cargo fmt --check
       cargo clippy --workspace --all-targets -- -D warnings
       tools/check_docs.sh
       tools/check_deps.py

6. **Update the plan file.** Mark the packet status, and add a short decision-log entry for
   anything you chose that the next reader would otherwise have to reconstruct: an offset you
   pinned from a fixture, a name you picked, a gotcha you hit.

7. **Commit once.** Imperative subject, the why in the body when the diff does not make it
   obvious. One packet, one commit or PR.

## Rules

- You never edit another packet's owned files. The `Owns` lists are how a swarm avoids
  stepping on itself, and they only work if everyone honours them.
- When two packets disagree about an interface, that is an ADR, not a workaround. Stop, write
  or amend a record under `docs/adr/`, and let the interface change land deliberately. A local
  patch that quietly reshapes a frozen type is exactly the failure the packet system prevents.
- When the last packet in a milestone is done, move the plan file from `docs/plans/active/`
  to `docs/plans/completed/` and add a short retro: what surprised you, what drifted, which
  doc fixes you filed.
