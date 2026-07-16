//! The uniffi-bindgen entry point, so bindings generate with the same uniffi version the crate
//! builds against. Built only with the `cli` feature; see the platform READMEs for the commands.

fn main() {
    uniffi::uniffi_bindgen_main()
}
