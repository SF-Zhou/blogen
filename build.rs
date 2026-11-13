//! Build script: make sure the embedded default theme is re-embedded when any
//! of its files change.
fn main() {
    println!("cargo:rerun-if-changed=themes");
    println!("cargo:rerun-if-changed=vendor");
}
