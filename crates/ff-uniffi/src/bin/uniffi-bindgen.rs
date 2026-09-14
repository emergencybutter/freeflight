//! Generates the Kotlin bindings for `ff-uniffi`.
//!
//! Built only with the `cli` feature (see Cargo.toml) and invoked from
//! `apps/android`'s Gradle build against the freshly-built Android
//! library, so the bindings can never describe a different version of
//! the core than the `.so` shipped beside them.

fn main() {
    uniffi::uniffi_bindgen_main()
}
