//! src/window/theme.rs
//! DELTA_08 paso 3: tema nativo de Galapagos. Contiene la paleta de marca, las fuentes del sistema
//! (Segoe UI Variable y Segoe Fluent Icons) y el dibujo suavizado con GDI+.
//! Sin crates: GDI y GDI+ de Windows vía `windows-sys`. Los fallos de GDI+ o del logo se informan
//! con `bool`/`Option`: el llamador los registra (`UI_ASSET_UNAVAILABLE`) y dibuja sin ellos.

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DeleteObject, EnumFontFamiliesExW, GetDC, ReleaseDC, CLEARTYPE_QUALITY,
    DEFAULT_CHARSET, FW_NORMAL, FW_SEMIBOLD, HDC, HFONT, LOGFONTW, TEXTMETRICW,
};
use windows_sys::Win32::Graphics::GdiPlus as gp;

/// Color sRGB de la paleta
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `COLORREF` de GDI (0x00BBGGRR)
    pub const fn colorref(self) -> u32 {
        (self.0 as u32) | ((self.1 as u32) << 8) | ((self.2 as u32) << 16)
    }

    /// `ARGB` de GDI+ (0xAARRGGBB)
    pub const fn argb(self, alpha: u8) -> u32 {
        ((alpha as u32) << 24) | ((self.0 as u32) << 16) | ((self.1 as u32) << 8) | self.2 as u32
    }

    /// Luminancia relativa (WCAG 2.x)
    fn luminance(self) -> f64 {
        let channel = |c: u8| {
            let c = c as f64 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(self.0) + 0.7152 * channel(self.1) + 0.0722 * channel(self.2)
    }
}

