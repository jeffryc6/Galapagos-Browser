//! src/engine/mod.rs
//! Adaptador del Motor Web de Renderizado (WebEnginePort) para Galapagos Browser v0.1
//! Conforme a contracts/galapagos_core_v1.rs, contracts/DELTA_01_engine_worker.md, ARC-01, SEC-01 y estándar ZNVE v2.4.0.
//! Conecta la superficie de ventana Win32 (HWND) con el ciclo de vida del worker enjaulado (H1–H5)
//! y la traducción de entrada Win32 que viaja al worker por IPC.
//! R-02 (INC-20261025-CORE-DEBLOAT): retirado `servo_ffi.rs`, un Servo simulado que entraba en
//! producción por el feature por omisión `engine-stub`.

pub mod input_dispatcher;

pub use input_dispatcher::{InputDispatcher, InputEvent, KeyModifiers, MouseButton};

use crate::galapagos_core_v1::{
    ContentToCoordinatorIpc, EngineBackend, GalapagosError, ProcessId, ResourceQuota, TabId, WebEnginePort,
    WindowsSandboxLevel, WorkerHandshake,
};
use crate::ipc::channel::RestrictedNamedPipeServer;
use crate::islands::security::JobObjectEnclosure;

use std::collections::HashMap;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, ResumeThread, TerminateProcess, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    PROCESS_INFORMATION, STARTUPINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, MoveWindow, RegisterClassW, ShowWindow,
    WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_VISIBLE,
};

static REGISTER_VIEWPORT_CLASS_ONCE: Once = Once::new();

/// INC-20261026-WORKER-CONSOLE: el worker es de subsistema consola y el Coordinador no tiene
/// consola. Sin `CREATE_NO_WINDOW`, Windows le crea una consola visible; con Windows Terminal como
/// terminal predeterminada el traspaso falla (0x800700E8) y el worker se bloquea al escribir en
/// stdout: ninguna página carga. Con la consola oculta no hay traspaso.
const WORKER_CREATION_FLAGS: u32 = CREATE_SUSPENDED | CREATE_NO_WINDOW;

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Evalúa si el nivel de sandbox realmente aplicado cumple con el solicitado (Invariante H2)
pub fn is_sandbox_sufficient(applied: WindowsSandboxLevel, requested: WindowsSandboxLevel) -> bool {
    let rank = |lvl| match lvl {
        WindowsSandboxLevel::TrustedCoordinator => 0,
        WindowsSandboxLevel::RestrictedService => 1,
        WindowsSandboxLevel::AppContainerStrict => 2,
    };
    rank(applied) >= rank(requested)
}

/// Rutas donde puede estar el worker. T-05 (INC-20261025-CORE-DEBLOAT): todas cuelgan del
/// ejecutable, nunca del directorio de trabajo (que un tercero puede controlar). En release el
/// worker debe estar junto al Coordinador; en debug se admite el `target` del workspace, buscado
/// en los ancestros del ejecutable (`target/debug/deps` en los tests).
fn worker_candidates() -> Vec<std::path::PathBuf> {
    let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|p| p.to_path_buf())) else {
        return Vec::new();
    };
    let mut candidates = vec![dir.join("galapagos-content.exe")];
    if cfg!(debug_assertions) {
        for ancestor in dir.ancestors().take(4) {
            for profile in ["debug", "release"] {
                candidates.push(
                    ancestor.join("crates").join("galapagos-content").join("target").join(profile).join("galapagos-content.exe"),
                );
            }
        }
    }
    candidates
}

/// DELTA_04 L-I7: entre los candidatos existentes, el de modificación más reciente
/// (evita lanzar un worker obsoleto que quedó en otra carpeta de `target`).
fn pick_newest_existing(candidates: Vec<std::path::PathBuf>) -> Option<std::path::PathBuf> {
    candidates
        .into_iter()
        .filter_map(|p| {
            let modified = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            Some((modified, p))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, p)| p)
}

fn find_worker_binary() -> Result<std::path::PathBuf, GalapagosError> {
    match pick_newest_existing(worker_candidates()) {
        Some(path) => absolute_worker_path(&path),
        None => Err(GalapagosError::SandboxEnforcementFailed {
            win32_error_code: 2, // ERROR_FILE_NOT_FOUND
            target_level: WindowsSandboxLevel::AppContainerStrict,
        }),
    }
}

/// CreateProcessW falla con ERROR_FILE_NOT_FOUND ante rutas relativas con '/'; el worker se lanza
/// siempre con ruta absoluta. `absolute` (no `canonicalize`) evita el prefijo `\\?\` en la línea de comandos.
fn absolute_worker_path(path: &std::path::Path) -> Result<std::path::PathBuf, GalapagosError> {
    std::path::absolute(path).map_err(|e| GalapagosError::SandboxEnforcementFailed {
        win32_error_code: e.raw_os_error().unwrap_or(2) as u32,
        target_level: WindowsSandboxLevel::AppContainerStrict,
    })
}

