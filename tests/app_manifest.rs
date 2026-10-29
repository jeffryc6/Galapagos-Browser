//! tests/app_manifest.rs
//! DELTA_08 paso 1: el ejecutable real del Coordinador lleva embebido su manifiesto
//! (controles comunes v6 y escala PerMonitorV2). Se inspecciona el binario que Cargo acaba de enlazar.

#[test]
fn coordinator_exe_embeds_app_manifest_delta08() {
    let exe = std::fs::read(env!("CARGO_BIN_EXE_galapagos-browser")).expect("leer galapagos-browser.exe");
    let contains = |needle: &str| exe.windows(needle.len()).any(|w| w == needle.as_bytes());
    assert!(contains("Microsoft.Windows.Common-Controls"), "falta la dependencia de controles comunes v6");
    assert!(contains("PerMonitorV2"), "falta la escala por monitor");
    assert!(contains("Galapagos.Browser.Coordinator"), "falta la identidad del ensamblado");
}
