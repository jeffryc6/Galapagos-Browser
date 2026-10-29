//! src/main.rs - Punto de entrada del Coordinador de Confianza de Galapagos Browser v0.1
//! Plataforma: Windows Desktop (Win32/Rust).
//! Arquitectura: Coordinador de Confianza desacoplado de procesos de contenido web.

#![cfg_attr(not(test), windows_subsystem = "windows")]

// R-01 (INC-20261025-CORE-DEBLOAT): el binario consume la biblioteca `galapagos_browser`
// en lugar de redeclarar sus módulos (antes se compilaba todo dos veces).
use galapagos_browser::{coordinator, defense, diagnostics, engine, galapagos_core_v1, ipc, storage, window};

use coordinator::Coordinator;
use diagnostics::DiagnosticCategory;
use galapagos_core_v1::{
    NetworkRoutePolicy, PrivacyLevel, ProfileCompartment, ProfileId, ResourceQuota, TabId,
    UiToCoordinatorCmd, WebEnginePort, WindowsSandboxLevel,
};
use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};

/// DELTA_05: el primer `WorkerFault` ya se registró (los siguientes se ignoran)
static WORKER_FAULT_REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Máximo de mensajes del worker atendidos por tick de 16 ms (N-I5)
const MAX_WORKER_EVENTS_PER_TICK: usize = 64;

/// Error irrecuperable de arranque: traza, diálogo nativo y salida (sin consola en producción)
fn fatal(tag: &'static str, what: &str, err: impl std::fmt::Display) -> ! {
    let msg = format!("{}: {}", what, err);
    diagnostics::log_fatal(tag, &msg, None);
    diagnostics::show_fatal_dialog("Galapagos Browser - Error Crítico", &msg);
    std::process::exit(1)
}