/// Resultado de `poll_content_events`: eventos válidos y, si lo hubo, la violación que cortó la lectura
#[derive(Debug, Default)]
pub struct PolledContentEvents {
    pub events: Vec<ContentToCoordinatorIpc>,
    pub violation: Option<GalapagosError>,
}

/// `tab_id` declarado por el contenido en cada evento (N-I8: debe coincidir con el del canal)
pub(crate) fn content_event_tab(event: &ContentToCoordinatorIpc) -> TabId {
    match event {
        ContentToCoordinatorIpc::FrameReady { tab_id, .. }
        | ContentToCoordinatorIpc::RequestNetworkFetch { tab_id, .. }
        | ContentToCoordinatorIpc::RequestCapability { tab_id, .. }
        | ContentToCoordinatorIpc::UrlChanged { tab_id, .. }
        | ContentToCoordinatorIpc::TitleChanged { tab_id, .. }
        | ContentToCoordinatorIpc::WorkerFault { tab_id, .. }
        | ContentToCoordinatorIpc::NavState { tab_id, .. }
        | ContentToCoordinatorIpc::ContentCrashed { tab_id, .. } => *tab_id,
        ContentToCoordinatorIpc::TelemetrySample(sample) => sample.tab_id,
    }
}

/// DELTA_08 §2.2: órdenes Coordinador → worker sin payload (tipos 14–17)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabCommand {
    GoBack,
    GoForward,
    Reload,
    Repaint,
}

impl TabCommand {
    pub fn msg_type(self) -> u16 {
        match self {
            Self::GoBack => galapagos_ipc::MSG_GO_BACK,
            Self::GoForward => galapagos_ipc::MSG_GO_FORWARD,
            Self::Reload => galapagos_ipc::MSG_RELOAD,
            Self::Repaint => galapagos_ipc::MSG_REPAINT,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::GoBack => "atrás",
            Self::GoForward => "adelante",
            Self::Reload => "recarga",
            Self::Repaint => "repintado",
        }
    }
}

/// Tipo de mensaje IPC Coordinador -> Worker para eventos de entrada
const MSG_TYPE_INPUT_EVENT: u16 = galapagos_ipc::MSG_INPUT_EVENT;

fn mouse_button_byte(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// Serializa un `InputEvent` al payload del tipo 12 que decodifica `galapagos-content/src/input.rs`.
/// Las coordenadas viajan con signo (i32 BE). `Resize` no usa este mensaje: devuelve `None`.
pub(crate) fn encode_input_event(event: &InputEvent) -> Option<Vec<u8>> {
    let mut p = Vec::with_capacity(18);
    match *event {
        InputEvent::KeyDown { key_code, modifiers } | InputEvent::KeyUp { key_code, modifiers } => {
            p.push(if matches!(event, InputEvent::KeyDown { .. }) { 1 } else { 2 });
            p.extend_from_slice(&key_code.to_be_bytes());
            p.push(modifiers.to_bitfield() as u8);
        }
        InputEvent::MouseDown { button, x, y, modifiers }
        | InputEvent::MouseUp { button, x, y, modifiers } => {
            p.push(if matches!(event, InputEvent::MouseDown { .. }) { 3 } else { 4 });
            p.push(mouse_button_byte(button));
            p.extend_from_slice(&x.to_be_bytes());
            p.extend_from_slice(&y.to_be_bytes());
            p.push(modifiers.to_bitfield() as u8);
        }
        InputEvent::MouseMove { x, y, .. } => {
            p.push(5);
            p.extend_from_slice(&x.to_be_bytes());
            p.extend_from_slice(&y.to_be_bytes());
        }
        InputEvent::MouseWheel { delta_x, delta_y, x, y, .. } => {
            p.push(6);
            p.extend_from_slice(&delta_x.to_be_bytes());
            p.extend_from_slice(&delta_y.to_be_bytes());
            p.extend_from_slice(&x.to_be_bytes());
            p.extend_from_slice(&y.to_be_bytes());
        }
        InputEvent::Char { scalar } => {
            p.push(7);
            p.extend_from_slice(&(scalar as u32).to_be_bytes());
        }
        InputEvent::Resize { .. } => return None,
    }
    Some(p)
}

/// H-01 (INC-20261025-CORE-DEBLOAT): un fallo de envío al worker se registra una vez hasta el
/// siguiente envío correcto (ráfagas de MouseMove o WM_SIZE con el worker caído no inundan el log)
fn report_send_failure(tab_id: TabId, event_name: &'static str, what: &str, res: Result<(), GalapagosError>) {
    static SEND_FAILURE_REPORTED: AtomicBool = AtomicBool::new(false);
    match res {
        Ok(()) => SEND_FAILURE_REPORTED.store(false, Ordering::Relaxed),
        Err(e) => {
            if !SEND_FAILURE_REPORTED.swap(true, Ordering::Relaxed) {
                crate::diagnostics::log_error(
                    crate::diagnostics::DiagnosticCategory::Ipc,
                    event_name,
                    format!("Fallo al enviar {} a la pestaña {:?}: {}", what, tab_id, e),
                    None,
                );
            }
        }
    }
}

/// Guardia RAII del proceso hijo (Invariante H5): si el arranque falla antes de transferirlo,
/// el proceso se termina y sus handles se cierran al salir de ámbito.
struct ChildProcessGuard {
    h_process: HANDLE,
    h_thread: HANDLE,
}

impl ChildProcessGuard {
    fn close_thread(&mut self) {
        if self.h_thread != 0 {
            unsafe {
                CloseHandle(self.h_thread);
            }
            self.h_thread = 0;
        }
    }

    fn into_process_handle(mut self) -> HANDLE {
        self.close_thread();
        std::mem::replace(&mut self.h_process, 0)
    }
}

impl Drop for ChildProcessGuard {
    fn drop(&mut self) {
        self.close_thread();
        if self.h_process != 0 {
            unsafe {
                TerminateProcess(self.h_process, 1);
                CloseHandle(self.h_process);
            }
        }
    }
}

/// R-16: destruye el HWND de superficie si el arranque falla antes de transferirlo al `WorkerInstance`
struct SurfaceGuard(HWND);

impl SurfaceGuard {
    fn into_inner(mut self) -> HWND {
        std::mem::replace(&mut self.0, 0)
    }
}

impl Drop for SurfaceGuard {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }
}

