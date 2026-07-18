# Connector template

This device-neutral crate proves the public SDK export surface. Build it with:

```text
cargo build --manifest-path core/templates/connector/Cargo.toml \
  --target wasm32-unknown-unknown --release --lib
```

Copy the crate outside Maverick, replace the local development dependency with the released
`mav-connector-sdk` version, implement the deterministic state machine, and define artifact metadata
with `artifact_metadata!`. Package metadata with `mavconn-pack`; keep the external Ed25519 signer and
all private material outside the project and tool arguments.

Generate canonical metadata with:

```text
cargo run --manifest-path core/templates/connector/Cargo.toml --bin metadata -- metadata-out
```
