//! build.rs del Coordinador (galapagos-browser)
//! DELTA_08 paso 1: embebe `assets/galapagos.manifest` en el ejecutable con el enlazador de MSVC
//! (controles comunes v6 y escala por monitor). Sin crates: solo argumentos de enlace, y solo
//! para el binario (`-bins`), no para los tests.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/galapagos.manifest");

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("galapagos.manifest");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
}