/// T-03/T-02 (INC-20261025-CORE-DEBLOAT): el cliente del pipe debe ser el proceso hijo lanzado,
/// según el PID que reporta el kernel, antes de aceptar cualquier trama suya
fn verify_worker_client(pipe: &RestrictedNamedPipeServer, tab_id: TabId, expected_pid: u32) -> Result<(), GalapagosError> {
    if pipe.client_process_id()? != expected_pid {
        return Err(GalapagosError::IpcBoundaryViolation {
            tab_id,
            reason: crate::galapagos_core_v1::IpcViolationReason::UnauthorizedCommand,
        });
    }
    Ok(())
}

/// Estado inmutable de un proceso worker de renderizado
pub struct WorkerInstance {
    pub process_id: ProcessId,
    pub sandbox: WindowsSandboxLevel,
    pub quota: ResourceQuota,
    pub surface_hwnd: HWND,
    pub current_url: String,
    pub handshake: WorkerHandshake,
    pub process_handle: Option<HANDLE>,
    /// Binario con el que se lanzó el worker (vacío en el backend Stub)
    pub binary: std::path::PathBuf,
    pub job_enclosure: Option<JobObjectEnclosure>,
    pub pipe_server: Option<RestrictedNamedPipeServer>,
}

impl Drop for WorkerInstance {
    fn drop(&mut self) {

        if let Some(h_proc) = self.process_handle.take() {
            unsafe {
                TerminateProcess(h_proc, 0);
                CloseHandle(h_proc);
            }
        }
        drop(self.job_enclosure.take());
        drop(self.pipe_server.take());

        if self.surface_hwnd != 0 {
            unsafe {
                DestroyWindow(self.surface_hwnd);
            }
            self.surface_hwnd = 0;
        }
    }
}

/// Adaptador del motor web en el Trusted Coordinator
pub struct ServoEngineAdapter {
    parent_hwnd: HWND,
    workers: HashMap<TabId, WorkerInstance>,
    backend: EngineBackend,
    /// Δ8: la superficie empieza bajo la barra de la ventana (altura escalada por DPI)
    toolbar_height: i32,
}

unsafe impl Send for ServoEngineAdapter {}
unsafe impl Sync for ServoEngineAdapter {}

impl ServoEngineAdapter {
    /// Inicializa el adaptador vinculado a la ventana anfitriona Win32.
    /// DELTA_04 L-I4: fuera de las pruebas unitarias el backend es siempre `Real`; si falta el
    /// worker, `initialize_sandboxed_worker` falla de forma explícita (nunca se degrada a `Stub`).
    pub fn new(parent_hwnd: isize) -> Result<Self, GalapagosError> {
        let backend = if cfg!(test) { EngineBackend::Stub } else { EngineBackend::Real };
        Self::with_backend(parent_hwnd, backend)
    }

    /// Inicializa el adaptador con selección explícita del backend (Real o Stub)
    pub fn with_backend(parent_hwnd: isize, backend: EngineBackend) -> Result<Self, GalapagosError> {
        let parent = parent_hwnd as HWND;
        let class_name = to_wide("GalapagosRenderViewport");
        let hinstance = unsafe { GetModuleHandleW(null_mut()) };

        REGISTER_VIEWPORT_CLASS_ONCE.call_once(|| {
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(DefWindowProcW),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: 0,
                hCursor: 0,
                hbrBackground: 0, // Fondo transparente para buffer gráfico del motor
                lpszMenuName: null_mut(),
                lpszClassName: class_name.as_ptr(),
            };

            unsafe {
                RegisterClassW(&wc);
            };
        });

