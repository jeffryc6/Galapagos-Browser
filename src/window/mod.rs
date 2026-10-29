//! src/window/mod.rs
//! Marco de Ventana Nativa Win32 (HWND) para Galapagos Browser v0.1
//! Conforme al estándar ZNVE v2.4.0 (Axiomas 1 y 2, ARC-02, ADR-003).
//! UI desacoplada del motor de renderizado, sin frameworks web parásitos (cero Electron).
//! DELTA_08 paso 4: barra de título oscura (DWM), icono, escala por DPI y barra de herramientas
//! dibujada con el tema (← → ⟳ ⌂, campo de dirección redondeado e insignia de estado medido).

pub mod theme;

use crate::engine::{InputDispatcher, InputEvent};
use crate::galapagos_core_v1::{AddressBarInput, GalapagosError};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Once;
use theme::{glyph, scale, Canvas, GdiPlus, Image, Rgb, UiFonts, THEME};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DeleteDC,
    DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect, InvalidateRect, ScreenToClient,
    SelectObject, SetBkColor, SetBkMode, SetTextColor, UpdateWindow, ValidateRect, HBRUSH, HDC,
    HFONT, PAINTSTRUCT, SRCCOPY,
};
pub use galapagos_ipc::NavState;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_RETURN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, DispatchMessageW, GetClientRect,
    GetDlgItem, GetMessageW, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, KillTimer,
    LoadCursorW, LoadImageW, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassW,
    SendMessageW, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
    TranslateMessage, CW_USEDEFAULT, GWLP_USERDATA, HICON, ICON_SMALL, IDC_ARROW, IMAGE_ICON,
    LR_LOADFROMFILE, MSG, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOZORDER, SW_HIDE, SW_SHOW, WM_CHAR, WM_DPICHANGED, WM_KEYDOWN, WM_KEYUP,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETFONT, WM_SETICON, WM_TIMER, WNDCLASSW, WS_CHILD,
    WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

/// Identificador del campo de dirección. D32: el botón «Navegar» (1002) se retiró; Enter navega.
pub const IDC_URL_EDIT: isize = 1001;

/// WM_APP + 2: `WM_SIZE` re-publicado al bucle propio (DELTA_04 L-I5); lParam = tamaño cliente
pub const WM_APP_RESIZE: u32 = 0x8002;
/// WM_APP + 3: clic en un botón de la barra re-publicado al bucle propio; wParam = `ToolbarButton::index`
pub const WM_APP_TOOLBAR: u32 = 0x8003;
const TIMER_ID_PUMP: usize = 1;
/// N-I5: cadencia del temporizador que sirve al pipe del worker sin bloquear la UI
pub const PUMP_INTERVAL_MS: u32 = 16;

/// Δ8: altura de la barra a 96 DPI; la real es `scale(TOOLBAR_HEIGHT_96, dpi)` (`NativeWindow::toolbar_height`)
pub const TOOLBAR_HEIGHT_96: i32 = 46;
/// Destino del botón Inicio; pasa por el camino único de navegación como cualquier entrada
pub const HOME_URL: &str = "galapagos://dashboard";
/// Ancho cliente (a 96 DPI) por debajo del cual el logo no se dibuja
const LOGO_MIN_WIDTH_96: i32 = 640;

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_CTLCOLOREDIT: u32 = 0x0133;
const EN_SETFOCUS: u32 = 0x0100;
const EN_KILLFOCUS: u32 = 0x0200;
const ICON_BIG: u32 = 1;
const ES_AUTOHSCROLL: u32 = 0x0080;
const DT_CENTER: u32 = 0x0001;
const DT_VCENTER: u32 = 0x0004;
const DT_SINGLELINE: u32 = 0x0020;
const DT_END_ELLIPSIS: u32 = 0x8000;

/// DELTA_02: eventos que la UI entrega al bucle principal. `Tick` es interno de la UI (N-I5).
#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    /// Enter en la barra de direcciones o botón Inicio (exactamente uno por acción, N-I2)
    NavigateRequested(AddressBarInput),
    /// Teclado, ratón o rueda sobre la superficie del worker (N-I7)
    SurfaceInput(InputEvent),
    Tick,
    /// DELTA_04 L-I5: nuevo tamaño del área cliente (> 0); el consumidor redimensiona la superficie
    Resize { width: u32, height: u32 },
    /// DELTA_08 paso 5: botones habilitados por `MSG_NAV_STATE` (mensajes 14–16)
    GoBack,
    GoForward,
    Reload,
    /// DELTA_08 paso 5 (R-3): Windows pide repintar la superficie del worker (mensaje 17)
    SurfaceRepaint,
    CloseRequested,
}

/// Δ8: botones de la barra, en orden de dibujo
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarButton {
    Back,
    Forward,
    Reload,
    Home,
}

impl ToolbarButton {
    pub const ALL: [ToolbarButton; 4] = [Self::Back, Self::Forward, Self::Reload, Self::Home];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<Self> {
        Self::ALL.get(i).copied()
    }

    fn glyph(self) -> char {
        match self {
            Self::Back => glyph::BACK,
            Self::Forward => glyph::FORWARD,
            Self::Reload => glyph::REFRESH,
            Self::Home => glyph::HOME,
        }
    }

    /// UI-I0: sin un `MSG_NAV_STATE` recibido no hay estado medido; solo Inicio está habilitado
    #[cfg(test)]
    fn enabled(self) -> bool {
        self.enabled_with(None)
    }

    /// DELTA_08 paso 5: atrás y adelante siguen el historial real del worker; recargar, que haya worker
    fn enabled_with(self, nav: Option<NavState>) -> bool {
        match (self, nav) {
            (Self::Home, _) => true,
            (_, None) => false,
            (Self::Back, Some(n)) => n.can_back,
            (Self::Forward, Some(n)) => n.can_forward,
            (Self::Reload, Some(_)) => true,
        }
    }
}

/// Motivo legible de una navegación no permitida (cada uno sale de un veredicto real)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    /// `DefenseDecision::Block`
    Threat,
    /// `DefenseDecision::RequireConfirmation` sin consentimiento
    ConfirmationRequired,
    /// `DefenseDecision::Unavailable` (fallo seguro)
    VerificationUnavailable,
    /// `Coordinator::navigate_confirmed` la rechazó
    CoordinatorDenied,
    /// Connectivity Island o el contexto de la pestaña la rechazó
    InvalidDestination,
    /// `normalize_address_input` rechazó el texto de la barra
    InvalidAddress,
}

/// DELTA_08 D33: insignia de la barra. Se calcula a partir de hechos medidos, nunca de constantes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityBadge {
    /// Destino evaluado por Ocean Defense y permitido
    Allowed,
    /// Ocean Defense bloqueó o pidió confirmación, o la navegación no llegó a evaluarse
    Blocked(BlockReason),
    /// Página interna (galapagos://)
    Internal,
    /// DELTA_11 D49: la página cayó (`MSG_CONTENT_CRASHED`); el worker sigue vivo y la siguiente
    /// navegación sustituye esta insignia
    PageCrashed,
}

impl SecurityBadge {
    pub fn text(self) -> &'static str {
        match self {
            Self::Allowed => "Ocean Defense: permitido",
            Self::Internal => "Página interna",
            Self::PageCrashed => "La página falló",
            Self::Blocked(reason) => match reason {
                BlockReason::Threat => "Ocean Defense: bloqueado",
                BlockReason::ConfirmationRequired => "Ocean Defense: requiere confirmación",
                BlockReason::VerificationUnavailable => "Ocean Defense: verificación no disponible",
                BlockReason::CoordinatorDenied => "Navegación denegada por el Coordinador",
                BlockReason::InvalidDestination => "Destino inválido",
                BlockReason::InvalidAddress => "Dirección rechazada",
            },
        }
    }
}

/// Estado del motor que la barra muestra en lugar de la insignia (R9: el fallo es visible)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineFault {
    Unavailable,
    WorkerFault,
    WorkerTerminated,
}