/// Relación de contraste WCAG entre dos colores (1.0 a 21.0)
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Paleta de marca (tablero de marca de Galapagos). Sustituye a `GALAPAGOS_DARK` del contrato (R-18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GalapagosTheme {
    /// Fondo de ventana
    pub volcanic_black: Rgb,
    /// Barra de herramientas
    pub deep_ocean: Rgb,
    /// Tarjetas y campo de dirección
    pub lava_stone: Rgb,
    /// Bordes y separadores
    pub stone_border: Rgb,
    /// Acento: elemento activo y alertas
    pub iguana_fire: Rgb,
    /// Foco, iconos y enlaces
    pub pacific_teal: Rgb,
    /// Estado protegido (solo si es un hecho medido, regla UI-I0)
    pub evolution_green: Rgb,
    pub text_primary: Rgb,
    pub text_secondary: Rgb,
}

pub const THEME: GalapagosTheme = GalapagosTheme {
    volcanic_black: Rgb(0x0B, 0x12, 0x1A),
    deep_ocean: Rgb(0x0F, 0x1C, 0x27),
    lava_stone: Rgb(0x22, 0x2B, 0x34),
    stone_border: Rgb(0x3A, 0x45, 0x52),
    iguana_fire: Rgb(0xE6, 0x6A, 0x1F),
    pacific_teal: Rgb(0x1D, 0x9B, 0xB8),
    evolution_green: Rgb(0x43, 0xA0, 0x47),
    text_primary: Rgb(0xE6, 0xED, 0xF3),
    text_secondary: Rgb(0x8C, 0x9B, 0xAB),
};

/// Escala un valor en píxeles a 96 DPI al DPI del monitor (redondeo como `MulDiv`)
pub fn scale(px: i32, dpi: u32) -> i32 {
    (px * dpi as i32 + 48) / 96
}

/// Familias de texto en orden de preferencia: Windows 11 trae Segoe UI Variable; Windows 10, Segoe UI
pub const TEXT_FACES: [&str; 2] = ["Segoe UI Variable Text", "Segoe UI"];
/// Familias de iconos: Segoe Fluent Icons (Windows 11) o Segoe MDL2 Assets (Windows 10); mismos códigos
pub const ICON_FACES: [&str; 2] = ["Segoe Fluent Icons", "Segoe MDL2 Assets"];

/// Glifos de Segoe Fluent Icons / MDL2 Assets
pub mod glyph {
    pub const BACK: char = '\u{E72B}';
    pub const FORWARD: char = '\u{E72A}';
    pub const REFRESH: char = '\u{E72C}';
    pub const HOME: char = '\u{E80F}';
    pub const SHIELD: char = '\u{EA18}';
    pub const INFO: char = '\u{E946}';
}

fn face_name(face: &str) -> [u16; 32] {
    let mut name = [0u16; 32];
    for (dst, src) in name.iter_mut().zip(face.encode_utf16().take(31)) {
        *dst = src;
    }
    name
}

unsafe extern "system" fn mark_found(_: *const LOGFONTW, _: *const TEXTMETRICW, _: u32, found: LPARAM) -> i32 {
    *(found as *mut bool) = true;
    0 // basta con una coincidencia
}

/// La familia está instalada en el sistema (`EnumFontFamiliesExW` sobre la pantalla)
pub fn font_available(face: &str) -> bool {
    let mut found = false;
    let mut lf: LOGFONTW = unsafe { std::mem::zeroed() };
    lf.lfCharSet = DEFAULT_CHARSET as u8;
    lf.lfFaceName = face_name(face);
    unsafe {
        let hdc = GetDC(0);
        EnumFontFamiliesExW(hdc, &lf, Some(mark_found), &mut found as *mut bool as LPARAM, 0);
        ReleaseDC(0, hdc);
    }
    found
}

/// Primera familia instalada de la lista; si no hay ninguna, la última (GDI la sustituye)
pub fn first_available(faces: &[&'static str]) -> &'static str {
    faces.iter().copied().find(|f| font_available(f)).unwrap_or(faces[faces.len() - 1])
}

/// Fuente GDI con liberación automática (`DeleteObject`)
pub struct UiFont(HFONT);

impl UiFont {
    /// `px` es la altura de carácter a 96 DPI; se escala al `dpi` del monitor
    pub fn new(face: &str, px: i32, weight: u32, dpi: u32) -> Option<Self> {
        let mut lf: LOGFONTW = unsafe { std::mem::zeroed() };
        lf.lfHeight = -scale(px, dpi);
        lf.lfWeight = weight as i32;
        lf.lfCharSet = DEFAULT_CHARSET as u8;
        lf.lfQuality = CLEARTYPE_QUALITY as u8;
        lf.lfFaceName = face_name(face);
        let font = unsafe { CreateFontIndirectW(&lf) };
        (font != 0).then_some(Self(font))
    }

    pub fn handle(&self) -> HFONT {
        self.0
    }
}

impl Drop for UiFont {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.0);
        }
    }
}

/// Juego de fuentes de la interfaz para un DPI concreto
pub struct UiFonts {
    pub text: UiFont,
    pub text_semibold: UiFont,
    pub title: UiFont,
    pub icons: UiFont,
}

impl UiFonts {
    pub fn for_dpi(dpi: u32) -> Option<Self> {
        let text_face = first_available(&TEXT_FACES);
        let icon_face = first_available(&ICON_FACES);
        Some(Self {
            text: UiFont::new(text_face, 14, FW_NORMAL, dpi)?,
            text_semibold: UiFont::new(text_face, 14, FW_SEMIBOLD, dpi)?,
            title: UiFont::new(text_face, 22, FW_SEMIBOLD, dpi)?,
            icons: UiFont::new(icon_face, 16, FW_NORMAL, dpi)?,
        })
    }
}

/// Sesión de GDI+ (una por proceso de UI, viva mientras haya ventana)
pub struct GdiPlus {
    token: usize,
}

impl GdiPlus {
    pub fn start() -> Option<Self> {
        let input = gp::GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        let mut token = 0usize;
        let status = unsafe { gp::GdiplusStartup(&mut token, &input, null_mut()) };
        (status == gp::Ok).then_some(Self { token })
    }
}

impl Drop for GdiPlus {
    fn drop(&mut self) {
        unsafe { gp::GdiplusShutdown(self.token) };
    }
}

/// Imagen cargada con GDI+ (PNG con alfa); se libera con `GdipDisposeImage`
pub struct Image(*mut gp::GpBitmap);