        Ok(Self {
            parent_hwnd: parent,
            workers: HashMap::new(),
            backend,
            toolbar_height: crate::window::TOOLBAR_HEIGHT_96,
        })
    }

    /// Δ8: altura real de la barra (`NativeWindow::toolbar_height`); se aplica al crear la
    /// superficie y en cada `resize_viewport`
    pub fn set_toolbar_height(&mut self, px: i32) {
        self.toolbar_height = px.max(0);
    }

    /// Retorna el backend configurado (Real o Stub)
    pub fn backend(&self) -> EngineBackend {
        self.backend
    }

    /// Retorna el nivel de sandbox realmente aplicado recibido en el handshake (Invariante H6)
    pub fn applied_sandbox_for_tab(&self, tab_id: TabId) -> Option<WindowsSandboxLevel> {
        self.workers.get(&tab_id).map(|w| w.handshake.applied_sandbox)
    }

    /// Retorna una referencia al handshake del worker de una pestaña
    pub fn handshake_for_tab(&self, tab_id: TabId) -> Option<&WorkerHandshake> {
        self.workers.get(&tab_id).map(|w| &w.handshake)
    }

    /// DELTA_04 L-I1: código de salida del worker si ya terminó (`GetExitCodeProcess`);
    /// `None` si sigue vivo o no hay proceso (backend Stub / pestaña desconocida).
    pub fn worker_exit_code(&self, tab_id: TabId) -> Option<u32> {
        const STILL_ACTIVE: u32 = 259;
        let handle = self.workers.get(&tab_id)?.process_handle?;
        let mut code: u32 = 0;
        let ok = unsafe { windows_sys::Win32::System::Threading::GetExitCodeProcess(handle, &mut code) };
        (ok != 0 && code != STILL_ACTIVE).then_some(code)
    }

    /// DELTA_04 L-I2: binario con el que se lanzó el worker de la pestaña
    pub fn worker_binary(&self, tab_id: TabId) -> Option<&std::path::Path> {
        self.workers.get(&tab_id).map(|w| w.binary.as_path()).filter(|p| !p.as_os_str().is_empty())
    }

    /// Retorna el HWND de superficie de renderizado asociado a una pestaña
    pub fn surface_for_tab(&self, tab_id: TabId) -> Option<isize> {
        self.workers.get(&tab_id).map(|w| w.surface_hwnd as isize)
    }

    /// Retorna el número de workers activos
    pub fn active_worker_count(&self) -> usize {
        self.workers.len()
    }

    /// Retorna la URL reportada por la pestaña
    pub fn servo_url_for_tab(&self, tab_id: TabId) -> Option<String> {
        self.workers.get(&tab_id).map(|w| w.current_url.clone())
    }

    /// Despacha un evento de entrada tipado hacia la instancia del worker
    pub fn dispatch_input(&mut self, tab_id: TabId, event: InputEvent) -> Result<(), GalapagosError> {
        let worker = self.workers.get_mut(&tab_id).ok_or(GalapagosError::IpcBoundaryViolation {
            tab_id,
            reason: crate::galapagos_core_v1::IpcViolationReason::MalformedMessage,
        })?;

        if let Some(ref pipe_server) = worker.pipe_server {
            // Resize viaja por `resize_viewport` (MSG_TYPE_RESIZE), no por MSG_TYPE_INPUT_EVENT
            if let Some(payload) = encode_input_event(&event) {
                return pipe_server.send_frame(MSG_TYPE_INPUT_EVENT, &payload);
            }
        }
        Ok(())
    }

    /// Lee sin bloquear hasta `max` mensajes del worker y los decodifica (N-I5). El contenido es hostil
    /// (ARC-02): el primer mensaje inválido, de otro `tab_id` (N-I8) o un pipe roto corta la lectura y
    /// queda en `violation` para que el llamador termine el worker.
    pub fn poll_content_events(&self, tab_id: TabId, max: usize) -> PolledContentEvents {
        let mut out = PolledContentEvents::default();
        let Some(pipe) = self.workers.get(&tab_id).and_then(|w| w.pipe_server.as_ref()) else {
            return out;
        };
        for _ in 0..max {
            let (msg_type, payload) = match pipe.try_receive_frame() {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(e) => {
                    out.violation = Some(e);
                    break;
                }
            };
            match crate::ipc::dispatcher::decode_content_ipc(msg_type, &payload) {
                Ok(event) if content_event_tab(&event) == tab_id => out.events.push(event),
                Ok(_) => {
                    out.violation = Some(GalapagosError::IpcBoundaryViolation {
                        tab_id,
                        reason: crate::galapagos_core_v1::IpcViolationReason::MalformedMessage,
                    });
                    break;
                }
                Err(e) => {
                    out.violation = Some(e);
                    break;
                }
            }
        }
        out
    }

    /// Reenvía un `InputEvent` ya traducido (p. ej. `UiEvent::SurfaceInput`) al worker de la pestaña
    pub fn forward_input(&mut self, tab_id: TabId, event: InputEvent) {
        let res = self.dispatch_input(tab_id, event);
        report_send_failure(tab_id, "INPUT_DISPATCH_FAILED", "entrada", res);
    }

    /// DELTA_08 paso 5: órdenes de payload vacío hacia el worker (atrás, adelante, recargar, repintar)
    pub fn send_tab_command(&mut self, tab_id: TabId, command: TabCommand) {
        if let Some(pipe_server) = self.workers.get(&tab_id).and_then(|w| w.pipe_server.as_ref()) {
            let res = pipe_server.send_frame(command.msg_type(), &[]);
            report_send_failure(tab_id, "TAB_COMMAND_DISPATCH_FAILED", command.label(), res);
        }
    }

    /// Ajusta las dimensiones del viewport de la pestaña activa tras un evento WM_SIZE
    pub fn resize_viewport(&mut self, tab_id: TabId, width: i32, height: i32) {
        if let Some(worker) = self.workers.get_mut(&tab_id) {
            let top = self.toolbar_height;
            let viewport_h = (height - top).max(10);
            if worker.surface_hwnd != 0 {
                unsafe {
                    MoveWindow(worker.surface_hwnd, 0, top, width, viewport_h, 1);
                }
            }
            if let Some(ref pipe_server) = worker.pipe_server {
                const MSG_TYPE_RESIZE: u16 = galapagos_ipc::MSG_RESIZE;
                let mut payload = Vec::with_capacity(8);
                payload.extend_from_slice(&(width.max(1) as u32).to_be_bytes());
                payload.extend_from_slice(&(viewport_h.max(1) as u32).to_be_bytes());
                let res = pipe_server.send_frame(MSG_TYPE_RESIZE, &payload);
                report_send_failure(tab_id, "RESIZE_DISPATCH_FAILED", "redimensionado", res);
            }
        }
    }
}