impl EngineFault {
    pub fn text(self) -> &'static str {
        match self {
            Self::Unavailable => "Motor no disponible",
            Self::WorkerFault => "Motor falló",
            Self::WorkerTerminated => "Worker terminado",
        }
    }
}

/// Rectángulo con igualdad (el `RECT` de windows-sys no la implementa)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Bounds {
    fn centered(left: i32, width: i32, height: i32, bar_height: i32) -> Self {
        let top = (bar_height - height) / 2;
        Self { left, top, right: left + width, bottom: top + height }
    }

    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }

    fn rect(&self) -> RECT {
        RECT { left: self.left, top: self.top, right: self.right, bottom: self.bottom }
    }

    fn inset(&self, dx: i32, dy: i32) -> Self {
        Self { left: self.left + dx, top: self.top + dy, right: self.right - dx, bottom: self.bottom - dy }
    }
}

/// Δ8: geometría de la barra para un ancho cliente y un DPI (lógica pura)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolbarLayout {
    pub height: i32,
    pub logo: Option<Bounds>,
    pub buttons: [Bounds; 4],
    /// Campo de dirección redondeado (fondo Lava Stone)
    pub address: Bounds,
    /// `EDIT` sin borde dentro del campo
    pub edit: Bounds,
    pub badge: Bounds,
}

impl ToolbarLayout {
    pub fn compute(client_width: i32, dpi: u32) -> Self {
        let s = |px: i32| scale(px, dpi);
        let height = s(TOOLBAR_HEIGHT_96);
        let pad = s(8);

        let mut x = pad;
        let logo = if client_width >= s(LOGO_MIN_WIDTH_96) {
            let r = Bounds::centered(x, s(24), s(24), height);
            x = r.right + s(8);
            Some(r)
        } else {
            None
        };

        let (btn, gap) = (s(32), s(4));
        let mut buttons = [Bounds::default(); 4];
        for (i, b) in buttons.iter_mut().enumerate() {
            *b = Bounds::centered(x + i as i32 * (btn + gap), btn, btn, height);
        }
        x = buttons[3].right + s(8);

        let right = client_width - pad;
        let badge_w = s(220).min(((right - x) / 3).max(0));
        let badge = Bounds::centered(right - badge_w, badge_w, s(28), height);

        let address_right = (badge.left - s(8)).max(x + s(100));
        let address = Bounds::centered(x, address_right - x, s(32), height);
        let edit_h = s(20);
        let edit_top = address.top + (address.height() - edit_h) / 2;
        let edit = Bounds {
            left: address.left + s(12),
            top: edit_top,
            right: address.right - s(12),
            bottom: edit_top + edit_h,
        };

        Self { height, logo, buttons, address, edit, badge }
    }

    fn hit_test(&self, x: i32, y: i32) -> Option<ToolbarButton> {
        ToolbarButton::ALL.into_iter().find(|b| self.buttons[b.index()].contains(x, y))
    }
}

/// Destinos relevantes para clasificar un mensaje del bucle (puro, testeable)
#[derive(Debug, Clone, Copy)]
struct LoopTargets {
    main: HWND,
    url_edit: HWND,
    surface: HWND,
}

#[derive(Debug, PartialEq, Eq)]
enum LoopAction {
    Navigate,
    Toolbar(ToolbarButton),
    Surface,
    /// R-3: `WM_PAINT` de la superficie del worker
    SurfacePaint,
    Tick,
    Resize,
    Pass,
}

fn classify_loop_message(msg: u32, wparam: WPARAM, target: HWND, t: &LoopTargets) -> LoopAction {
    match msg {
        // N-I2: Enter en la barra se intercepta antes de TranslateMessage (sin beep ni WM_CHAR)
        WM_KEYDOWN if target == t.url_edit && wparam == VK_RETURN as usize => LoopAction::Navigate,
        WM_APP_TOOLBAR if target == t.main => {
            ToolbarButton::from_index(wparam).map_or(LoopAction::Pass, LoopAction::Toolbar)
        }
        WM_APP_RESIZE if target == t.main => LoopAction::Resize,
        WM_TIMER if target == t.main && wparam == TIMER_ID_PUMP => LoopAction::Tick,
        WM_PAINT if t.surface != 0 && target == t.surface => LoopAction::SurfacePaint,
        // La rueda llega a la ventana con foco; se decide por posición en `run_event_loop`
        WM_MOUSEWHEEL if t.surface != 0 => LoopAction::Surface,
        WM_KEYDOWN | WM_KEYUP | WM_CHAR | WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP
        | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONUP
            if t.surface != 0 && target == t.surface =>
        {
            LoopAction::Surface
        }
        _ => LoopAction::Pass,
    }
}

/// Tamaño cliente (ancho, alto) empaquetado en el lParam de `WM_SIZE`
fn unpack_client_size(lparam: isize) -> (u32, u32) {
    ((lparam & 0xFFFF) as u32, ((lparam >> 16) & 0xFFFF) as u32)
}

/// WM_MOUSEWHEEL trae coordenadas de pantalla; el worker espera coordenadas de la superficie
fn pack_client_lparam(x: i32, y: i32) -> isize {
    (((y as u16 as u32) << 16) | (x as u16 as u32)) as isize
}

fn screen_point_from_lparam(lparam: isize) -> (i32, i32) {
    ((lparam & 0xFFFF) as u16 as i16 as i32, ((lparam >> 16) & 0xFFFF) as u16 as i16 as i32)
}

/// R-2: estilo de la ventana principal (incluye WS_CLIPCHILDREN)
const MAIN_WINDOW_STYLE: u32 = WS_OVERLAPPEDWINDOW | WS_VISIBLE | WS_CLIPCHILDREN;