fn main() {
    // 0. Inicialización del subsistema de diagnóstico y capturador global de pánicos (ZNVE Guardrail 2)
    if let Err(e) = diagnostics::init_diagnostics() {
        diagnostics::log_error(
            DiagnosticCategory::AppLifecycle,
            "DIAGNOSTICS_INIT_FAILED",
            format!("Fallo al inicializar subsistema de diagnóstico: {}", e),
            None,
        );
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has_flag = |flag: &str| args.iter().any(|a| a == flag);
    let (headless, smoke_test) = (has_flag("--headless"), has_flag("--smoke-test"));

    // 1. Inicialización del Coordinador de Confianza
    let mut coordinator = Coordinator::new();
    let initial_url = "about:blank";
    let tab = TabId(1);

    // 2. Cuota del worker. PRUEBA (2026-10-26, decisión pendiente en RESOURCE_BUDGETS.md): 2 GiB de
    //    memoria comprometida y CPU al 100 % (sin tope duro). Con 512 MiB y tope del 50 % el worker
    //    agotaba la memoria con google.com ("memory allocation of 2097218 bytes failed", 0xC0000409).
    let quota = ResourceQuota::new(
        NonZeroU64::new(2 * 1024 * 1024 * 1024).expect("RAM quota no-cero"),
        NonZeroU16::new(galapagos_core_v1::MAX_CPU_RATE_LIMIT_BASIS_POINTS).expect("CPU quota no-cero"),
        NonZeroU32::new(1024 * 1024).expect("IPC payload quota no-cero"),
        NonZeroU32::new(5000).expect("IPC timeout quota no-cero"),
    )
    .unwrap_or_else(|e| fatal("RESOURCE_QUOTA", "Error al configurar cuota de recursos", e));

    // 3. DELTA_07 W-I3: base Personal (WAL en disco; en memoria con --smoke-test, D28) adjunta
    //    ANTES de crear perfiles; sin base no se arranca (D23, sin degradar a memoria en silencio)
    coordinator.attach_storage(
        open_personal_storage(smoke_test)
            .unwrap_or_else(|e| fatal("STORAGE", "Error al abrir la base del perfil Personal", e)),
    );

    // 4. Perfil Personal: el guardado de menor id o uno nuevo (W-I2), y la primera pestaña
    let profile_id = bootstrap_personal_profile(&mut coordinator)
        .unwrap_or_else(|e| fatal("PROFILE", "Error al cargar o crear el perfil predeterminado", e));
    coordinator
        .dispatch_ui_cmd(UiToCoordinatorCmd::SpawnTab {
            profile_id,
            initial_url: initial_url.to_string(),
            quota,
        })
        .unwrap_or_else(|e| fatal("TAB_SPAWN", "Error al inicializar pestaña", e));

    // Lista de amenazas de prueba: solo en el escenario de certificación (R10)
    if smoke_test {
        coordinator.defense_mut().block_host("phishing.evil.com".to_string());
    }

    // 5. Instanciación de la ventana nativa Win32 (UI Desktop)
    let native_win = (!headless).then(|| {
        let win = window::NativeWindow::new("Galapagos Browser v0.1 - Trusted Coordinator", 1024, 768)
            .unwrap_or_else(|e| fatal("WINDOW", "No se pudo inicializar la ventana Win32", e));
        // D33: sin insignia hasta el primer veredicto real de Ocean Defense
        win.set_url(initial_url);
        win
    });

    // 6. Inicialización del adaptador del motor de renderizado (WebEnginePort)
    let parent_hwnd = native_win.as_ref().map_or(0, |w| w.hwnd());
    let mut engine = engine::ServoEngineAdapter::new(parent_hwnd)
        .and_then(|mut eng| {
            if let Some(win) = native_win.as_ref() {
                eng.set_toolbar_height(win.toolbar_height());
            }
            let pid = eng.initialize_sandboxed_worker(tab, WindowsSandboxLevel::AppContainerStrict, quota)?;
            Ok((eng, pid))
        })
        .map(|(mut eng, _pid)| {
            if let Err(e) = eng.dispatch_navigation(tab, initial_url) {
                diagnostics::log_error(
                    DiagnosticCategory::Ipc,
                    "INITIAL_NAVIGATION_FAILED",
                    format!("Fallo al despachar la URL inicial al worker: {}", e),
                    None,
                );
            }
            eng
        })
        .map_err(|e| {
            // Sin consola en producción: el fallo debe quedar en el log y visible en la ventana (R9)
            diagnostics::log_error(
                DiagnosticCategory::AppLifecycle,
                "ENGINE_UNAVAILABLE",
                format!("Motor de renderizado no disponible: {}", e),
                None,
            );
            if let Some(win) = native_win.as_ref() {
                win.set_engine_fault(window::EngineFault::Unavailable);
            }
        })
        .ok();

    // 7. Flujo de navegación: URLs de la CLI o galapagos://dashboard como portada predeterminada
    let mut target_urls: Vec<String> = args.iter().filter(|a| !a.starts_with("--")).cloned().collect();
    if target_urls.is_empty() {
        target_urls.push("galapagos://dashboard".to_string());
    }

    for url in target_urls {
        let nav_start = std::time::Instant::now();
        let sec_overhead_us = match navigate_checked(&mut coordinator, &mut engine, native_win.as_ref(), &url) {
            NavOutcome::Rejected => continue,
            NavOutcome::Evaluated { sec_overhead_us, .. } => sec_overhead_us,
        };

        // Registro en Performance Island + SQLite (solo certificación: usa un ProcessId fijo)
        if !smoke_test {
            continue;
        }
        let current_process = unsafe { windows_sys::Win32::System::Threading::GetCurrentProcess() };
        let pid = galapagos_core_v1::ProcessId(NonZeroU32::new(1).unwrap());
        match coordinator.performance_mut().record_sample(
            tab,
            pid,
            current_process,
            nav_start.elapsed().as_millis() as u32,
            sec_overhead_us as u32,
            0,
            &quota,
        ) {
            Ok((sample, _verdict)) => {
                // D27: la muestra va a la base del perfil de la pestaña
                let persisted = coordinator.storage_for_tab(tab).and_then(|storage| {
                    storage.insert_perf_sample(
                        tab,
                        sample.timestamp_monotonic_ns as i64,
                        sample.private_working_set_bytes as i64,
                        sample.cpu_time_delta_us as i64,
                        sample.navigation_latency_ms as i64,
                        sample.security_filter_overhead_us as i64,
                    )
                });
                if let Err(e) = persisted {
                    diagnostics::log_warn(
                        DiagnosticCategory::Storage,
                        "PERF_SAMPLE_PERSIST_FAILED",
                        format!("No se pudo persistir la muestra de rendimiento: {}", e),
                        None,
                    );
                }
            }
            Err(e) => diagnostics::log_warn(
                DiagnosticCategory::AppLifecycle,
                "PERFORMANCE_BUDGET_EXCEEDED",
                format!("Transgresión de presupuesto detectada: {}", e),
                None,
            ),
        }
    }

    if let Some(win) = native_win.as_ref().filter(|_| !smoke_test) {
        // DELTA_02: la UI emite eventos; el bucle principal valida y despacha (N-I1)
        if let Some(surface) = engine.as_ref().and_then(|e| e.surface_for_tab(tab)) {
            win.set_surface_hwnd(surface);
        }
        win.run_event_loop(|event| match event {
            window::UiEvent::NavigateRequested(input) => {
                handle_address_input(&mut coordinator, &mut engine, win, &input);
            }
            window::UiEvent::SurfaceInput(input) => {
                if let Some(eng) = engine.as_mut() {
                    eng.forward_input(tab, input);
                }
            }
            window::UiEvent::Tick => pump_worker(&mut coordinator, &mut engine, win, tab),
            window::UiEvent::Resize { width, height } => {
                if let Some(eng) = engine.as_mut() {
                    // Δ8: la altura cambia con el DPI (WM_DPICHANGED llega antes que su WM_SIZE)
                    eng.set_toolbar_height(win.toolbar_height());
                    eng.resize_viewport(tab, width as i32, height as i32);
                }
            }
            window::UiEvent::GoBack => send_history(&mut engine, win, tab, engine::TabCommand::GoBack),
            window::UiEvent::GoForward => send_history(&mut engine, win, tab, engine::TabCommand::GoForward),
            window::UiEvent::Reload => send_history(&mut engine, win, tab, engine::TabCommand::Reload),
            window::UiEvent::SurfaceRepaint => {
                if let Some(eng) = engine.as_mut() {
                    eng.send_tab_command(tab, engine::TabCommand::Repaint);
                }
            }
            window::UiEvent::CloseRequested => {}
        });
    }
}

/// DELTA_07 (D23): `%LOCALAPPDATA%\GalapagosBrowser\profiles\personal.db`. Sin `LOCALAPPDATA`
/// no hay respaldo en `%TEMP%`: los datos del usuario no van a un directorio que se purga.
fn personal_db_path(local_app_data: Option<std::ffi::OsString>) -> Result<std::path::PathBuf, galapagos_core_v1::GalapagosError> {
    local_app_data
        .filter(|v| !v.is_empty())
        .map(|base| std::path::PathBuf::from(base).join("GalapagosBrowser").join("profiles").join("personal.db"))
        .ok_or(galapagos_core_v1::GalapagosError::PersistenceInvariantViolated {
            reason: galapagos_core_v1::PersistenceViolation::DataDirUnavailable,
        })
}

/// Base del compartimento Personal: WAL en disco, o en memoria con `--smoke-test` (D28: la
/// certificación nunca escribe en los datos del usuario)
fn open_personal_storage(smoke_test: bool) -> Result<storage::StorageEngine, galapagos_core_v1::GalapagosError> {
    if smoke_test {
        return storage::StorageEngine::open_in_memory();
    }
    let path = personal_db_path(std::env::var_os("LOCALAPPDATA"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|_| galapagos_core_v1::GalapagosError::PersistenceInvariantViolated {
            reason: galapagos_core_v1::PersistenceViolation::DataDirUnavailable,
        })?;
    }
    storage::StorageEngine::open_file(&path)
}

/// DELTA_07 W-I2: carga los perfiles guardados y devuelve el Personal de menor id; si no hay
/// ninguno, crea uno (Daily, DirectSystem). En el segundo arranque no se reinserta el perfil.
fn bootstrap_personal_profile(coordinator: &mut Coordinator) -> Result<ProfileId, galapagos_core_v1::GalapagosError> {
    coordinator.load_persisted_profiles()?;
    if let Some(id) = coordinator.lowest_profile_id(ProfileCompartment::Personal) {
        return Ok(id);
    }
    coordinator.dispatch_ui_cmd(UiToCoordinatorCmd::CreateProfile {
        compartment: ProfileCompartment::Personal,
        privacy_level: PrivacyLevel::Daily,
        route_policy: NetworkRoutePolicy::DirectSystem,
    })?;
    coordinator
        .lowest_profile_id(ProfileCompartment::Personal)
        .ok_or(galapagos_core_v1::GalapagosError::StorageUnattached)
}

/// Ruta y fecha de modificación (UTC) del binario del worker, para identificar qué versión corre
fn describe_worker_binary(eng: &engine::ServoEngineAdapter, tab: TabId) -> String {
    match eng.worker_binary(tab) {
        Some(path) => {
            let modified = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .map_or_else(|e| format!("fecha no disponible: {}", e), diagnostics::format_iso8601);
            format!("{} (modificado {})", path.display(), modified)
        }
        None => "ninguno".to_string(),
    }
}

/// Resultado de `navigate_checked`
enum NavOutcome {
    /// Destino malformado: ni se evaluó ni se despachó
    Rejected,
    Evaluated {
        #[allow(dead_code)]
        decision: defense::DefenseDecision,
        sec_overhead_us: i64,
    },
}

/// N-I1: camino único de navegación (CLI y UI): Connectivity -> Ocean Defense -> Coordinador -> motor.
/// `url` ya pasó `validate_url_scheme` (CLI: argumentos; UI: `normalize_address_input`).
fn navigate_checked(
    coordinator: &mut Coordinator,
    engine: &mut Option<engine::ServoEngineAdapter>,
    win: Option<&window::NativeWindow>,
    url: &str,
) -> NavOutcome {
    let blocked = |reason: window::BlockReason| {
        if let Some(win) = win {
            win.set_security_badge(window::SecurityBadge::Blocked(reason));
        }
    };

    // Validación previa de destino en Connectivity Island
    if !coordinator.connectivity().validate_destination(url, 443) {
        diagnostics::log_warn(
            DiagnosticCategory::AppLifecycle,
            "DESTINATION_REJECTED",
            format!("Destino inválido o malformado: {}", url),
            None,
        );
        return NavOutcome::Rejected;
    }

    // A. Evaluación en caliente de Ocean Defense. R-13: el Coordinador aporta el contexto real
    //    (perfil, nivel de privacidad y URL vigente); sin pestaña no se navega (fail-closed).
    let eval_start = std::time::Instant::now();
    let eval = match coordinator.evaluate_navigation(TabId(1), url) {
        Ok(eval) => eval,
        Err(e) => {
            diagnostics::log_warn(
                DiagnosticCategory::AppLifecycle,
                "NAVIGATION_CONTEXT_MISSING",
                format!("Sin pestaña o perfil para evaluar {}: {}", url, e),
                None,
            );
            return NavOutcome::Rejected;
        }
    };
    let sec_overhead_us = eval_start.elapsed().as_micros() as i64;

    match eval.decision {
        defense::DefenseDecision::Allow | defense::DefenseDecision::Restrict => {
            dispatch_allowed(coordinator, engine, win, url, false);
        }
        defense::DefenseDecision::Block => blocked(window::BlockReason::Threat),
        defense::DefenseDecision::RequireConfirmation => {
            blocked(window::BlockReason::ConfirmationRequired);
            let confirmed = coordinator.freedom().request_user_confirmation(
                win.map_or(0, |w| w.hwnd()),
                url,
                "Navegación que requiere confirmación explícita del usuario",
            );
            if confirmed {
                // P-I2: único camino que fija `user_confirmed`, justo tras el consentimiento explícito
                dispatch_allowed(coordinator, engine, win, url, true);
            }
        }
        defense::DefenseDecision::Unavailable => {
            blocked(window::BlockReason::VerificationUnavailable);
            diagnostics::log_warn(
                DiagnosticCategory::AppLifecycle,
                "DEFENSE_UNAVAILABLE",
                format!("Fallo seguro: verificación no completada para {}", url),
                None,
            );
        }
    }

    NavOutcome::Evaluated { decision: eval.decision, sec_overhead_us }
}

/// Despacho de una navegación ya evaluada. DELTA_03 P-I1: el motor solo recibe la URL si el
/// Coordinador (Behavior Sentinel + Ocean Defense) la acepta; devuelve si fue aceptada.
fn dispatch_allowed(
    coordinator: &mut Coordinator,
    engine: &mut Option<engine::ServoEngineAdapter>,
    win: Option<&window::NativeWindow>,
    url: &str,
    user_confirmed: bool,
) -> bool {
    if let Err(e) = coordinator.navigate_confirmed(TabId(1), url.to_string(), user_confirmed) {
        if let Some(win) = win {
            win.set_security_badge(window::SecurityBadge::Blocked(window::BlockReason::CoordinatorDenied));
        }
        diagnostics::log_warn(
            DiagnosticCategory::AppLifecycle,
            "COORDINATOR_NAVIGATE_FAILED",
            format!("Navegación rechazada por el Coordinador: {}", e),
            None,
        );
        return false;
    }

    if let Some(win) = win {
        win.set_url(url);
    }

    if url.starts_with("galapagos://") {
        if let Some(win) = win {
            win.set_security_badge(window::SecurityBadge::Internal);
            win.set_title("Dashboard — Galapagos Browser");
            win.show_native_dashboard(true);
        }
        return true;
    }

    if let Some(win) = win {
        win.set_security_badge(window::SecurityBadge::Allowed);
        win.show_native_dashboard(false);
    }

    if let Some(Err(e)) = engine.as_mut().map(|eng| eng.dispatch_navigation(TabId(1), url)) {
        diagnostics::log_error(
            DiagnosticCategory::Ipc,
            "NAVIGATION_DISPATCH_FAILED",
            format!("Fallo al despachar URL al motor de renderizado: {}", e),
            None,
        );
    }
    true
}

/// Enter en la barra o botón Inicio: normaliza (D7), valida y navega por el camino único
fn handle_address_input(
    coordinator: &mut Coordinator,
    engine: &mut Option<engine::ServoEngineAdapter>,
    win: &window::NativeWindow,
    input: &galapagos_core_v1::AddressBarInput,
) {
    match galapagos_core_v1::normalize_address_input(input) {
        Ok(url) => {
            if let NavOutcome::Rejected = navigate_checked(coordinator, engine, Some(win), &url) {
                win.set_security_badge(window::SecurityBadge::Blocked(window::BlockReason::InvalidDestination));
            }
        }
        Err(reason) => {
            win.set_security_badge(window::SecurityBadge::Blocked(window::BlockReason::InvalidAddress));
            diagnostics::log_warn(
                DiagnosticCategory::OsEvent,
                "ADDRESS_INPUT_REJECTED",
                format!("Entrada de barra rechazada: {}", reason),
                None,
            );
        }
    }
}

/// DELTA_08 paso 5: atrás, adelante y recargar actúan sobre el historial del worker. Si está a la
/// vista el panel interno, el worker sigue en su página: se vuelve a ella en vez de enviar la orden.
fn send_history(
    engine: &mut Option<engine::ServoEngineAdapter>,
    win: &window::NativeWindow,
    tab: TabId,
    command: engine::TabCommand,
) {
    let Some(eng) = engine.as_mut() else { return };
    if win.is_native_dashboard_shown() {
        win.leave_native_dashboard();
        if command != engine::TabCommand::Reload {
            return;
        }
    }
    eng.send_tab_command(tab, command);
}

/// N-I5: atiende sin bloquear los mensajes del worker y aplica URL/título reales (N-I4)
fn pump_worker(
    coordinator: &mut Coordinator,
    engine: &mut Option<engine::ServoEngineAdapter>,
    win: &window::NativeWindow,
    tab: TabId,
) {
    use galapagos_core_v1::ContentToCoordinatorIpc;

    let Some(eng) = engine.as_mut() else { return };
    let polled = eng.poll_content_events(tab, MAX_WORKER_EVENTS_PER_TICK);

    for event in &polled.events {
        if let Err(e) = ipc::dispatcher::IpcDispatcher::dispatch_event(coordinator, event) {
            diagnostics::log_warn(
                DiagnosticCategory::Ipc,
                "CONTENT_EVENT_REJECTED",
                format!("Evento del worker rechazado por política: {}", e),
                None,
            );
        }
        match event {
            ContentToCoordinatorIpc::UrlChanged { url, .. } => win.set_url_from_engine(url),
            ContentToCoordinatorIpc::TitleChanged { title, .. } => win.set_title(title),
            // DELTA_11 D49: la página cayó pero el worker sigue vivo (no es `EngineFault`)
            ContentToCoordinatorIpc::ContentCrashed { reason, .. } => {
                diagnostics::log_warn(
                    DiagnosticCategory::Ipc,
                    "CONTENT_CRASHED",
                    format!("Una página del worker falló: {}", reason),
                    None,
                );
                win.set_security_badge(window::SecurityBadge::PageCrashed);
            }
            ContentToCoordinatorIpc::NavState { can_back, can_forward, loading, .. } => {
                win.set_nav_state(window::NavState { can_back: *can_back, can_forward: *can_forward, loading: *loading })
            }
            // DELTA_05 A-I5: solo el primer fallo se registra; las tramas 8 posteriores se ignoran
            ContentToCoordinatorIpc::WorkerFault { message, .. }
                if !WORKER_FAULT_REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) =>
            {
                diagnostics::log_error(
                    DiagnosticCategory::Ipc,
                    "WORKER_FAULT",
                    format!("El worker informó un fallo: {}", message),
                    None,
                );
                win.set_engine_fault(window::EngineFault::WorkerFault);
            }
            _ => {}
        }
    }

    // ARC-02: contenido hostil o canal roto => se termina el worker (sin degradación silenciosa)
    if let Some(violation) = polled.violation {
        // L-I1: antes de terminar el worker, dejar constancia de por qué cayó el canal
        let exit = eng
            .worker_exit_code(tab)
            .map_or_else(|| "en ejecución".to_string(), |code| format!("terminó con código {} (0x{:X})", code, code));
        diagnostics::log_error(
            DiagnosticCategory::Ipc,
            "CONTENT_CHANNEL_VIOLATION",
            format!(
                "Canal del worker cerrado: {} | worker {} | binario: {}",
                violation,
                exit,
                describe_worker_binary(eng, tab)
            ),
            None,
        );
        if let Some(pid) = eng.handshake_for_tab(tab).map(|h| h.process_id) {
            if let Err(e) = eng.terminate_worker(pid) {
                diagnostics::log_error(
                    DiagnosticCategory::Ipc,
                    "WORKER_TERMINATE_FAILED",
                    format!("No se pudo terminar el worker {:?}: {}", pid, e),
                    None,
                );
            }
        }
        win.set_engine_fault(window::EngineFault::WorkerTerminated);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Perfil Personal/Daily y pestaña 1, con `phishing.evil.com` en la blocklist
    fn coordinator_with_tab() -> Coordinator {
        let mut c = Coordinator::new();
        c.defense_mut().block_host("phishing.evil.com".to_string());
        c.dispatch_ui_cmd(UiToCoordinatorCmd::CreateProfile {
            compartment: ProfileCompartment::Personal,
            privacy_level: PrivacyLevel::Daily,
            route_policy: NetworkRoutePolicy::DirectSystem,
        })
        .unwrap();
        let quota = ResourceQuota::new(
            NonZeroU64::new(512 * 1024 * 1024).unwrap(),
            NonZeroU16::new(5000).unwrap(),
            NonZeroU32::new(1024 * 1024).unwrap(),
            NonZeroU32::new(5000).unwrap(),
        )
        .unwrap();
        c.dispatch_ui_cmd(UiToCoordinatorCmd::SpawnTab {
            profile_id: ProfileId(1),
            initial_url: "about:blank".to_string(),
            quota,
        })
        .unwrap();
        c
    }

    /// N-I1 / N-I3: el camino único evalúa y un destino bloqueado no se despacha (sin motor ni ventana)
    #[test]
    fn navigate_checked_blocks_listed_host_and_never_panics_without_engine_delta02() {
        let mut coordinator = coordinator_with_tab();
        let mut engine = None;
        match navigate_checked(&mut coordinator, &mut engine, None, "https://phishing.evil.com/login") {
            NavOutcome::Evaluated { decision, .. } => {
                assert_eq!(decision, defense::DefenseDecision::Block)
            }
            NavOutcome::Rejected => panic!("el host bloqueado debe evaluarse, no rechazarse"),
        }
        // Un destino normal también completa la evaluación sin motor
        assert!(matches!(
            navigate_checked(&mut coordinator, &mut engine, None, "https://servo.org"),
            NavOutcome::Evaluated { .. }
        ));
    }

    /// INC-20261025-CORE-DEBLOAT R-13: la UI evalúa con el perfil real de la pestaña. Con un perfil
    /// Hardened, http:// es Restrict (antes la UI evaluaba con Daily fijo y veía Allow).
    #[test]
    fn navigate_checked_uses_real_profile_privacy_inc20261025() {
        let mut coordinator = Coordinator::new();
        coordinator
            .dispatch_ui_cmd(UiToCoordinatorCmd::CreateProfile {
                compartment: ProfileCompartment::Personal,
                privacy_level: PrivacyLevel::Hardened,
                route_policy: NetworkRoutePolicy::DirectSystem,
            })
            .unwrap();
        let quota = ResourceQuota::new(
            NonZeroU64::new(512 * 1024 * 1024).unwrap(),
            NonZeroU16::new(5000).unwrap(),
            NonZeroU32::new(1024 * 1024).unwrap(),
            NonZeroU32::new(5000).unwrap(),
        )
        .unwrap();
        coordinator
            .dispatch_ui_cmd(UiToCoordinatorCmd::SpawnTab {
                profile_id: ProfileId(1),
                initial_url: "about:blank".to_string(),
                quota,
            })
            .unwrap();
        let mut engine = None;
        match navigate_checked(&mut coordinator, &mut engine, None, "http://insecure-news.org") {
            NavOutcome::Evaluated { decision, .. } => assert_eq!(decision, defense::DefenseDecision::Restrict),
            NavOutcome::Rejected => panic!("con pestaña y perfil la navegación debe evaluarse"),
        }
    }

    /// DELTA_07 D23: ruta bajo %LOCALAPPDATA%; sin la variable (o vacía) no hay respaldo
    #[test]
    fn personal_db_path_requires_local_app_data_delta07() {
        let unavailable = Err(galapagos_core_v1::GalapagosError::PersistenceInvariantViolated {
            reason: galapagos_core_v1::PersistenceViolation::DataDirUnavailable,
        });
        assert_eq!(personal_db_path(None), unavailable);
        assert_eq!(personal_db_path(Some("".into())), unavailable);
        assert_eq!(
            personal_db_path(Some(r"C:\Users\x\AppData\Local".into())),
            Ok(std::path::PathBuf::from(r"C:\Users\x\AppData\Local\GalapagosBrowser\profiles\personal.db"))
        );
    }

    /// W-I2/W-I3: el primer arranque crea el perfil Personal; el segundo, sobre la misma base,
    /// lo reutiliza sin reinsertarlo (antes: fallo por clave primaria en el segundo arranque)
    #[test]
    fn bootstrap_reuses_persisted_personal_profile_delta07() {
        let mut first = Coordinator::new();
        first.attach_storage(storage::StorageEngine::open_in_memory().unwrap());
        assert_eq!(bootstrap_personal_profile(&mut first), Ok(ProfileId(1)));
        let rows = first.storage().unwrap().list_profiles().unwrap();
        assert_eq!(rows.len(), 1);

        // Segundo arranque simulado: base con el perfil ya guardado
        let reopened = storage::StorageEngine::open_in_memory().unwrap();
        reopened
            .insert_profile(rows[0].profile_id, rows[0].compartment, rows[0].privacy_level, rows[0].created_at_epoch_s)
            .unwrap();
        let mut second = Coordinator::new();
        second.attach_storage(reopened);
        assert_eq!(bootstrap_personal_profile(&mut second), Ok(ProfileId(1)));
        assert_eq!(second.storage().unwrap().list_profiles().unwrap().len(), 1);
        assert_eq!(second.profile_count(), 1);
    }

    /// D28: --smoke-test nunca abre la base del usuario
    #[test]
    fn smoke_test_storage_is_in_memory_delta07() {
        let storage = open_personal_storage(true).unwrap();
        assert!(storage.list_profiles().unwrap().is_empty());
    }

    /// R-13: sin pestaña registrada no hay contexto real y la navegación se rechaza (fail-closed)
    #[test]
    fn navigate_checked_rejects_without_tab_inc20261025() {
        let mut coordinator = Coordinator::new();
        let mut engine = None;
        assert!(matches!(
            navigate_checked(&mut coordinator, &mut engine, None, "https://servo.org"),
            NavOutcome::Rejected
        ));
    }

    /// DELTA_03 P-I1: un rechazo del Coordinador (aquí, bucle de redirecciones del Behavior Sentinel)
    /// impide el despacho, aunque la evaluación previa de Ocean Defense lo permitiera.
    #[test]
    fn dispatch_allowed_stops_when_coordinator_rejects_delta03() {
        use galapagos_core_v1::TabId as T;
        let mut coordinator = Coordinator::new();
        coordinator
            .dispatch_ui_cmd(UiToCoordinatorCmd::CreateProfile {
                compartment: ProfileCompartment::Personal,
                privacy_level: PrivacyLevel::Daily,
                route_policy: NetworkRoutePolicy::DirectSystem,
            })
            .unwrap();
        let quota = ResourceQuota::new(
            NonZeroU64::new(512 * 1024 * 1024).unwrap(),
            NonZeroU16::new(5000).unwrap(),
            NonZeroU32::new(1024 * 1024).unwrap(),
            NonZeroU32::new(5000).unwrap(),
        )
        .unwrap();
        coordinator
            .dispatch_ui_cmd(UiToCoordinatorCmd::SpawnTab {
                profile_id: ProfileId(1),
                initial_url: "about:blank".to_string(),
                quota,
            })
            .unwrap();
        let mut engine = None;

        assert!(dispatch_allowed(&mut coordinator, &mut engine, None, "https://servo.org", false));
        assert_eq!(coordinator.current_url(T(1)), Some("https://servo.org"));

        for i in 1..=10 {
            coordinator.sentinel_mut().record_navigation_hop(T(1), &format!("https://redirect{}.org", i));
        }
        assert!(!dispatch_allowed(&mut coordinator, &mut engine, None, "https://redirect11.org", true));
        // El estado del Coordinador no avanzó a la URL rechazada
        assert_eq!(coordinator.current_url(T(1)), Some("https://servo.org"));
    }
}