impl WebEnginePort for ServoEngineAdapter {
    /// Instancia una superficie gráfica HWND y vincula un worker enjaulado (ARC-01 / SEC-01 / H1–H5)
    fn initialize_sandboxed_worker(
        &mut self,
        tab_id: TabId,
        sandbox: WindowsSandboxLevel,
        quota: ResourceQuota,
    ) -> Result<ProcessId, GalapagosError> {
        // Creación del HWND hijo donde Servo renderizará (o buffer offscreen en headless)
        let surface_hwnd = if self.parent_hwnd != 0 {
            let class_name = to_wide("GalapagosRenderViewport");
            let hinstance = unsafe { GetModuleHandleW(null_mut()) };

            let style = WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS;
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    null_mut(),
                    style,
                    0,
                    self.toolbar_height,
                    800,
                    554,
                    self.parent_hwnd,
                    tab_id.0 as _,
                    hinstance,
                    null_mut(),
                )
            };

            if hwnd == 0 {
                return Err(GalapagosError::SandboxEnforcementFailed {
                    win32_error_code: 1400,
                    target_level: sandbox,
                });
            }

            unsafe {
                ShowWindow(hwnd, 5); // SW_SHOW
            }
            hwnd
        } else {
            0 // Modo headless offscreen
        };

        match self.backend {
            // R-02: fuera de las pruebas unitarias `Stub` falla de forma explícita (Guardrail 9)
            #[cfg(not(test))]
            EngineBackend::Stub => {
                drop(SurfaceGuard(surface_hwnd));
                Err(GalapagosError::SandboxEnforcementFailed {
                    win32_error_code: 120, // ERROR_CALL_NOT_IMPLEMENTED
                    target_level: sandbox,
                })
            }
            #[cfg(test)]
            EngineBackend::Stub => {
                // Invariante H1: ProcessId devuelto == PID real del proceso en ejecución (nunca un contador)
                let pid_raw = unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessId() };
                let process_id = ProcessId(std::num::NonZeroU32::new(pid_raw).expect("PID no cero"));

                let handshake = WorkerHandshake {
                    process_id,
                    applied_sandbox: sandbox,
                    protocol_version: std::num::NonZeroU16::new(1).unwrap(),
                };

                self.workers.insert(
                    tab_id,
                    WorkerInstance {
                        process_id,
                        sandbox,
                        quota,
                        surface_hwnd,
                        current_url: "about:blank".to_string(),
                        handshake,
                        process_handle: None,
                        binary: std::path::PathBuf::new(),
                        job_enclosure: None,
                        pipe_server: None,
                    },
                );

                Ok(process_id)
            }
            EngineBackend::Real => {
                // Ante cualquier `?` de aquí en adelante, los guardas liberan en orden inverso:
                // Job (KILL_ON_JOB_CLOSE), proceso hijo, pipe y superficie
                let surface = SurfaceGuard(surface_hwnd);

                // H5: Orden de arranque estricto:
                // 1. Crear Named Pipe exclusivo para el tab
                let pipe_server = RestrictedNamedPipeServer::create_for_tab(tab_id)?;

                // 2. Localizar binario worker
                let worker_bin = find_worker_binary()?;

                // 3. CreateProcessW(CREATE_SUSPENDED | CREATE_NO_WINDOW)
                let cmd = format!(
                    "\"{}\" --tab-id {} --pipe \"{}\" --surface {}",
                    worker_bin.display(),
                    tab_id.0,
                    pipe_server.pipe_name(),
                    surface_hwnd as isize
                );
                let mut cmd_wide = to_wide(&cmd);

                let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
                si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
                let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

                let create_res = unsafe {
                    CreateProcessW(
                        null(),
                        cmd_wide.as_mut_ptr(),
                        null(),
                        null(),
                        0,
                        WORKER_CREATION_FLAGS,
                        null(),
                        null(),
                        &si,
                        &mut pi,
                    )
                };

                if create_res == 0 {
                    return Err(GalapagosError::SandboxEnforcementFailed {
                        win32_error_code: unsafe { windows_sys::Win32::Foundation::GetLastError() },
                        target_level: sandbox,
                    });
                }
                let mut child = ChildProcessGuard { h_process: pi.hProcess, h_thread: pi.hThread };

                // 4. Asignar a Job Object con cuotas forzadas
                let job = JobObjectEnclosure::new(tab_id)?;
                job.apply_resource_limits(&quota)?;
                job.assign_process(pi.hProcess)?;

                // 5. ResumeThread y cierre del handle del hilo
                if unsafe { ResumeThread(pi.hThread) } == u32::MAX {
                    return Err(GalapagosError::SandboxEnforcementFailed {
                        win32_error_code: unsafe { windows_sys::Win32::Foundation::GetLastError() },
                        target_level: sandbox,
                    });
                }
                child.close_thread();

                // 6. Conexión (H3: timeout <= ipc_timeout_ms); el cliente debe ser el hijo lanzado
                pipe_server.wait_for_connection()?;
                verify_worker_client(&pipe_server, tab_id, pi.dwProcessId)?;

                let (msg_type, payload) = pipe_server.receive_frame()?;
                if msg_type != crate::ipc::dispatcher::MSG_TYPE_HANDSHAKE {
                    return Err(GalapagosError::IpcBoundaryViolation {
                        tab_id,
                        reason: crate::galapagos_core_v1::IpcViolationReason::MalformedMessage,
                    });
                }
                let handshake = crate::ipc::dispatcher::decode_worker_handshake(&payload)?;

                // Invariante H1: ProcessId devuelto == WorkerHandshake.process_id == PID del proceso hijo creado
                if handshake.process_id.0.get() != pi.dwProcessId {
                    return Err(GalapagosError::IpcBoundaryViolation {
                        tab_id,
                        reason: crate::galapagos_core_v1::IpcViolationReason::MalformedMessage,
                    });
                }

                // Invariante H2: Si applied_sandbox < nivel solicitado -> matar proceso y devolver SandboxEnforcementFailed.
                // Pendiente T-02: `applied_sandbox` lo declara el propio worker; lo resuelve el delta de AppContainer.
                if !is_sandbox_sufficient(handshake.applied_sandbox, sandbox) {
                    return Err(GalapagosError::SandboxEnforcementFailed {
                        win32_error_code: 5,
                        target_level: sandbox,
                    });
                }

                let process_id = handshake.process_id;
                self.workers.insert(
                    tab_id,
                    WorkerInstance {
                        process_id,
                        sandbox: handshake.applied_sandbox,
                        quota,
                        surface_hwnd: surface.into_inner(),
                        current_url: "about:blank".to_string(),
                        handshake,
                        process_handle: Some(child.into_process_handle()),
                        binary: worker_bin,
                        job_enclosure: Some(job),
                        pipe_server: Some(pipe_server),
                    },
                );

                Ok(process_id)
            }
        }
    }

    /// Despacha la navegación validada hacia la superficie de renderizado y el motor Servo
    fn dispatch_navigation(
        &mut self,
        tab_id: TabId,
        validated_url: &str,
    ) -> Result<(), GalapagosError> {
        let worker = self.workers.get_mut(&tab_id).ok_or(GalapagosError::IpcBoundaryViolation {
            tab_id,
            reason: crate::galapagos_core_v1::IpcViolationReason::MalformedMessage,
        })?;

        worker.current_url = validated_url.to_string();

        // 1. Despacho real hacia el proceso worker confinado a través del canal IPC Named Pipe (MSG_TYPE_NAVIGATE = 10)
        if let Some(ref pipe_server) = worker.pipe_server {
            const MSG_TYPE_NAVIGATE: u16 = galapagos_ipc::MSG_NAVIGATE;
            let url_bytes = validated_url.as_bytes();
            let mut payload = Vec::with_capacity(2 + url_bytes.len());
            payload.extend_from_slice(&(url_bytes.len() as u16).to_be_bytes());
            payload.extend_from_slice(url_bytes);
            return pipe_server.send_frame(MSG_TYPE_NAVIGATE, &payload);
        }

        Ok(())
    }

    /// Termina el worker y destruye de forma determinista la superficie gráfica HWND y la instancia FFI
    fn terminate_worker(&mut self, process_id: ProcessId) -> Result<(), GalapagosError> {
        let target_tab = self
            .workers
            .iter()
            .find(|(_, w)| w.process_id == process_id)
            .map(|(&tid, _)| tid);

        if let Some(tab_id) = target_tab {
            if let Some(worker) = self.workers.remove(&tab_id) {
                // Drop de worker ejecuta limpieza determinista de todos los recursos (RAII)
                drop(worker);
            }
            Ok(())
        } else {
            Err(GalapagosError::ResourceQuotaExceeded {
                tab_id: TabId(0),
                process_id,
                observed_bytes: 0,
                limit_bytes: 0,
            })
        }
    }
}