static REGISTER_WINDOW_CLASS_ONCE: Once = Once::new();
static DASHBOARD_ACTIVE: AtomicBool = AtomicBool::new(false);

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn path_to_wide(path: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Δ8: recursos de UI junto al ejecutable (`<exe>\` o `<exe>\assets\`); en debug, también en el árbol del crate
fn asset_path(name: &str) -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.to_path_buf()));
    let source_tree = cfg!(debug_assertions).then(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"));
    exe_dir
        .iter()
        .flat_map(|d| [d.join(name), d.join("assets").join(name)])
        .chain(source_tree.map(|d| d.join(name)))
        .find(|p| p.is_file())
}

fn log_asset_unavailable(what: &str) {
    crate::diagnostics::log_warn(
        crate::diagnostics::DiagnosticCategory::OsEvent,
        "UI_ASSET_UNAVAILABLE",
        format!("{}: la interfaz se dibuja sin él", what),
        None,
    );
}

/// Contrato §4: si DWM no admite el atributo (Windows 10 anterior a 20H1) la barra queda clara, sin error
fn apply_dark_title_bar(hwnd: HWND) {
    let enabled: i32 = 1;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &enabled as *const i32 as *const _,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// Iconos grande y pequeño de `assets\galapagos.ico` al tamaño del sistema para `dpi`
fn load_window_icons(dpi: u32) -> [HICON; 2] {
    let Some(path) = asset_path("galapagos.ico") else {
        log_asset_unavailable("galapagos.ico no encontrado");
        return [0, 0];
    };
    let wide = path_to_wide(&path);
    let load = |cx, cy| unsafe {
        LoadImageW(0, wide.as_ptr(), IMAGE_ICON, GetSystemMetricsForDpi(cx, dpi), GetSystemMetricsForDpi(cy, dpi), LR_LOADFROMFILE)
            as HICON
    };
    let icons = [load(SM_CXICON, SM_CYICON), load(SM_CXSMICON, SM_CYSMICON)];
    if icons.contains(&0) {
        log_asset_unavailable("galapagos.ico no se pudo cargar");
    }
    icons
}

/// Estado de la UI que lee el procedimiento de ventana (vía `GWLP_USERDATA`). Hilo de UI únicamente;
/// los préstamos de `RefCell` nunca cruzan llamadas Win32 que reentren en `wnd_proc`.
struct UiState {
    dpi: Cell<u32>,
    layout: Cell<ToolbarLayout>,
    hover: Cell<Option<ToolbarButton>>,
    pressed: Cell<Option<ToolbarButton>>,
    tracking_leave: Cell<bool>,
    edit_focused: Cell<bool>,
    badge: Cell<Option<SecurityBadge>>,
    engine_fault: Cell<Option<EngineFault>>,
    nav: Cell<Option<NavState>>,
    /// Última URL real informada por el worker (N-I4)
    engine_url: RefCell<String>,
    fonts: RefCell<Option<UiFonts>>,
    edit_brush: HBRUSH,
    icons: [HICON; 2],
    // Orden de liberación: el logo antes que la sesión GDI+
    logo: Option<Image>,
    gdiplus: Option<GdiPlus>,
}

impl UiState {
    fn new(hwnd: HWND) -> Self {
        let dpi = match unsafe { GetDpiForWindow(hwnd) } {
            0 => 96,
            d => d,
        };
        let fonts = UiFonts::for_dpi(dpi);
        if fonts.is_none() {
            log_asset_unavailable("fuentes Segoe no disponibles");
        }
        let gdiplus = GdiPlus::start();
        let logo = match (&gdiplus, asset_path("galapagos-logo.png")) {
            (None, _) => {
                log_asset_unavailable("GdiplusStartup falló");
                None
            }
            (Some(_), None) => {
                log_asset_unavailable("galapagos-logo.png no encontrado");
                None
            }
            (Some(_), Some(path)) => {
                let image = Image::load(&path);
                if image.is_none() {
                    log_asset_unavailable("galapagos-logo.png no se pudo cargar");
                }
                image
            }
        };
        Self {
            dpi: Cell::new(dpi),
            layout: Cell::new(ToolbarLayout::compute(0, dpi)),
            hover: Cell::new(None),
            pressed: Cell::new(None),
            tracking_leave: Cell::new(false),
            edit_focused: Cell::new(false),
            badge: Cell::new(None),
            engine_fault: Cell::new(None),
            nav: Cell::new(None),
            engine_url: RefCell::new(String::new()),
            fonts: RefCell::new(fonts),
            edit_brush: unsafe { CreateSolidBrush(THEME.lava_stone.colorref()) },
            icons: load_window_icons(dpi),
            logo,
            gdiplus,
        }
    }

    /// Texto y color del glifo de la insignia; un fallo del motor tiene prioridad (R9)
    fn status(&self) -> Option<(char, Rgb, &'static str)> {
        if let Some(fault) = self.engine_fault.get() {
            return Some((glyph::INFO, THEME.iguana_fire, fault.text()));
        }
        self.badge.get().map(|badge| match badge {
            SecurityBadge::Allowed => (glyph::SHIELD, THEME.pacific_teal, badge.text()),
            SecurityBadge::Blocked(_) => (glyph::SHIELD, THEME.iguana_fire, badge.text()),
            SecurityBadge::Internal => (glyph::INFO, THEME.text_secondary, badge.text()),
            SecurityBadge::PageCrashed => (glyph::INFO, THEME.iguana_fire, badge.text()),
        })
    }

    fn text_font(&self) -> HFONT {
        self.fonts.borrow().as_ref().map_or(0, |f| f.text.handle())
    }
}

impl Drop for UiState {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.edit_brush);
            for icon in self.icons.into_iter().filter(|&i| i != 0) {
                DestroyIcon(icon);
            }
        }
    }
}

unsafe fn ui_state<'a>(hwnd: HWND) -> Option<&'a UiState> {
    (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const UiState).as_ref()
}

fn toolbar_rect(state: &UiState, width: i32) -> RECT {
    RECT { left: 0, top: 0, right: width, bottom: state.layout.get().height }
}

unsafe fn invalidate_toolbar(hwnd: HWND, state: &UiState) {
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);
    InvalidateRect(hwnd, &toolbar_rect(state, rc.right), 0);
}

/// Recalcula la geometría para el ancho cliente y coloca el `EDIT`
unsafe fn apply_layout(hwnd: HWND, state: &UiState, client_width: i32) {
    let layout = ToolbarLayout::compute(client_width, state.dpi.get());
    state.layout.set(layout);
    let edit = GetDlgItem(hwnd, IDC_URL_EDIT as _);
    if edit != 0 {
        let e = layout.edit;
        MoveWindow(edit, e.left, e.top, e.width(), e.height(), 1);
    }
    InvalidateRect(hwnd, null_mut(), 0);
}

/// Ventana nativa Win32 anfitriona del Trusted Coordinator
pub struct NativeWindow {
    hwnd: HWND,
    url_edit_hwnd: HWND,
    /// HWND hijo donde renderiza el worker; 0 hasta `set_surface_hwnd`
    surface_hwnd: AtomicIsize,
    state: Box<UiState>,
}

// Envío seguro entre hilos del descriptor Win32
unsafe impl Send for NativeWindow {}
unsafe impl Sync for NativeWindow {}

impl NativeWindow {
    /// Registra la clase de ventana Win32 e inicializa la ventana principal.
    /// `width` y `height` son píxeles a 96 DPI; se escalan al DPI del monitor.
    pub fn new(title: &str, width: i32, height: i32) -> Result<Self, GalapagosError> {
        let class_name = to_wide("GalapagosTrustedWindow");
        let title_wide = to_wide(title);

        let hinstance = unsafe { GetModuleHandleW(null_mut()) };

        REGISTER_WINDOW_CLASS_ONCE.call_once(|| {
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(Self::wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: 0,
                hCursor: unsafe { LoadCursorW(0, IDC_ARROW) },
                // Δ8: fondo Volcanic Black (vive lo que la clase: todo el proceso)
                hbrBackground: unsafe { CreateSolidBrush(THEME.volcanic_black.colorref()) },
                lpszMenuName: null_mut(),
                lpszClassName: class_name.as_ptr(),
            };

            unsafe {
                RegisterClassW(&wc);
            };
        });

        // Crear ventana principal con estilo estándar WS_OVERLAPPEDWINDOW. R-2 (render incompleto):
        // WS_CLIPCHILDREN impide que el borrado de fondo y el pintado de esta ventana escriban sobre
        // la superficie hija donde presenta el worker (antes "se peleaban" por la misma zona).
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                title_wide.as_ptr(),
                MAIN_WINDOW_STYLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width,
                height,
                0,
                0,
                hinstance,
                null_mut(),
            )
        };

        if hwnd == 0 {
            return Err(GalapagosError::SandboxEnforcementFailed {
                win32_error_code: 1400, // Invalid window handle
                target_level: crate::galapagos_core_v1::WindowsSandboxLevel::TrustedCoordinator,
            });
        }

        // Δ8: tema, fuentes, logo, icono y barra oscura; `wnd_proc` lo lee desde aquí
        apply_dark_title_bar(hwnd);
        let state = Box::new(UiState::new(hwnd));
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*state as *const UiState as isize);
            for (kind, icon) in [(ICON_BIG, state.icons[0]), (ICON_SMALL, state.icons[1])] {
                if icon != 0 {
                    SendMessageW(hwnd, WM_SETICON, kind as usize, icon);
                }
            }
        }

        // Campo de dirección: EDIT sin borde dentro del rectángulo redondeado (D32: sin botón Navegar)
        let edit_class = to_wide("EDIT");
        let url_edit_hwnd = unsafe {
            CreateWindowExW(
                0,
                edit_class.as_ptr(),
                to_wide("about:blank").as_ptr(),
                WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL,
                0,
                0,
                0,
                0,
                hwnd,
                IDC_URL_EDIT as _,
                hinstance,
                null_mut(),
            )
        };
        let text_font = state.text_font();
        unsafe {
            if text_font != 0 {
                SendMessageW(url_edit_hwnd, WM_SETFONT, text_font as usize, 1);
            }
            let dpi = state.dpi.get();
            if dpi != 96 {
                // Provoca WM_SIZE, que coloca la barra con la geometría escalada
                SetWindowPos(hwnd, 0, 0, 0, scale(width, dpi), scale(height, dpi), SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rc);
            apply_layout(hwnd, &state, rc.right);
        }

        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);
        }

        // N-I5: temporizador que mantiene el pipe del worker servido sin bloquear el hilo de UI
        unsafe {
            SetTimer(hwnd, TIMER_ID_PUMP, PUMP_INTERVAL_MS, None);
        }

        Ok(Self {
            hwnd,
            url_edit_hwnd,
            surface_hwnd: AtomicIsize::new(0),
            state,
        })
    }

    /// Título de la ventana principal a partir del título de la página (ya saneado por el codec)
    pub fn set_title(&self, page_title: &str) {
        let full = if page_title.is_empty() {
            "Galapagos Browser".to_string()
        } else {
            format!("{} — Galapagos Browser", page_title)
        };
        let wide = to_wide(&full);
        unsafe {
            SetWindowTextW(self.hwnd, wide.as_ptr());
        }
    }

    /// Registra el HWND de la superficie del worker para reenviarle teclado y ratón
    pub fn set_surface_hwnd(&self, surface: isize) {
        self.surface_hwnd.store(surface, Ordering::Release);
    }

    /// N-I4: aplica la URL real del worker salvo que el usuario esté editando la barra
    pub fn set_url_from_engine(&self, url: &str) {
        url.clone_into(&mut self.state.engine_url.borrow_mut());
        if unsafe { GetFocus() } != self.url_edit_hwnd {
            self.set_url(url);
        }
    }

    /// Retorna el handle nativo HWND de la ventana anfitriona
    pub fn hwnd(&self) -> isize {
        self.hwnd as isize
    }

    /// Δ8: altura actual de la barra escalada al DPI del monitor; la superficie del worker empieza debajo
    pub fn toolbar_height(&self) -> i32 {
        self.state.layout.get().height
    }

    /// Actualiza el texto en la barra de direcciones
    pub fn set_url(&self, url: &str) {
        let wide = to_wide(url);
        unsafe {
            SetWindowTextW(self.url_edit_hwnd, wide.as_ptr());
        }
    }

    /// Obtiene la URL actualmente escrita en la barra de direcciones
    pub fn get_url(&self) -> String {
        unsafe {
            let len = GetWindowTextLengthW(self.url_edit_hwnd);
            if len == 0 {
                return String::new();
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            let read = GetWindowTextW(self.url_edit_hwnd, buf.as_mut_ptr(), len + 1);
            String::from_utf16_lossy(&buf[..read as usize])
        }
    }

    /// D33: insignia con el último veredicto real de la navegación
    pub fn set_security_badge(&self, badge: SecurityBadge) {
        self.state.badge.set(Some(badge));
        unsafe { invalidate_toolbar(self.hwnd, &self.state) };
    }

    /// R9: fallo del motor visible en la barra (tiene prioridad sobre la insignia)
    pub fn set_engine_fault(&self, fault: EngineFault) {
        self.state.engine_fault.set(Some(fault));
        // UI-I0: sin worker no hay historial que mostrar
        self.state.nav.set(None);
        unsafe { invalidate_toolbar(self.hwnd, &self.state) };
    }

    /// DELTA_08 paso 5: estado real del worker (`MSG_NAV_STATE`); se ignora tras un fallo del motor
    pub fn set_nav_state(&self, nav: NavState) {
        if self.state.engine_fault.get().is_some() || self.state.nav.get() == Some(nav) {
            return;
        }
        self.state.nav.set(Some(nav));
        unsafe { invalidate_toolbar(self.hwnd, &self.state) };
    }

    /// Si la vista nativa de galapagos://dashboard ocupa el lugar de la superficie
    pub fn is_native_dashboard_shown(&self) -> bool {
        DASHBOARD_ACTIVE.load(Ordering::Acquire)
    }

    /// Oculta el panel interno y vuelve a mostrar la página real del worker en la barra
    pub fn leave_native_dashboard(&self) {
        self.show_native_dashboard(false);
        let url = self.state.engine_url.borrow().clone();
        if !url.is_empty() {
            self.set_url(&url);
        }
    }

    /// Muestra u oculta la vista nativa de galapagos://dashboard
    pub fn show_native_dashboard(&self, show: bool) {
        DASHBOARD_ACTIVE.store(show, Ordering::Release);
        let surf = self.surface_hwnd.load(Ordering::Acquire);
        if surf != 0 {
            unsafe {
                ShowWindow(surf as HWND, if show { SW_HIDE } else { SW_SHOW });
            }
        }
        unsafe {
            InvalidateRect(self.hwnd, null_mut(), 1);
        }
    }

    /// Bucle de mensajes sin consumidor de eventos (comportamiento previo a DELTA_02)
    pub fn run_message_loop(&self) -> i32 {
        self.run_event_loop(|_| {})
    }

    /// Bucle de mensajes reactivo Win32 que entrega `UiEvent` al consumidor.
    /// Enter en la barra y teclado/ratón de la superficie se interceptan antes de `DispatchMessageW`.
    pub fn run_event_loop<F: FnMut(UiEvent)>(&self, mut on_event: F) -> i32 {
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            loop {
                let got = GetMessageW(&mut msg, 0, 0, 0);
                if got == 0 {
                    break; // WM_QUIT
                }
                if got < 0 {
                    crate::diagnostics::log_error(
                        crate::diagnostics::DiagnosticCategory::OsEvent,
                        "MESSAGE_LOOP_ERROR",
                        "GetMessageW devolvió -1; se cierra el bucle",
                        Some(windows_sys::Win32::Foundation::GetLastError()),
                    );
                    break;
                }

                let targets = LoopTargets {
                    main: self.hwnd,
                    url_edit: self.url_edit_hwnd,
                    surface: self.surface_hwnd.load(Ordering::Acquire),
                };
                match classify_loop_message(msg.message, msg.wParam, msg.hwnd, &targets) {
                    LoopAction::Navigate => on_event(UiEvent::NavigateRequested(AddressBarInput {
                        raw: self.get_url(),
                    })),
                    LoopAction::Toolbar(ToolbarButton::Home) => {
                        on_event(UiEvent::NavigateRequested(AddressBarInput { raw: HOME_URL.to_string() }))
                    }
                    // Solo con el botón habilitado por el último estado real del worker
                    LoopAction::Toolbar(button) if button.enabled_with(self.state.nav.get()) => match button {
                        ToolbarButton::Back => on_event(UiEvent::GoBack),
                        ToolbarButton::Forward => on_event(UiEvent::GoForward),
                        ToolbarButton::Reload => on_event(UiEvent::Reload),
                        ToolbarButton::Home => {}
                    },
                    LoopAction::Toolbar(_) => {}
                    LoopAction::SurfacePaint => {
                        // El worker presenta con ANGLE: aquí solo se valida la región y se le pide el frame
                        ValidateRect(targets.surface, null_mut());
                        on_event(UiEvent::SurfaceRepaint);
                    }
                    LoopAction::Tick => on_event(UiEvent::Tick),
                    LoopAction::Resize => {
                        // Minimizar (0x0) no genera evento (L-I5)
                        let (width, height) = unpack_client_size(msg.lParam);
                        if width > 0 && height > 0 {
                            on_event(UiEvent::Resize { width, height });
                        }
                    }
                    LoopAction::Surface => {
                        let mut lparam = msg.lParam;
                        let mut forward = true;
                        if msg.message == WM_MOUSEWHEEL {
                            // Convertir a coordenadas de la superficie; fuera de ella la rueda sigue su curso
                            let (sx, sy) = screen_point_from_lparam(msg.lParam);
                            let mut pt = POINT { x: sx, y: sy };
                            let mut rc: RECT = std::mem::zeroed();
                            ScreenToClient(targets.surface, &mut pt);
                            GetClientRect(targets.surface, &mut rc);
                            forward = pt.x >= 0 && pt.y >= 0 && pt.x < rc.right && pt.y < rc.bottom;
                            lparam = pack_client_lparam(pt.x, pt.y);
                        } else if msg.message == WM_LBUTTONDOWN {
                            SetFocus(msg.hwnd); // el teclado va a la superficie solo tras hacer clic en ella (N-I7)
                        }

                        if !forward {
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                            continue;
                        }
                        if let Some(ev) = InputDispatcher::translate_win32_message(msg.message, msg.wParam, lparam) {
                            on_event(UiEvent::SurfaceInput(ev));
                        }
                        if msg.message == WM_KEYDOWN {
                            TranslateMessage(&msg); // genera el WM_CHAR que se reenvía como texto
                        }
                    }
                    LoopAction::Pass => {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
            on_event(UiEvent::CloseRequested);
            msg.wParam as i32
        }
    }

    /// Procedimiento de ventana principal Win32
    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        const WM_DESTROY: u32 = 0x0002;
        const WM_SIZE: u32 = 0x0005;
        const WM_COMMAND: u32 = 0x0111;

        // El bucle propio redimensiona la superficie del worker (la UI no toca el motor)
        if msg == WM_SIZE {
            PostMessageW(hwnd, WM_APP_RESIZE, 0, lparam);
        }
        if msg == WM_DESTROY {
            PostQuitMessage(0);
            return 0;
        }
        // Mensajes de creación (antes de `GWLP_USERDATA`) o tras `Drop`: comportamiento por defecto
        let Some(state) = ui_state(hwnd) else {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        };
        let (x, y) = screen_point_from_lparam(lparam); // coordenadas cliente en los mensajes de ratón

        match msg {
            WM_COMMAND => {
                // P-02: sin log por WM_COMMAND (la barra emite EN_CHANGE/EN_UPDATE en cada pulsación)
                let control_id = (wparam & 0xFFFF) as isize;
                let notification = ((wparam >> 16) & 0xFFFF) as u32;
                if control_id == IDC_URL_EDIT && (notification == EN_SETFOCUS || notification == EN_KILLFOCUS) {
                    state.edit_focused.set(notification == EN_SETFOCUS);
                    invalidate_toolbar(hwnd, state);
                }
                0
            }
            WM_CTLCOLOREDIT => {
                let hdc = wparam as HDC;
                SetTextColor(hdc, THEME.text_primary.colorref());
                SetBkColor(hdc, THEME.lava_stone.colorref());
                state.edit_brush
            }
            WM_SIZE => {
                apply_layout(hwnd, state, (lparam & 0xFFFF) as i32);
                0
            }
            WM_DPICHANGED => {
                let dpi = (wparam & 0xFFFF) as u32;
                state.dpi.set(dpi);
                let fonts = UiFonts::for_dpi(dpi);
                if fonts.is_none() {
                    log_asset_unavailable("fuentes Segoe no disponibles");
                }
                // La fuente anterior sigue seleccionada en el EDIT hasta WM_SETFONT: se sustituye después
                let previous = state.fonts.replace(fonts);
                let edit = GetDlgItem(hwnd, IDC_URL_EDIT as _);
                SendMessageW(edit, WM_SETFONT, state.text_font() as usize, 1);
                drop(previous);
                let r = &*(lparam as *const RECT);
                SetWindowPos(hwnd, 0, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                let mut rc: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rc);
                apply_layout(hwnd, state, rc.right);
                0
            }
            WM_MOUSEMOVE => {
                if !state.tracking_leave.get() {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    state.tracking_leave.set(TrackMouseEvent(&mut tme) != 0);
                }
                let hover = state.layout.get().hit_test(x, y).filter(|b| b.enabled_with(state.nav.get()));
                if hover != state.hover.get() {
                    state.hover.set(hover);
                    invalidate_toolbar(hwnd, state);
                }
                0
            }
            WM_MOUSELEAVE => {
                state.tracking_leave.set(false);
                state.pressed.set(None);
                if state.hover.replace(None).is_some() {
                    invalidate_toolbar(hwnd, state);
                }
                0
            }
            WM_LBUTTONDOWN => {
                state.pressed.set(state.layout.get().hit_test(x, y).filter(|b| b.enabled_with(state.nav.get())));
                0
            }
            WM_LBUTTONUP => {
                let released = state.layout.get().hit_test(x, y);
                if let Some(button) = state.pressed.replace(None).filter(|b| Some(*b) == released) {
                    // N-I2: un único evento por clic; lo atiende el bucle propio
                    PostMessageW(hwnd, WM_APP_TOOLBAR, button.index(), 0);
                }
                0
            }
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);
                if hdc != 0 {
                    paint_window(hwnd, hdc, state);
                    EndPaint(hwnd, &ps);
                }
                0
            }
            // Todo el área cliente se pinta en WM_PAINT (sin parpadeo)
            WM_ERASEBKGND => 1,
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

const WM_PAINT: u32 = 0x000F;
const WM_ERASEBKGND: u32 = 0x0014;

unsafe fn fill_solid(hdc: HDC, rc: &RECT, color: Rgb) {
    let brush = CreateSolidBrush(color.colorref());
    FillRect(hdc, rc, brush);
    DeleteObject(brush);
}

unsafe fn draw_text(hdc: HDC, font: HFONT, text: &str, rc: Bounds, color: Rgb, format: u32) {
    let wide: Vec<u16> = text.encode_utf16().collect();
    let previous = (font != 0).then(|| SelectObject(hdc, font));
    SetTextColor(hdc, color.colorref());
    let mut r = rc.rect();
    DrawTextW(hdc, wide.as_ptr(), wide.len() as i32, &mut r, format);
    if let Some(previous) = previous {
        SelectObject(hdc, previous);
    }
}

/// Barra arriba; debajo, el panel nativo o el fondo (la superficie del worker la cubre al navegar)
unsafe fn paint_window(hwnd: HWND, hdc: HDC, state: &UiState) {
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);
    let body = RECT { top: state.layout.get().height, ..rc };
    if DASHBOARD_ACTIVE.load(Ordering::Acquire) {
        paint_galapagos_dashboard(hdc, body);
    } else {
        fill_solid(hdc, &body, THEME.volcanic_black);
    }
    paint_toolbar(hdc, rc.right, state);
}

/// Dibuja la barra en un mapa de bits intermedio y la copia de una vez (sin parpadeo al pasar el ratón)
unsafe fn paint_toolbar(target: HDC, width: i32, state: &UiState) {
    let layout = state.layout.get();
    let dpi = state.dpi.get();
    if width <= 0 || layout.height <= 0 {
        return;
    }
    let hdc = CreateCompatibleDC(target);
    let bitmap = CreateCompatibleBitmap(target, width, layout.height);
    let previous_bitmap = SelectObject(hdc, bitmap);

    let bar = toolbar_rect(state, width);
    fill_solid(hdc, &bar, THEME.deep_ocean);
    // DELTA_08 paso 5: con carga en curso (estado real del worker) la línea inferior pasa a teal
    let loading = state.nav.get().is_some_and(|n| n.loading);
    let line = if loading { (scale(2, dpi).max(2), THEME.pacific_teal) } else { (scale(1, dpi).max(1), THEME.stone_border) };
    fill_solid(hdc, &RECT { top: bar.bottom - line.0, ..bar }, line.1);

    let status = state.status();
    let address_radius = scale(8, dpi) as f32;

    // 1. Formas con GDI+ (suavizadas); sin GDI+ se usan rectángulos planos
    {
        let canvas = state.gdiplus.as_ref().and_then(|_| Canvas::new(hdc));
        let fill = |b: Bounds, radius: f32, color: Rgb| match &canvas {
            Some(c) if c.fill_round_rect(b.rect(), radius, color.argb(255)) => {}
            _ => fill_solid(hdc, &b.rect(), color),
        };
        if let (Some(c), Some(logo), Some(r)) = (&canvas, &state.logo, layout.logo) {
            c.draw_image(logo, r.rect());
        }
        if let Some(button) = state.hover.get() {
            fill(layout.buttons[button.index()], scale(6, dpi) as f32, THEME.lava_stone);
        }
        fill(layout.address, address_radius, THEME.lava_stone);
        if state.edit_focused.get() {
            if let Some(c) = &canvas {
                c.stroke_round_rect(layout.address.inset(1, 1).rect(), address_radius, THEME.pacific_teal.argb(255), scale(1, dpi).max(1) as f32);
            }
        }
        if status.is_some() {
            fill(layout.badge, (layout.badge.height() / 2) as f32, THEME.lava_stone);
        }
    } // el lienzo GDI+ se libera antes de dibujar texto con GDI

    // 2. Glifos y texto con GDI (ClearType)
    SetBkMode(hdc, 1 /* TRANSPARENT */);
    let fonts = state.fonts.borrow();
    let (icon_font, text_font) = fonts.as_ref().map_or((0, 0), |f| (f.icons.handle(), f.text.handle()));
    let centered = DT_CENTER | DT_VCENTER | DT_SINGLELINE;
    for button in ToolbarButton::ALL {
        let color = if button.enabled_with(state.nav.get()) { THEME.text_primary } else { THEME.stone_border };
        draw_text(hdc, icon_font, &button.glyph().to_string(), layout.buttons[button.index()], color, centered);
    }
    if let Some((icon, icon_color, text)) = status {
        let b = layout.badge;
        let icon_rect = Bounds { left: b.left + scale(8, dpi), right: b.left + scale(30, dpi), ..b };
        draw_text(hdc, icon_font, &icon.to_string(), icon_rect, icon_color, centered);
        let text_rect = Bounds { left: icon_rect.right + scale(4, dpi), right: b.right - scale(10, dpi), ..b };
        draw_text(hdc, text_font, text, text_rect, THEME.text_primary, DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    }
    drop(fonts);

    BitBlt(target, 0, 0, width, layout.height, hdc, 0, 0, SRCCOPY);
    SelectObject(hdc, previous_bitmap);
    DeleteObject(bitmap);
    DeleteDC(hdc);
}

/// T-02b (INC-20261025-CORE-DEBLOAT): solo afirmaciones ciertas en v0.1. El texto anterior
/// mostraba AppContainerStrict, «Proceso Confinado» y BlockAllP2P, que no se aplican.
const DASHBOARD_CARDS: [(&str, &str); 3] = [
    (
        "[ AISLAMIENTO ]",
        "• Worker de contenido: proceso separado\n• Job Object Win32: límite de memoria (CPU sin tope, en prueba)\n• Sandbox AppContainer: NO aplicado (pendiente)",
    ),
    (
        "[ PRIVACIDAD ]",
        "• Ocean Defense: solo navegación desde la barra\n• Filtro de rastreadores: no aplicado a subrecursos\n• WebRTC: sin control (pendiente)\n• Persistencia: solo en memoria",
    ),
    (
        "[ RENDIMIENTO ]",
        "• IPC: Named Pipe local, solo propietario\n• Vista interna: nativa GDI, sin WebView",
    ),
];

/// Renderiza la capa visual nativa de galapagos://dashboard en `body` (el área bajo la barra)
unsafe fn paint_galapagos_dashboard(hdc: HDC, body: RECT) {
    let rc = body;
    let top = body.top;

    // Fondo Volcanic Black #0B121A
    let bg_brush = CreateSolidBrush(rgb(11, 18, 26));
    FillRect(hdc, &rc, bg_brush);
    DeleteObject(bg_brush as _);

    SetBkMode(hdc, 1 /* TRANSPARENT */);

    // Encabezado Principal (Iguana Fire #E66A1F)
    let title = to_wide("GALAPAGOS BROWSER // PANEL DE CONTROL NATIVO");
    let mut header_rc = RECT {
        left: 24,
        top: top + 9,
        right: rc.right - 24,
        bottom: top + 34,
    };
    SetTextColor(hdc, rgb(230, 106, 31));
    DrawTextW(hdc, title.as_ptr(), (title.len() - 1) as i32, &mut header_rc, 0);

    // Subtítulo (Pacific Teal #1D9BB8)
    let sub = to_wide("Arquitectura Zero-Noise Vibe Engineering (ZNVE v2.4.2) | Vista nativa GDI");
    let mut sub_rc = RECT {
        left: 24,
        top: top + 36,
        right: rc.right - 24,
        bottom: top + 58,
    };
    SetTextColor(hdc, rgb(29, 155, 184));
    DrawTextW(hdc, sub.as_ptr(), (sub.len() - 1) as i32, &mut sub_rc, 0);

    // Dimensiones dinámicas de 3 tarjetas
    let total_w = rc.right - 48;
    let card_w = ((total_w - 32) / 3).max(180);
    let card_top = top + 69;
    let card_bottom = (rc.bottom - 70).max(card_top + 180);

    let card_bg = rgb(34, 43, 52); // Lava Stone #222B34
    let iguana_pen = rgb(230, 106, 31);
    let green_pen = rgb(67, 160, 71);
    let teal_pen = rgb(29, 155, 184);

    let draw_card = |left: i32, border_color: u32, title_text: &str, body_text: &str| {
        let card_rc = RECT {
            left,
            top: card_top,
            right: left + card_w,
            bottom: card_bottom,
        };
        let brush = CreateSolidBrush(card_bg);
        FillRect(hdc, &card_rc, brush);
        DeleteObject(brush as _);

        let border_brush = CreateSolidBrush(border_color);
        FrameRect(hdc, &card_rc, border_brush);
        DeleteObject(border_brush as _);

        // Cabecera de tarjeta
        let mut t_rc = RECT {
            left: left + 14,
            top: card_top + 14,
            right: left + card_w - 14,
            bottom: card_top + 38,
        };
        let t_wide = to_wide(title_text);
        SetTextColor(hdc, border_color);
        DrawTextW(hdc, t_wide.as_ptr(), (t_wide.len() - 1) as i32, &mut t_rc, 0);

        // Cuerpo de tarjeta
        let mut b_rc = RECT {
            left: left + 14,
            top: card_top + 44,
            right: left + card_w - 14,
            bottom: card_bottom - 12,
        };
        let b_wide = to_wide(body_text);
        SetTextColor(hdc, rgb(200, 210, 225));
        DrawTextW(hdc, b_wide.as_ptr(), (b_wide.len() - 1) as i32, &mut b_rc, 0x00000010 /* DT_WORDBREAK */);
    };

    for (i, ((card_title, card_body), pen)) in DASHBOARD_CARDS
        .iter()
        .zip([iguana_pen, green_pen, teal_pen])
        .enumerate()
    {
        draw_card(24 + (card_w + 16) * i as i32, pen, card_title, card_body);
    }

    // Banner Inferior
    let mut footer_rc = RECT {
        left: 24,
        top: card_bottom + 16,
        right: rc.right - 24,
        bottom: card_bottom + 42,
    };
    let footer_text = to_wide("Escriba una URL en la barra superior (ej: https://servo.org) o use 'galapagos://dashboard' para volver al panel.");
    SetTextColor(hdc, rgb(140, 160, 180));
    DrawTextW(hdc, footer_text.as_ptr(), (footer_text.len() - 1) as i32, &mut footer_rc, 0);
}

impl Drop for NativeWindow {
    fn drop(&mut self) {
        if self.hwnd != 0 {
            unsafe {
                KillTimer(self.hwnd, TIMER_ID_PUMP);
                // `state` se libera después de la ventana; `wnd_proc` deja de verlo antes
                SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                DestroyWindow(self.hwnd);
            }
            self.hwnd = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_window_instantiation_and_controls() {
        // En entorno Windows Desktop interactivo o headless, se valida la creación de la ventana
        let window = NativeWindow::new("Galapagos Browser - Test Window", 800, 600);
        assert!(window.is_ok(), "La ventana Win32 debe crearse exitosamente");

        let win = window.unwrap();
        assert_ne!(win.hwnd(), 0);

        // Validar actualización y lectura de la barra de direcciones
        win.set_url("https://servo.org");
        assert_eq!(win.get_url(), "https://servo.org");

        // Validar actualización de la insignia de Ocean Defense (D33)
        win.set_security_badge(SecurityBadge::Allowed);
        assert_eq!(win.state.badge.get(), Some(SecurityBadge::Allowed));
    }

    /// INC-20261025-CORE-DEBLOAT T-02b: el dashboard no anuncia protecciones que v0.1 no aplica
    #[test]
    fn dashboard_cards_make_no_false_claims_inc20261025() {
        for (title, body) in DASHBOARD_CARDS {
            for false_claim in ["AppContainerStrict", "Proceso Confinado", "BlockAllP2P", "100% Local", "Ring Buffer"] {
                assert!(!title.contains(false_claim) && !body.contains(false_claim), "{}", false_claim);
            }
        }
        assert!(DASHBOARD_CARDS[0].1.contains("NO aplicado"));
    }

    /// R-2 (render incompleto): la ventana real se crea con WS_CLIPCHILDREN
    #[test]
    fn main_window_clips_children_render_fix() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWL_STYLE};
        let win = NativeWindow::new("Galapagos Browser - Style Test", 800, 600).unwrap();
        let style = unsafe { GetWindowLongPtrW(win.hwnd, GWL_STYLE) } as u32;
        assert_ne!(style & WS_CLIPCHILDREN, 0, "estilo {:#x} sin WS_CLIPCHILDREN", style);
        assert_ne!(style & WS_OVERLAPPEDWINDOW, 0);
    }

    /// Δ8: la ventana real tiene la barra de título oscura (DWM), icono grande y pequeño, y un
    /// campo de dirección sin borde
    #[test]
    fn main_window_dark_title_icon_and_borderless_edit_delta08() {
        use windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute;
        use windows_sys::Win32::UI::WindowsAndMessaging::{GWL_STYLE, WM_GETICON, WS_BORDER};
        let win = NativeWindow::new("Galapagos Browser - DWM Test", 800, 600).unwrap();

        let mut dark: i32 = 0;
        let hr = unsafe {
            DwmGetWindowAttribute(win.hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, &mut dark as *mut i32 as *mut _, 4)
        };
        assert!(hr >= 0 && dark == 1, "hr {:#x} · modo oscuro {}", hr, dark);

        for kind in [ICON_BIG, ICON_SMALL] {
            let icon = unsafe { SendMessageW(win.hwnd, WM_GETICON, kind as usize, 0) };
            assert_ne!(icon, 0, "sin icono {}", kind);
        }

        let edit_style = unsafe { GetWindowLongPtrW(win.url_edit_hwnd, GWL_STYLE) } as u32;
        assert_eq!(edit_style & WS_BORDER, 0, "estilo del EDIT {:#x} con borde", edit_style);
    }

    /// D32: no existe el botón «Navegar»; el único hijo de la barra es el EDIT
    #[test]
    fn navigate_button_removed_d32() {
        use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowExW;
        let win = NativeWindow::new("Galapagos Browser - D32 Test", 800, 600).unwrap();
        let button_class = to_wide("BUTTON");
        assert_eq!(unsafe { FindWindowExW(win.hwnd, 0, button_class.as_ptr(), std::ptr::null()) }, 0);
        assert_eq!(unsafe { GetDlgItem(win.hwnd, 1002) }, 0, "el control 1002 (Navegar) sigue existiendo");
        assert_eq!(unsafe { GetDlgItem(win.hwnd, IDC_URL_EDIT as _) }, win.url_edit_hwnd);
    }

    /// D33 / UI-I0: cada texto de la insignia sale de un estado real; ninguno afirma «seguro»
    #[test]
    fn badge_texts_come_from_measured_states_d33() {
        let reasons = [
            BlockReason::Threat,
            BlockReason::ConfirmationRequired,
            BlockReason::VerificationUnavailable,
            BlockReason::CoordinatorDenied,
            BlockReason::InvalidDestination,
            BlockReason::InvalidAddress,
        ];
        let mut texts: Vec<&str> = reasons.iter().map(|r| SecurityBadge::Blocked(*r).text()).collect();
        texts.extend([SecurityBadge::Allowed.text(), SecurityBadge::Internal.text()]);
        texts.extend([EngineFault::Unavailable, EngineFault::WorkerFault, EngineFault::WorkerTerminated].map(EngineFault::text));
        for text in &texts {
            let lower = text.to_lowercase();
            assert!(!text.is_empty());
            for claim in ["seguro", "protección activa", "protegido"] {
                assert!(!lower.contains(claim), "«{}» contiene «{}»", text, claim);
            }
        }
        let mut unique = texts.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), texts.len(), "cada estado tiene su propio texto (motivo legible)");

        // Ventana real: sin navegación no hay insignia; el fallo del motor tiene prioridad
        let win = NativeWindow::new("Galapagos Browser - Badge Test", 800, 600).unwrap();
        assert_eq!(win.state.status(), None);
        win.set_security_badge(SecurityBadge::Blocked(BlockReason::Threat));
        assert_eq!(win.state.status().map(|s| s.2), Some("Ocean Defense: bloqueado"));
        win.set_engine_fault(EngineFault::WorkerTerminated);
        assert_eq!(win.state.status().map(|s| s.2), Some("Worker terminado"));
    }

    /// Δ8: geometría de la barra a 96, 144 y 192 DPI (lógica pura)
    #[test]
    fn toolbar_layout_scales_with_dpi_delta08() {
        let base = ToolbarLayout::compute(1024, 96);
        assert_eq!(base.height, TOOLBAR_HEIGHT_96);
        for dpi in [96u32, 144, 192] {
            let width = scale(1024, dpi);
            let l = ToolbarLayout::compute(width, dpi);
            assert_eq!(l.height, scale(TOOLBAR_HEIGHT_96, dpi), "dpi {}", dpi);
            assert!(l.logo.is_some(), "dpi {}: sin logo a 1024 px lógicos", dpi);

            let inside_bar = |b: &Bounds| b.top >= 0 && b.bottom <= l.height && b.left >= 0 && b.right <= width;
            for (i, b) in l.buttons.iter().enumerate() {
                assert!(inside_bar(b), "dpi {}: botón {} fuera de la barra", dpi, i);
                if i > 0 {
                    assert!(l.buttons[i - 1].right <= b.left, "dpi {}: botones solapados", dpi);
                }
            }
            assert!(l.logo.unwrap().right <= l.buttons[0].left);
            assert!(l.buttons[3].right <= l.address.left);
            assert!(l.address.right <= l.badge.left && inside_bar(&l.badge) && inside_bar(&l.address));
            let e = l.edit;
            assert!(e.left > l.address.left && e.right < l.address.right && e.top >= l.address.top && e.bottom <= l.address.bottom);

            // Proporcional al DPI (±1 px de redondeo)
            let ratio = dpi as f64 / 96.0;
            let near = |scaled: i32, base: i32| ((scaled as f64) - (base as f64) * ratio).abs() <= 1.0;
            for (s, b) in [(l.address, base.address), (l.edit, base.edit), (l.badge, base.badge), (l.buttons[3], base.buttons[3])] {
                assert!(near(s.left, b.left) && near(s.right, b.right) && near(s.top, b.top) && near(s.bottom, b.bottom), "dpi {}: {:?} vs {:?}", dpi, s, b);
            }
        }
        // Ventana estrecha: sin logo y la insignia no invade el campo
        let narrow = ToolbarLayout::compute(500, 96);
        assert!(narrow.logo.is_none());
        assert!(narrow.address.right <= narrow.badge.left);
    }

    #[test]
    fn toolbar_hit_test_and_enabled_buttons_delta08() {
        let l = ToolbarLayout::compute(1024, 96);
        for button in ToolbarButton::ALL {
            let b = l.buttons[button.index()];
            assert_eq!(l.hit_test(b.left + b.width() / 2, b.top + b.height() / 2), Some(button));
            assert_eq!(ToolbarButton::from_index(button.index()), Some(button));
        }
        assert_eq!(l.hit_test(l.address.left + 5, l.address.top + 5), None);
        assert_eq!(ToolbarButton::from_index(4), None);
        // UI-I0: sin MSG_NAV_STATE (paso 5) solo Inicio está habilitado
        let enabled: Vec<_> = ToolbarButton::ALL.into_iter().filter(|b| b.enabled()).collect();
        assert_eq!(enabled, vec![ToolbarButton::Home]);
    }

    #[test]
    fn page_crash_badge_is_not_sticky_delta11() {
        assert_eq!(SecurityBadge::PageCrashed.text(), "La página falló");
        let win = NativeWindow::new("t", 800, 600).expect("ventana");
        win.set_nav_state(NavState { can_back: true, can_forward: false, loading: false });
        win.set_security_badge(SecurityBadge::PageCrashed);
        assert_eq!(win.state.status().map(|s| s.2), Some("La página falló"));
        // Una página caída no es un fallo del motor: el estado de navegación sigue vivo
        assert_eq!(win.state.engine_fault.get(), None);
        assert!(win.state.nav.get().is_some());
        // La siguiente navegación la sustituye
        win.set_security_badge(SecurityBadge::Allowed);
        assert_eq!(win.state.status().map(|s| s.2), Some("Ocean Defense: permitido"));
    }

    #[test]
    fn buttons_follow_measured_nav_state_delta08_step5() {
        let on = |nav| ToolbarButton::ALL.into_iter().filter(|b| b.enabled_with(nav)).collect::<Vec<_>>();
        use ToolbarButton::*;
        assert_eq!(on(None), vec![Home]);
        let none = NavState { can_back: false, can_forward: false, loading: true };
        assert_eq!(on(Some(none)), vec![Reload, Home]);
        assert_eq!(on(Some(NavState { can_back: true, ..none })), vec![Back, Reload, Home]);
        assert_eq!(on(Some(NavState { can_back: true, can_forward: true, loading: false })), vec![Back, Forward, Reload, Home]);
        // R-3: WM_PAINT solo de la superficie
        let t = targets();
        assert_eq!(classify_loop_message(WM_PAINT, 0, 30, &t), LoopAction::SurfacePaint);
        assert_eq!(classify_loop_message(WM_PAINT, 0, 10, &t), LoopAction::Pass);
        assert_eq!(classify_loop_message(WM_PAINT, 0, 30, &LoopTargets { surface: 0, ..t }), LoopAction::Pass);
    }

    fn targets() -> LoopTargets {
        LoopTargets { main: 10, url_edit: 20, surface: 30 }
    }

    #[test]
    fn loop_message_classification_delta02() {
        let t = targets();
        let enter = VK_RETURN as usize;
        // N-I2: Enter solo navega dentro de la barra
        assert_eq!(classify_loop_message(WM_KEYDOWN, enter, 20, &t), LoopAction::Navigate);
        assert_eq!(classify_loop_message(WM_KEYDOWN, enter, 30, &t), LoopAction::Surface);
        assert_eq!(classify_loop_message(WM_KEYDOWN, 0x41, 20, &t), LoopAction::Pass);
        assert_eq!(classify_loop_message(WM_KEYUP, enter, 20, &t), LoopAction::Pass);
        // Δ8: botones de la barra re-publicados (D32 retiró WM_APP_NAVIGATE) y temporizador
        assert_eq!(classify_loop_message(WM_APP_TOOLBAR, 3, 10, &t), LoopAction::Toolbar(ToolbarButton::Home));
        assert_eq!(classify_loop_message(WM_APP_TOOLBAR, 0, 10, &t), LoopAction::Toolbar(ToolbarButton::Back));
        assert_eq!(classify_loop_message(WM_APP_TOOLBAR, 9, 10, &t), LoopAction::Pass);
        assert_eq!(classify_loop_message(WM_APP_TOOLBAR, 3, 30, &t), LoopAction::Pass);
        assert_eq!(classify_loop_message(WM_APP_RESIZE, 0, 10, &t), LoopAction::Resize);
        assert_eq!(classify_loop_message(WM_APP_RESIZE, 0, 30, &t), LoopAction::Pass);
        assert_eq!(classify_loop_message(WM_TIMER, TIMER_ID_PUMP, 10, &t), LoopAction::Tick);
        assert_eq!(classify_loop_message(WM_TIMER, 99, 10, &t), LoopAction::Pass);
        // N-I7: entrada de superficie solo sobre el HWND de superficie
        for m in [WM_CHAR, WM_MOUSEMOVE, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_RBUTTONDOWN, WM_MBUTTONUP] {
            assert_eq!(classify_loop_message(m, 0, 30, &t), LoopAction::Surface, "msg {:#x}", m);
            assert_eq!(classify_loop_message(m, 0, 20, &t), LoopAction::Pass, "msg {:#x}", m);
        }
        assert_eq!(classify_loop_message(WM_MOUSEWHEEL, 0, 20, &t), LoopAction::Surface);
        // Sin superficie registrada nada se reenvía
        let none = LoopTargets { surface: 0, ..t };
        assert_eq!(classify_loop_message(WM_MOUSEWHEEL, 0, 20, &none), LoopAction::Pass);
        assert_eq!(classify_loop_message(WM_CHAR, 0, 0, &none), LoopAction::Pass);
    }

    #[test]
    fn client_size_unpacking_delta04() {
        assert_eq!(unpack_client_size((768 << 16) | 1024), (1024, 768));
        assert_eq!(unpack_client_size(0), (0, 0));
    }

    #[test]
    fn lparam_point_roundtrip_with_negative_coords_delta02() {
        for (x, y) in [(0, 0), (120, 340), (-5, 7), (7, -5), (-32768, 32767)] {
            assert_eq!(screen_point_from_lparam(pack_client_lparam(x, y)), (x, y));
        }
    }

    /// Mensajes Win32 reales: Enter en la barra produce exactamente un `NavigateRequested` con el
    /// texto de la barra; una letra en la barra se teclea en ella y no genera evento propio. Un clic
    /// real en Inicio produce uno hacia `HOME_URL`; un clic en Atrás (deshabilitado) no produce ninguno.
    #[test]
    fn enter_and_home_click_each_emit_one_navigate_delta02() {
        use windows_sys::Win32::System::Threading::GetCurrentThreadId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};

        let win = NativeWindow::new("Galapagos Browser - Loop Test", 800, 600).unwrap();
        win.set_url("servo.org");
        let layout = win.state.layout.get();
        let click = |button: ToolbarButton| {
            let b = layout.buttons[button.index()];
            let at = pack_client_lparam(b.left + b.width() / 2, b.top + b.height() / 2);
            unsafe {
                PostMessageW(win.hwnd, WM_LBUTTONDOWN, 0, at);
                PostMessageW(win.hwnd, WM_LBUTTONUP, 0, at);
            }
        };
        unsafe {
            PostMessageW(win.url_edit_hwnd, WM_KEYDOWN, VK_RETURN as usize, 0);
            PostMessageW(win.url_edit_hwnd, WM_KEYDOWN, 0x41, 0);
        }
        click(ToolbarButton::Back);
        click(ToolbarButton::Home);

        let mut events = Vec::new();
        let mut navigates = 0;
        let mut ticks = 0;
        win.run_event_loop(|ev| {
            match &ev {
                UiEvent::NavigateRequested(_) => navigates += 1,
                UiEvent::Tick => ticks += 1,
                _ => {}
            }
            events.push(ev);
            // Salida: tras los dos Navigate (o, como cota de seguridad, ~3 s de temporizador)
            if navigates == 2 || ticks > 200 {
                unsafe { PostThreadMessageW(GetCurrentThreadId(), WM_QUIT, 0, 0) };
            }
        });

        let navs: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, UiEvent::NavigateRequested(_)))
            .collect();
        let nav = |raw: &str| UiEvent::NavigateRequested(AddressBarInput { raw: raw.to_string() });
        assert_eq!(navs, vec![&nav("servo.org"), &nav(HOME_URL)]);
        assert_eq!(events.last(), Some(&UiEvent::CloseRequested));
        assert_eq!(win.get_url(), "aservo.org");
    }
}