impl Image {
    pub fn load(path: &std::path::Path) -> Option<Self> {
        let wide: Vec<u16> = path.as_os_str().encode_wide_nul();
        let mut bitmap = null_mut();
        let status = unsafe { gp::GdipCreateBitmapFromFile(wide.as_ptr(), &mut bitmap) };
        (status == gp::Ok && !bitmap.is_null()).then_some(Self(bitmap))
    }

    pub fn width(&self) -> u32 {
        let mut w = 0;
        unsafe { gp::GdipGetImageWidth(self.0 as *mut gp::GpImage, &mut w) };
        w
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe { gp::GdipDisposeImage(self.0 as *mut gp::GpImage) };
    }
}

trait EncodeWideNul {
    fn encode_wide_nul(&self) -> Vec<u16>;
}

impl EncodeWideNul for std::ffi::OsStr {
    fn encode_wide_nul(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(Some(0)).collect()
    }
}

/// Lienzo GDI+ sobre un HDC con antialiasing; requiere una `GdiPlus` viva
pub struct Canvas(*mut gp::GpGraphics);

/// Ruta GDI+ con liberación automática
struct Path(*mut gp::GpPath);

impl Drop for Path {
    fn drop(&mut self) {
        unsafe { gp::GdipDeletePath(self.0) };
    }
}

/// Rectángulo de esquinas redondeadas como ruta cerrada (cuatro arcos de 90°)
fn round_rect_path(rect: RECT, radius: f32) -> Option<Path> {
    let mut raw = null_mut();
    if unsafe { gp::GdipCreatePath(gp::FillModeAlternate, &mut raw) } != gp::Ok {
        return None;
    }
    let path = Path(raw);
    let (l, t) = (rect.left as f32, rect.top as f32);
    let (w, h) = ((rect.right - rect.left) as f32, (rect.bottom - rect.top) as f32);
    let d = (2.0 * radius).min(w).min(h).max(0.0);
    let arcs = [
        (l, t, 180.0),
        (l + w - d, t, 270.0),
        (l + w - d, t + h - d, 0.0),
        (l, t + h - d, 90.0),
    ];
    for (x, y, start) in arcs {
        if unsafe { gp::GdipAddPathArc(path.0, x, y, d, d, start, 90.0) } != gp::Ok {
            return None;
        }
    }
    (unsafe { gp::GdipClosePathFigure(path.0) } == gp::Ok).then_some(path)
}

impl Canvas {
    pub fn new(hdc: HDC) -> Option<Self> {
        let mut g = null_mut();
        if unsafe { gp::GdipCreateFromHDC(hdc, &mut g) } != gp::Ok || g.is_null() {
            return None;
        }
        unsafe {
            gp::GdipSetSmoothingMode(g, gp::SmoothingModeAntiAlias);
            gp::GdipSetPixelOffsetMode(g, gp::PixelOffsetModeHighQuality);
            gp::GdipSetInterpolationMode(g, gp::InterpolationModeHighQualityBicubic);
        }
        Some(Self(g))
    }

    /// Rellena un rectángulo redondeado con un color ARGB
    pub fn fill_round_rect(&self, rect: RECT, radius: f32, argb: u32) -> bool {
        let Some(path) = round_rect_path(rect, radius) else { return false };
        let mut brush = null_mut();
        if unsafe { gp::GdipCreateSolidFill(argb, &mut brush) } != gp::Ok {
            return false;
        }
        let ok = unsafe { gp::GdipFillPath(self.0, brush as *mut gp::GpBrush, path.0) } == gp::Ok;
        unsafe { gp::GdipDeleteBrush(brush as *mut gp::GpBrush) };
        ok
    }

    /// Traza el borde de un rectángulo redondeado
    pub fn stroke_round_rect(&self, rect: RECT, radius: f32, argb: u32, width: f32) -> bool {
        let Some(path) = round_rect_path(rect, radius) else { return false };
        let mut pen = null_mut();
        if unsafe { gp::GdipCreatePen1(argb, width, gp::UnitPixel, &mut pen) } != gp::Ok {
            return false;
        }
        let ok = unsafe { gp::GdipDrawPath(self.0, pen, path.0) } == gp::Ok;
        unsafe { gp::GdipDeletePen(pen) };
        ok
    }

