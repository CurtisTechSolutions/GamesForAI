//! Rebuild embedded migrations when their source changes.
fn main() {
    println!("cargo:rerun-if-changed=migrations/sqlite");
    println!("cargo:rerun-if-changed=migrations/postgres");
}
