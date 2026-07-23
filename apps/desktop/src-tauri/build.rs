fn main() {
    println!("cargo:rerun-if-changed=src/contracts.rs");
    tauri_build::build();
}
