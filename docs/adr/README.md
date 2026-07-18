# Architecture decision records

- [ADR-001](ADR-001.md) — One shared Rust core, thin native bindings
- [ADR-002](ADR-002.md) — A synchronous typed pipeline, not event buses
- [ADR-003](ADR-003.md) — Append-only raw storage
- [ADR-004](ADR-004.md) — A canonical timeline that never interpolates
- [ADR-005](ADR-005.md) — Capability negotiation
- [ADR-006](ADR-006.md) — ML inference stays native
- [ADR-007](ADR-007.md) — Declarative manifests plus a boxed DeviceCodec
- [ADR-008](ADR-008.md) — Provenance by metadata reference
- [ADR-009](ADR-009.md) — Algorithms admitted only with a golden fixture or published reference
- [ADR-010](ADR-010.md) — UniFFI for the bindings
- [ADR-011](ADR-011.md) — Connectors are a separate importable package, not bundled in the app
- [ADR-012](ADR-012.md) — Frame parameters become manifest data
- [ADR-013](ADR-013.md) — One stateful host runtime beside the stateless fixture runner
- [ADR-014](ADR-014.md) — Additive stream kinds for the WHOOP gen5 K=18 record
- [ADR-015](ADR-015.md) — Stream kinds for the WHOOP gen5 deep buffers (v20 optical, v21 IMU)
- [ADR-016](ADR-016.md) — Device codec crates: WHOOP moves out of the core
- [ADR-017](ADR-017.md) — Runtime-loaded WebAssembly connectors
- [ADR-018](ADR-018.md) — Connector runtime errors own codes 11000–11999
