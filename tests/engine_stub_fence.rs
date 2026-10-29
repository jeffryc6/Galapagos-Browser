//! tests/engine_stub_fence.rs
//! INC-20261025-CORE-DEBLOAT R-02: fuera de las pruebas unitarias no existe motor simulado.
//! Las pruebas de integración compilan la biblioteca sin `cfg(test)`, igual que el binario.

use galapagos_browser::engine::ServoEngineAdapter;
use galapagos_browser::galapagos_core_v1::{
    EngineBackend, GalapagosError, ResourceQuota, TabId, WebEnginePort, WindowsSandboxLevel,
};

use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};

#[test]
fn stub_backend_fails_explicitly_outside_unit_tests_inc20261025() {
    let quota = ResourceQuota::new(
        NonZeroU64::new(512 * 1024 * 1024).unwrap(),
        NonZeroU16::new(5000).unwrap(),
        NonZeroU32::new(1024 * 1024).unwrap(),
        NonZeroU32::new(5000).unwrap(),
    )
    .unwrap();
    let mut engine = ServoEngineAdapter::with_backend(0, EngineBackend::Stub).unwrap();

    let res = engine.initialize_sandboxed_worker(TabId(301), WindowsSandboxLevel::AppContainerStrict, quota);
    assert_eq!(
        res,
        Err(GalapagosError::SandboxEnforcementFailed {
            win32_error_code: 120,
            target_level: WindowsSandboxLevel::AppContainerStrict,
        })
    );
    assert_eq!(engine.active_worker_count(), 0);
    assert_eq!(engine.surface_for_tab(TabId(301)), None);
}