    /// Dibuja una imagen escalada al rectángulo
    pub fn draw_image(&self, image: &Image, rect: RECT) -> bool {
        unsafe {
            gp::GdipDrawImageRectI(
                self.0,
                image.0 as *mut gp::GpImage,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            ) == gp::Ok
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe { gp::GdipDeleteGraphics(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, GdiFlush, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };

    /// WCAG AA: texto normal ≥ 4.5:1 sobre sus fondos; acentos de icono ≥ 3:1
    #[test]
    fn palette_meets_contrast_delta08() {
        let t = THEME;
        for bg in [t.volcanic_black, t.deep_ocean, t.lava_stone] {
            assert!(contrast_ratio(t.text_primary, bg) >= 4.5, "texto principal sobre {:?}", bg);
            assert!(contrast_ratio(t.text_secondary, bg) >= 4.5, "texto secundario sobre {:?}", bg);
            for accent in [t.pacific_teal, t.iguana_fire, t.evolution_green] {
                assert!(contrast_ratio(accent, bg) >= 3.0, "acento {:?} sobre {:?}", accent, bg);
            }
        }
        assert!((contrast_ratio(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 1e-9);
    }

    #[test]
    fn color_encodings_delta08() {
        assert_eq!(THEME.iguana_fire.colorref(), 0x001F_6AE6);
        assert_eq!(THEME.iguana_fire.argb(0xFF), 0xFFE6_6A1F);
    }

    #[test]
    fn dpi_scaling_delta08() {
        assert_eq!(scale(14, 96), 14);
        assert_eq!(scale(14, 144), 21);
        assert_eq!(scale(46, 192), 92);
        assert_eq!(scale(26, 120), 33); // 32.5 redondea hacia arriba
    }

    /// Las fuentes se crean con una familia realmente instalada
    #[test]
    fn fonts_resolve_installed_faces_delta08() {
        assert!(font_available(first_available(&TEXT_FACES)));
        assert!(font_available(first_available(&ICON_FACES)));
        assert!(!font_available("Galapagos Fuente Inexistente"));
        assert!(UiFonts::for_dpi(144).is_some());
    }

    /// GDI+ real: el relleno redondeado pinta el centro y deja la esquina exterior intacta
    #[test]
    fn gdiplus_round_rect_paints_rounded_corners_delta08() {
        let _gdiplus = GdiPlus::start().expect("GdiplusStartup");
        unsafe {
            let dc = CreateCompatibleDC(0);
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 40,
                biHeight: -40, // de arriba abajo
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB as u32,
                ..std::mem::zeroed()
            };
            let mut bits: *mut std::ffi::c_void = null_mut();
            let dib = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, 0, 0);
            assert_ne!(dib, 0);
            let old = SelectObject(dc, dib);
            {
                let canvas = Canvas::new(dc).expect("lienzo GDI+");
                let rect = RECT { left: 0, top: 0, right: 40, bottom: 40 };
                assert!(canvas.fill_round_rect(rect, 12.0, THEME.pacific_teal.argb(0xFF)));
            }
            GdiFlush();
            let pixels = std::slice::from_raw_parts(bits as *const u32, 40 * 40);
            let at = |x: usize, y: usize| pixels[y * 40 + x] & 0x00FF_FFFF; // BGRA → 0x00RRGGBB
            assert_eq!(at(20, 20), THEME.pacific_teal.argb(0) & 0x00FF_FFFF, "centro relleno");
            assert_eq!(at(0, 0), 0, "la esquina exterior queda sin pintar");
            SelectObject(dc, old);
            DeleteObject(dib);
            DeleteDC(dc);
        }
    }

    /// El logo del paso 2 se carga con GDI+ con su tamaño real
    #[test]
    fn logo_loads_with_gdiplus_delta08() {
        let _gdiplus = GdiPlus::start().expect("GdiplusStartup");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("galapagos-logo.png");
        let logo = Image::load(&path).expect("galapagos-logo.png");
        assert_eq!(logo.width(), 128);
        assert!(Image::load(std::path::Path::new("Z:\\no-existe.png")).is_none());
    }
}
