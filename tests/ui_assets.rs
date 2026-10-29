//! tests/ui_assets.rs
//! DELTA_08 paso 2: los assets de marca existen, tienen el formato esperado y Windows los carga
//! con su propio cargador (`LoadImageW`), que es el que usará la ventana en el paso 4.

use std::path::PathBuf;

fn asset(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets").join(name)
}

#[test]
fn logo_png_is_128_rgba_delta08() {
    let png = std::fs::read(asset("galapagos-logo.png")).expect("assets/galapagos-logo.png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(&png[12..16], b"IHDR");
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    assert_eq!((width, height), (128, 128));
    assert_eq!(png[25], 6, "color type 6 = RGBA (fondo transparente)");
}

#[test]
fn icon_has_expected_dib_entries_delta08() {
    let ico = std::fs::read(asset("galapagos.ico")).expect("assets/galapagos.ico");
    let u16_at = |o: usize| u16::from_le_bytes(ico[o..o + 2].try_into().unwrap());
    let u32_at = |o: usize| u32::from_le_bytes(ico[o..o + 4].try_into().unwrap()) as usize;
    assert_eq!((u16_at(0), u16_at(2)), (0, 1), "cabecera ICONDIR");
    let count = u16_at(4) as usize;

    let mut sizes = Vec::new();
    for i in 0..count {
        let e = 6 + 16 * i;
        let size = if ico[e] == 0 { 256 } else { ico[e] as u32 };
        let (len, offset) = (u32_at(e + 8), u32_at(e + 12));
        assert!(offset + len <= ico.len(), "entrada {} fuera del archivo", size);
        // DIB: BITMAPINFOHEADER de 40 bytes, alto doble (imagen + máscara), 32 bits por píxel
        assert_eq!(u32_at(offset), 40, "entrada {} no es DIB", size);
        assert_eq!(u32_at(offset + 8), (size * 2) as usize);
        assert_eq!(u16_at(offset + 14), 32);
        sizes.push(size);
    }
    assert_eq!(sizes, vec![16, 20, 24, 32, 40, 48, 64, 128]);
}

#[test]
fn windows_loads_icon_from_file_delta08() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, LoadImageW, IMAGE_ICON, LR_LOADFROMFILE};
    let path: Vec<u16> = asset("galapagos.ico").to_string_lossy().encode_utf16().chain(Some(0)).collect();
    for size in [16, 32, 48] {
        let icon = unsafe { LoadImageW(0, path.as_ptr(), IMAGE_ICON, size, size, LR_LOADFROMFILE) };
        assert_ne!(icon, 0, "LoadImageW no cargó el icono de {} px", size);
        unsafe { DestroyIcon(icon) };
    }
}
