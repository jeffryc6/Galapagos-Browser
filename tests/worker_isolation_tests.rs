//! tests/worker_isolation_tests.rs
//! Pruebas de Aislamiento, Sandboxing y Ciclo de Vida de Worker Confinado (Paso 7 de DELTA 01)
//! Conforme a contracts/DELTA_01_engine_worker.md (Invariantes H1-H5, Decisiones D1-D4).
//! Gobernado bajo estándar ZNVE v2.4.0 (Axiomas 1 y 2).

use galapagos_browser::engine::{is_sandbox_sufficient, ServoEngineAdapter};
use galapagos_browser::galapagos_core_v1::{
    EngineBackend, ResourceQuota, TabId, WebEnginePort, WindowsSandboxLevel,
};
use galapagos_browser::islands::security::JobObjectEnclosure;

use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};

fn test_quota() -> ResourceQuota {
    ResourceQuota::new(
        NonZeroU64::new(512 * 1024 * 1024).unwrap(), // 512 MiB
        NonZeroU16::new(5000).unwrap(),              // 50.00% CPU
        NonZeroU32::new(1024 * 1024).unwrap(),       // 1 MiB IPC
        NonZeroU32::new(5000).unwrap(),              // 5 000 ms timeout
    )
    .unwrap()
}

/// H1: El proceso hijo real tiene un PID del SO distinto al del Coordinador
#[test]
fn test_real_worker_spawn_pid_distinct_from_coordinator() {
    let coordinator_pid = unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessId() };
    let mut engine = ServoEngineAdapter::with_backend(0, EngineBackend::Real)
        .expect("Inicialización de motor con backend Real");

    let tab_id = TabId(201);
    let quota = test_quota();

    let pid_res = engine.initialize_sandboxed_worker(
        tab_id,
        WindowsSandboxLevel::AppContainerStrict,
        quota,
    );

    assert!(pid_res.is_ok(), "Fallo al inicializar worker real: {:?}", pid_res.err());
    let worker_pid = pid_res.unwrap();

    // Invariante H1: PID real del worker != PID del proceso coordinador
    assert_ne!(
        worker_pid.0.get(),
        coordinator_pid,
        "El PID del worker real debe ser distinto al del coordinador"
    );

    // Invariante H6: Nivel de sandbox aplicado registrado del handshake real
    let applied = engine.applied_sandbox_for_tab(tab_id);
    assert_eq!(
        applied,
        Some(WindowsSandboxLevel::AppContainerStrict),
        "El nivel de sandbox reportado debe coincidir con el handshake"
    );

    // Limpieza y terminación atómica
    assert!(engine.terminate_worker(worker_pid).is_ok());
    assert_eq!(engine.active_worker_count(), 0);
}

/// H2: Rechazo por nivel de sandbox menor que el solicitado (cero degradación silenciosa)
#[test]
fn test_rejection_when_applied_sandbox_lower_than_requested() {
    // Si el coordinador solicita AppContainerStrict, pero el worker reporta nivel inferior,
    // is_sandbox_sufficient debe rechazarlo deterministamente
    assert!(!is_sandbox_sufficient(
        WindowsSandboxLevel::TrustedCoordinator,
        WindowsSandboxLevel::AppContainerStrict
    ));
    assert!(!is_sandbox_sufficient(
        WindowsSandboxLevel::RestrictedService,
        WindowsSandboxLevel::AppContainerStrict
    ));

    // Si el worker reporta el mismo nivel o superior, debe ser admitido
    assert!(is_sandbox_sufficient(
        WindowsSandboxLevel::AppContainerStrict,
        WindowsSandboxLevel::AppContainerStrict
    ));
    assert!(is_sandbox_sufficient(
        WindowsSandboxLevel::AppContainerStrict,
        WindowsSandboxLevel::RestrictedService
    ));
}

/// H5 / Cuotas de Hardware: Aplicación y liberación determinista de Job Object Enclosure
#[test]
fn test_job_object_quota_enclosure_lifecycle() {
    let tab_id = TabId(301);
    let job = JobObjectEnclosure::new(tab_id).expect("Creación exitosa de Job Object");
    let quota = test_quota();

    // Aplicación de cuotas de hardware estrictas
    assert!(job.apply_resource_limits(&quota).is_ok());

    // Al salir de este ámbito, el Job Object se destruye deterministamente por RAII
    drop(job);
}
