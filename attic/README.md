# attic

Complete crates that lost their caller when a boundary moved. Parked under ADR-025, not deleted:
they are working code with tests and fixtures, and git history is a worse place to find that than a
directory saying what it is.

Nothing here is in the workspace, in `tools/check_deps.py`, or in any gate. Nothing here is a
dependency of anything that ships.

| directory | what it is | what would bring it back |
|---|---|---|
| `mav-frame` | CRC-8/16/32, the frame reassembler, the typed reader | a host-side transport that is not connector-mediated |
| `mav-codec` | the explicitly admitted open Bluetooth SIG profile decoders | a bundled SIG-profile source, admitted without a connector — `docs/platform.md` still describes one |
| `fixtures-standard` | the spec-derived Heart Rate Measurement vectors `mav-codec` is pinned by | moves back with `mav-codec` |

Re-admission means moving the directory back to `core/crates/`, restoring its workspace membership
and its `tools/check_deps.py` edge, and running the full gate. It does not mean copying fragments
out: the tests come with the code, and code arriving without them is code nobody has checked.

ADR-016, ADR-017, and ADR-020 record why the boundary moved. Connectors reassemble and CRC-check
their own frames inside the signed artifact; `whoop-protocol` in the connectors repository is where
that lives now.
