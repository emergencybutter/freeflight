//! Regenerates `apps/web/src/chartVocabulary.ts` from
//! `ff_charts::vocabulary`, which is the single source of truth for chart
//! naming and airport display thresholds across both clients.
//!
//! ```sh
//! cargo run -p ff-core --bin gen-web-vocabulary
//! ```
//!
//! `vocabulary::tests::generated_typescript_is_current` fails if the
//! checked-in file and this output disagree, so forgetting to run it is a
//! test failure rather than a silent divergence between the clients.
fn main() {
    let out = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/web/src/chartVocabulary.ts"
    );
    let contents = ff_core::vocabulary::to_typescript();
    std::fs::write(out, &contents).expect("write chartVocabulary.ts");
    println!("wrote {} ({} bytes)", out, contents.len());
}
