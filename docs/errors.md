# Errors and observability

Error handling is milestone-zero work in Maverick, delivered before the first byte of protocol code,
and this document explains both the model and why it comes first. The short version of the why: this
codebase is written by a swarm of agents, and a swarm without forensics ships silent corruption. When
a decode goes subtly wrong on a normal team, someone eventually notices the odd chart and goes
digging. In a swarm, the agent that wrote the bug has no memory of it, the agent that touches the
code next has no suspicion, and a value that is wrong but plausible will flow through features into
metrics unchallenged. The only defence is that every anomaly leaves a record at the moment it
happens, with enough context to walk back to the bytes that caused it. That machinery cannot be
retrofitted; if the first milestones are built without it, the data they produced was never
witnessed, and there is no going back to check.

## MavError

Every fallible operation in the core returns `Result<T, MavError>`. The error type has five parts:

- **Code** — a stable numeric code, unique to one failure condition.
- **Category** — one of `Transport`, `Frame`, `Decode`, `Timeline`, `Storage`, `Feature`,
  `Analytic`, `Ml`, `Ffi`, `Internal`.
- **Message** — human-readable, specific, and stating what was expected against what was found.
- **Context chain** — the trail of what was being done when the failure happened, accumulated as
  the error propagates up: which device, which frame, which stage.
- **Severity** — how bad this is, from a logged-and-continue rejection up to a corruption risk.

The code is the part with the strictest rule: **codes are append-only.** A code, once assigned, is
never renumbered, never reused, and never deleted; a condition that stops being possible keeps its
code, marked retired. This is what makes a code stable enough to grep three months of journals for,
to cite in a fixture, and to reference from documentation. All codes are documented in this file as
they are assigned, next to the condition they name.

Library code does not panic. `unwrap`, `expect`, and `panic!` are denied by the clippy
configuration for library code and allowed in tests. An impossible state is an `Internal` error
with a code, not a crash, because a crash in an FFI'd library takes the host app down with it.

## No silent drops

Stated in [pipeline.md](pipeline.md) and enforced here: nothing is discarded without a record. Every
dropped packet, fragment, frame, or sample logs its error code and reason at the point of the drop.
A frame that fails CRC, a fragment the reassembler skips, a sample rejected by a plausibility gate,
an RR interval outside the valid window: each one is an entry in the journal, not an absence. The
discipline pays off in both directions. When data is missing, the journal says why; when the journal
is quiet, missing data means the device genuinely did not send it, and that distinction is the
difference between debugging and guessing.

## Tracing

The core uses the `tracing` crate, with one span per stage per unit of work. Ids (frame, session,
stream, device) are attached as span fields, so a log line is never orphaned from the data it
concerns. Because the pipeline is a synchronous call graph, spans nest in exactly the order the
stages ran, and a trace of one frame's journey reads top to bottom without interleaving.

## Sinks

There are two, chosen for different questions.

The **ring buffer** is in-memory, cheap, and always on. It holds the recent past at full detail and
answers "what just happened", which is what you want mid-session and what the report bundle
snapshots when the user hits a problem.

The **error journal** is a table in SQLite, written through `mav-store`, and holds errors durably
across restarts. It answers "what has been happening", which is what you want when a user reports
that last Tuesday's sleep looks wrong. Journal entries carry their code, category, severity,
context, and the relevant ids, so a journal row can be joined back to the samples and frames it
describes.

Beyond the sinks, the `Tap` trait (described in [pipeline.md](pipeline.md)) gives boundary-level
observation of the healthy path: counts and ids at every stage boundary, payload summaries in debug
builds. The taps are how you see what the pipeline is doing; the sinks are how you see what it
refused to do.

## The walk-back requirement

For any metric value on screen, it must be possible to walk backwards through stored ids: metric to
features, features to samples, samples to frames, frames to raw bytes. This is a requirement on the
whole system, checked in the hardening milestone, and it is what the provenance table in
[storage.md](storage.md) exists to serve. An error model that logs codes but cannot connect them to
data would be half the job.

## The report bundle

User-facing error reporting exists from day one, over FFI, in two forms.

First, a recent-errors query, so the apps can show an in-app diagnostics screen without any custom
plumbing.

Second, `export_report_bundle()`, which produces a zip containing: the app and core versions, the
device model, the manifest versions in use, a slice of the error journal, and the contents of the
ring buffer. The bundle is **redacted by default**: it contains no raw health samples unless the
user explicitly opts in. The versions matter as much as the errors, because a bug report that does
not pin the algorithm and manifest versions cannot be reproduced, and a manifest version can change
the meaning of a decode.

The bundle is designed for the failure mode this project will actually have: a user with a strap we
have never physically tested, seeing something wrong, and no way for us to attach a debugger. The
bundle is the debugger. If it is good, a protocol fact can be corrected from a single report; if it
is an afterthought, every field report is an unreproducible anecdote.
