//! Throwaway: create an empty ff-storage-schema bundle at the given path,
//! for testing add_* functions against a small file instead of the real
//! ~145MB cycle bundle.
//!
//! ```sh
//! cargo run -p ff-etl --example mkempty_bundle -- /path/to/out.sqlite
//! ```
fn main() {
    let out = std::env::args().nth(1).expect("usage: mkempty_bundle <path>");
    let _ = std::fs::remove_file(&out);
    ff_storage::open(&out).expect("create ff-storage schema");
    println!("created empty bundle at {out}");
}