impl Drop for ServoEngineAdapter {
    fn drop(&mut self) {
        self.workers.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// INC-20261026-WORKER-CONSOLE: el worker se lanza suspendido (Job antes de ejecutar) y sin
    /// consola visible (sin traspaso a Windows Terminal que lo bloquee)
    #[test]
    fn worker_launches_suspended_without_console_window_inc20261026() {
        assert_ne!(WORKER_CREATION_FLAGS & CREATE_SUSPENDED, 0);
        assert_ne!(WORKER_CREATION_FLAGS & CREATE_NO_WINDOW, 0, "flags {:#x} sin CREATE_NO_WINDOW", WORKER_CREATION_FLAGS);
    }

    #[test]
    fn test_regression_hotfix_worker_binary_discovery_and_real_dispatch() {
        let worker_res = find_worker_binary();
        assert!(worker_res.is_ok(), "El binario real galapagos-content.exe debe ser localizable en el workspace");
        let path = worker_res.unwrap();
        assert!(path.exists());
        assert!(path.to_string_lossy().contains("galapagos-content.exe"));
    }

    /// Regresión: la ruta del worker debe ser absoluta; una relativa con '/' hace que
    /// CreateProcessW falle con ERROR_FILE_NOT_FOUND (production_certification.rs:177).
    /// DELTA_04 L-I7: gana el candidato existente con modificación más reciente; los inexistentes se ignoran
    #[test]
    fn pick_newest_existing_prefers_latest_modification_delta04() {
        use std::time::{Duration, SystemTime};
        let dir = std::env::temp_dir().join(format!("znve_pick_newest_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (old, new, missing) = (dir.join("old.exe"), dir.join("new.exe"), dir.join("missing.exe"));
        let base = SystemTime::now() - Duration::from_secs(3600);
        for (path, age) in [(&old, 0u64), (&new, 600)] {
            let f = std::fs::File::create(path).unwrap();
            f.set_modified(base + Duration::from_secs(age)).unwrap();
        }
        assert_eq!(pick_newest_existing(vec![old.clone(), missing.clone(), new.clone()]), Some(new.clone()));
        assert_eq!(pick_newest_existing(vec![new.clone(), old.clone()]), Some(new));
        assert_eq!(pick_newest_existing(vec![missing]), None);
        assert_eq!(pick_newest_existing(vec![]), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// INC-20261025-CORE-DEBLOAT T-05: ningún candidato depende del directorio de trabajo
    #[test]
    fn worker_candidates_are_anchored_to_the_executable_inc20261025() {
        let exe_dir = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let candidates = worker_candidates();
        assert_eq!(candidates.first(), Some(&exe_dir.join("galapagos-content.exe")));
        for c in &candidates {
            assert!(c.is_absolute(), "candidato relativo al CWD: {}", c.display());
            assert!(exe_dir.ancestors().any(|a| c.starts_with(a)), "fuera de los ancestros del exe: {}", c.display());
        }
    }

    /// T-03/T-02: solo se acepta el handshake si el cliente del pipe es el proceso esperado (PID del kernel)
    #[test]
    fn verify_worker_client_checks_kernel_pid_inc20261025() {
        use crate::ipc::channel::RestrictedNamedPipeClient;
        let tab = TabId(894);
        let server = RestrictedNamedPipeServer::create_for_tab(tab).unwrap();
        let name = server.pipe_name().to_string();
        let (connected_tx, connected_rx) = std::sync::mpsc::channel::<()>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let client = std::thread::spawn(move || {
            let _client = RestrictedNamedPipeClient::connect(&name, tab).unwrap();
            connected_tx.send(()).unwrap();
            done_rx.recv().unwrap();
        });
        server.wait_for_connection().unwrap();
        connected_rx.recv().unwrap();

        let own_pid = std::process::id();
        assert_eq!(verify_worker_client(&server, tab, own_pid), Ok(()));
        assert_eq!(
            verify_worker_client(&server, tab, own_pid.wrapping_add(4)),
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id: tab,
                reason: crate::galapagos_core_v1::IpcViolationReason::UnauthorizedCommand,
            })
        );
        done_tx.send(()).unwrap();
        client.join().unwrap();
    }

    #[test]
    fn worker_accessors_are_none_without_a_real_process_delta04() {
        let engine = ServoEngineAdapter::with_backend(0, EngineBackend::Stub).unwrap();
        assert_eq!(engine.worker_exit_code(TabId(9)), None);
        assert_eq!(engine.worker_binary(TabId(9)), None);
    }

    fn mods(shift: bool, ctrl: bool) -> KeyModifiers {
        KeyModifiers { shift, ctrl, alt: false, meta: false }
    }

    #[test]
    fn tab_commands_and_nav_state_tab_delta08() {
        let types: Vec<u16> = [TabCommand::GoBack, TabCommand::GoForward, TabCommand::Reload, TabCommand::Repaint]
            .into_iter()
            .map(TabCommand::msg_type)
            .collect();
        assert_eq!(types, vec![14, 15, 16, 17]);
        let e = ContentToCoordinatorIpc::NavState { tab_id: TabId(5), can_back: true, can_forward: false, loading: false };
        assert_eq!(content_event_tab(&e), TabId(5));
    }

    #[test]
    fn content_event_tab_covers_every_variant_delta02() {
        use crate::galapagos_core_v1::{HttpMethod, SiteCapability};
        let t = TabId(5);
        let events = [
            ContentToCoordinatorIpc::FrameReady { tab_id: t, frame_seq: 1 },
            ContentToCoordinatorIpc::RequestNetworkFetch {
                tab_id: t, request_id: 1, url: "https://a.org".into(), method: HttpMethod::Get, is_third_party: false,
            },
            ContentToCoordinatorIpc::RequestCapability {
                tab_id: t, origin: "https://a.org".into(), capability: SiteCapability::LocalStorage,
            },
            ContentToCoordinatorIpc::UrlChanged { tab_id: t, url: "https://a.org".into() },
            ContentToCoordinatorIpc::TitleChanged { tab_id: t, title: "a".into() },
            ContentToCoordinatorIpc::WorkerFault { tab_id: t, message: "fallo".into() },
        ];
        for e in &events {
            assert_eq!(content_event_tab(e), t);
        }
        // Sin worker/pipe no hay nada que leer ni violación
        let engine = ServoEngineAdapter::with_backend(0, EngineBackend::Stub).unwrap();
        let polled = engine.poll_content_events(t, 8);
        assert!(polled.events.is_empty() && polled.violation.is_none());
    }

    #[test]
    fn input_wire_format_matches_worker_decoder_delta02() {
        let enc = |e| encode_input_event(&e).expect("evento serializable");

        // KeyDown/KeyUp: [tipo][vk u32][mods]; Shift=1, Ctrl=2
        assert_eq!(enc(InputEvent::KeyDown { key_code: 0x0D, modifiers: mods(true, true) }), vec![1, 0, 0, 0, 0x0D, 3]);
        assert_eq!(enc(InputEvent::KeyUp { key_code: 0x41, modifiers: mods(false, false) }), vec![2, 0, 0, 0, 0x41, 0]);

        // MouseDown/MouseUp: [tipo][botón][x i32][y i32][mods]; Left=0, Middle=1, Right=2
        let down = enc(InputEvent::MouseDown { button: MouseButton::Right, x: -5, y: 300, modifiers: mods(false, false) });
        assert_eq!(down.len(), 11);
        assert_eq!((down[0], down[1]), (3, 2));
        assert_eq!(i32::from_be_bytes(down[2..6].try_into().unwrap()), -5);
        assert_eq!(i32::from_be_bytes(down[6..10].try_into().unwrap()), 300);
        let up = enc(InputEvent::MouseUp { button: MouseButton::Middle, x: 1, y: 2, modifiers: mods(false, false) });
        assert_eq!((up[0], up[1]), (4, 1));

        // MouseMove: [5][x][y] (9 bytes, el mínimo que exige el worker)
        let mv = enc(InputEvent::MouseMove { x: 10, y: -20, modifiers: mods(false, false) });
        assert_eq!(mv.len(), 9);
        assert_eq!(i32::from_be_bytes(mv[5..9].try_into().unwrap()), -20);

        // MouseWheel: [6][dx f32][dy f32][x][y] (17 bytes)
        let wheel = enc(InputEvent::MouseWheel { delta_x: 0.0, delta_y: -1.0, x: 7, y: 9, modifiers: mods(false, false) });
        assert_eq!(wheel.len(), 17);
        assert_eq!(f32::from_be_bytes(wheel[5..9].try_into().unwrap()), -1.0);
        assert_eq!(i32::from_be_bytes(wheel[13..17].try_into().unwrap()), 9);

        // Char: [7][escalar u32]
        assert_eq!(enc(InputEvent::Char { scalar: 'ñ' }), [vec![7], ('ñ' as u32).to_be_bytes().to_vec()].concat());

        // Resize no usa el mensaje 12
        assert!(encode_input_event(&InputEvent::Resize { width: 1, height: 1 }).is_none());
    }

    #[test]
    fn test_regression_worker_path_is_absolute_for_createprocess() {
        let abs = absolute_worker_path(std::path::Path::new("target/debug/galapagos-content.exe"))
            .expect("la ruta relativa debe resolverse");
        assert!(abs.is_absolute(), "ruta no absoluta: {}", abs.display());
        assert!(
            !abs.to_string_lossy().starts_with(r"\\?\"),
            "prefijo verbatim no admitido en la línea de comandos: {}",
            abs.display()
        );

        let found = find_worker_binary().expect("worker localizable en el workspace");
        assert!(found.is_absolute(), "find_worker_binary devolvió ruta relativa: {}", found.display());
    }
}
