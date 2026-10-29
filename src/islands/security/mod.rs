//! src/islands/security/mod.rs
//! Tarea F2.1: Security Island — Enjaulamiento por Hardware y Mitigaciones Win32
//! Estándar ZNVE v2.4.0 (Axiomas 1 y 2, ARC-01, SEC-01, SEC-02, ADR-001, ADR-005).
//!
//! Responsabilidades críticas:
//! 1. Enjaulamiento Win32 Job Object: Creación, configuración y asignación de workers a Job Objects dedicados.
//! 2. Cuotas de Hardware forzadas por SO: Límites de memoria (JOB_OBJECT_LIMIT_PROCESS_MEMORY) y CPU (JOB_OBJECT_CPU_RATE_CONTROL_ENABLE).
//! 3. Mitigaciones de Proceso Win32: Signature Policy (MicrosoftSignedOnly) y Arbitrary Code Guard (ACG / ProhibitDynamicCode).
//! 4. Invariante ZNVE: Cero dependencias externas; uso exclusivo de llamadas nativas de windows-sys.

use crate::galapagos_core_v1::{
    GalapagosError, ResourceQuota, TabId, WindowsSandboxLevel, MAX_CPU_RATE_LIMIT_BASIS_POINTS,
};
use std::mem::size_of;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectCpuRateControlInformation,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
    JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};
use windows_sys::Win32::System::Threading::{
    SetProcessMitigationPolicy, ProcessDynamicCodePolicy,
    ProcessSignaturePolicy, ProcessStrictHandleCheckPolicy,
};

/// Envoltorio seguro RAII para un Windows Job Object asignado a un worker de contenido
pub struct JobObjectEnclosure {
    handle: HANDLE,
    tab_id: TabId,
}

unsafe impl Send for JobObjectEnclosure {}
unsafe impl Sync for JobObjectEnclosure {}

impl JobObjectEnclosure {
    /// Crea un Job Object **sin nombre** para el worker de una pestaña. T-06
    /// (INC-20261025-CORE-DEBLOAT): antes se llamaba `Galapagos_TabJob_<tab>`, de modo que el
    /// Coordinador y el motor abrían el mismo objeto dos veces y cualquier proceso del usuario
    /// podía abrirlo por nombre y relajar sus límites. Sin nombre, solo existe su handle.
    pub fn new(tab_id: TabId) -> Result<Self, GalapagosError> {
        let handle = unsafe { CreateJobObjectW(null_mut(), null_mut()) };
        if handle == 0 {
            return Err(GalapagosError::SandboxEnforcementFailed {
                win32_error_code: unsafe { GetLastError() },
                target_level: WindowsSandboxLevel::AppContainerStrict,
            });
        }

        Ok(Self { handle, tab_id })
    }

    /// Aplica cuotas estrictas de hardware forzadas a nivel de kernel de Windows
    pub fn apply_resource_limits(&self, quota: &ResourceQuota) -> Result<(), GalapagosError> {
        // 1. Límites de memoria física y terminación atómica al cerrar el job
        let mut extended_limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        extended_limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        extended_limits.ProcessMemoryLimit = quota.max_working_set_bytes.get() as usize;

        let ret_mem = unsafe {
            SetInformationJobObject(
                self.handle,
                JobObjectExtendedLimitInformation,
                &extended_limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };

        if ret_mem == 0 {
            return Err(GalapagosError::SandboxEnforcementFailed {
                win32_error_code: unsafe { GetLastError() },
                target_level: WindowsSandboxLevel::AppContainerStrict,
            });
        }

        // 2. Límite de tasa de CPU (CPU Rate Control). Una cuota del 100 % no limita nada, así que
        //    no se configura: un tope duro congela todos los hilos del worker al alcanzarlo.
        if quota.cpu_rate_limit_basis_points.get() >= MAX_CPU_RATE_LIMIT_BASIS_POINTS {
            return Ok(());
        }
        // Basis points: 1..=10_000 (100.00%). El valor Win32 CpuRate es en porcentaje x 100
        let mut cpu_rate_info: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = unsafe { std::mem::zeroed() };
        cpu_rate_info.ControlFlags =
            JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
        cpu_rate_info.Anonymous.CpuRate = quota.cpu_rate_limit_basis_points.get() as u32;

        let ret_cpu = unsafe {
            SetInformationJobObject(
                self.handle,
                JobObjectCpuRateControlInformation,
                &cpu_rate_info as *const _ as *const _,
                size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            )
        };

        if ret_cpu == 0 {
            return Err(GalapagosError::SandboxEnforcementFailed {
                win32_error_code: unsafe { GetLastError() },
                target_level: WindowsSandboxLevel::AppContainerStrict,
            });
        }

        Ok(())
    }

    /// Asigna un proceso de contenido (por su handle de proceso) al Job Object
    pub fn assign_process(&self, process_handle: HANDLE) -> Result<(), GalapagosError> {
        let ret = unsafe { AssignProcessToJobObject(self.handle, process_handle) };
        if ret == 0 {
            return Err(GalapagosError::SandboxEnforcementFailed {
                win32_error_code: unsafe { GetLastError() },
                target_level: WindowsSandboxLevel::AppContainerStrict,
            });
        }
        Ok(())
    }

    /// Retorna el identificador de pestaña vinculado
    pub fn tab_id(&self) -> TabId {
        self.tab_id
    }

    /// Retorna el handle Win32 nativo
    pub fn handle(&self) -> HANDLE {
        self.handle
    }
}

impl Drop for JobObjectEnclosure {
    /// Liberación determinista de recursos Win32 (Axioma 1 / Resource Lifecycle)
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe {
                CloseHandle(self.handle);
            }
            self.handle = 0;
        }
    }
}

/// Políticas de mitigación de proceso Win32 para procesos hostiles de contenido web
pub struct ProcessMitigationEngine;

impl ProcessMitigationEngine {
    /// Aplica las políticas de mitigación de proceso en el worker actual:
    /// - Binary Signature Policy: Restringe carga de binarios solo a aquellos firmados por Microsoft.
    /// - Dynamic Code Policy: Prohibición de generación o modificación de código dinámico (ACG).
    /// - Strict Handle Policy: Excepciones inmediatas en desreferencia de handles inválidos.
    pub fn apply_content_worker_mitigations() -> Result<(), GalapagosError> {
        unsafe {
            // 1. Binary Signature Policy (MicrosoftSignedOnly = 1)
            let signature_policy: u32 = 0x00000001;
            let ret_sig = SetProcessMitigationPolicy(
                ProcessSignaturePolicy,
                &signature_policy as *const _ as *const _,
                size_of::<u32>(),
            );

            // 2. Dynamic Code Policy / ACG (ProhibitDynamicCode = 1)
            let dynamic_code_policy: u32 = 0x00000001;
            let ret_dyn = SetProcessMitigationPolicy(
                ProcessDynamicCodePolicy,
                &dynamic_code_policy as *const _ as *const _,
                size_of::<u32>(),
            );

            // 3. Strict Handle Check Policy (0x00000003: RaiseException + PermanentlyEnabled)
            let strict_handle_policy: u32 = 0x00000003;
            let ret_hnd = SetProcessMitigationPolicy(
                ProcessStrictHandleCheckPolicy,
                &strict_handle_policy as *const _ as *const _,
                size_of::<u32>(),
            );

            Self::evaluate_mitigation_results(&[ret_sig, ret_dyn, ret_hnd])
        }
    }

    /// Fail-closed (D-6): cualquier política rechazada por el SO invalida al worker.
    /// `SetProcessMitigationPolicy` devuelve 0 en fallo; el código Win32 se toma de `GetLastError`
    /// solo en esa rama, antes de cualquier otra llamada al SO.
    fn evaluate_mitigation_results(rets: &[i32]) -> Result<(), GalapagosError> {
        if rets.iter().any(|&r| r == 0) {
            let win32_error_code = unsafe { GetLastError() };
            return Err(GalapagosError::SandboxEnforcementFailed {
                win32_error_code,
                target_level: WindowsSandboxLevel::AppContainerStrict,
            });
        }
        Ok(())
    }

    /// Valida si el proceso actual puede ejecutar bajo un Job Object
    pub fn is_job_enclosure_supported() -> bool {
        let handle = unsafe { CreateJobObjectW(null_mut(), null_mut()) };
        if handle != 0 {
            unsafe {
                CloseHandle(handle);
            }
            true
        } else {
            false
        }
    }
}

/// Fachada de la Isla de Seguridad (Security Island) en el Trusted Coordinator
pub struct SecurityIsland {
    active_enclosures: std::collections::HashMap<TabId, JobObjectEnclosure>,
}

impl SecurityIsland {
    pub fn new() -> Self {
        Self {
            active_enclosures: std::collections::HashMap::new(),
        }
    }

    /// Crea y configura un Job Object con cuotas estrictas para una pestaña
    pub fn create_and_configure_enclosure(
        &mut self,
        tab_id: TabId,
        quota: &ResourceQuota,
    ) -> Result<HANDLE, GalapagosError> {
        let enclosure = JobObjectEnclosure::new(tab_id)?;
        enclosure.apply_resource_limits(quota)?;
        let raw_handle = enclosure.handle();
        self.active_enclosures.insert(tab_id, enclosure);
        Ok(raw_handle)
    }

    /// Retorna el handle del Job Object para una pestaña activa
    pub fn get_enclosure_handle(&self, tab_id: TabId) -> Option<HANDLE> {
        self.active_enclosures.get(&tab_id).map(|e| e.handle())
    }

    /// Remueve y destruye deterministamente el Job Object al cerrar una pestaña
    pub fn destroy_enclosure(&mut self, tab_id: TabId) -> bool {
        self.active_enclosures.remove(&tab_id).is_some()
    }

    /// Retorna la cantidad de enjaulamientos activos
    pub fn active_enclosure_count(&self) -> usize {
        self.active_enclosures.len()
    }
}

impl Default for SecurityIsland {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};

    fn make_test_quota(ram_bytes: u64, cpu_basis_points: u16) -> ResourceQuota {
        ResourceQuota::new(
            NonZeroU64::new(ram_bytes).unwrap(),
            NonZeroU16::new(cpu_basis_points).unwrap(),
            NonZeroU32::new(1024 * 1024).unwrap(),
            NonZeroU32::new(5000).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn test_job_object_enclosure_lifecycle_and_quotas() {
        let tab_id = TabId(42);
        let enclosure = JobObjectEnclosure::new(tab_id)
            .expect("Creación exitosa de Windows Job Object");

        assert_eq!(enclosure.tab_id(), tab_id);
        assert_ne!(enclosure.handle(), 0);

        // Cuota: 512 MiB RAM, 50% CPU (5 000 basis points)
        let quota = make_test_quota(512 * 1024 * 1024, 5000);
        enclosure
            .apply_resource_limits(&quota)
            .expect("Aplicación exitosa de límites de RAM y CPU a nivel kernel");

        // Verificación de desecho explícito al salir de scope
        drop(enclosure);
    }

    /// Prueba de cuota (2026-10-26): con el 100 % no se configura control de CPU (sin tope duro);
    /// por debajo del 100 % se mantiene el tope duro. El límite de memoria se aplica igual.
    #[test]
    fn test_full_cpu_quota_sets_no_rate_control_quota_trial() {
        use windows_sys::Win32::System::JobObjects::QueryInformationJobObject;
        let query = |job: &JobObjectEnclosure| unsafe {
            let mut cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = std::mem::zeroed();
            let mut mem: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            assert_ne!(
                QueryInformationJobObject(job.handle(), JobObjectCpuRateControlInformation,
                    &mut cpu as *mut JOBOBJECT_CPU_RATE_CONTROL_INFORMATION as *mut std::ffi::c_void,
                    size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32, null_mut()),
                0
            );
            assert_ne!(
                QueryInformationJobObject(job.handle(), JobObjectExtendedLimitInformation,
                    &mut mem as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *mut std::ffi::c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32, null_mut()),
                0
            );
            (cpu.ControlFlags, mem.ProcessMemoryLimit)
        };

        let full = JobObjectEnclosure::new(TabId(43)).unwrap();
        full.apply_resource_limits(&make_test_quota(2 * 1024 * 1024 * 1024, MAX_CPU_RATE_LIMIT_BASIS_POINTS)).unwrap();
        assert_eq!(query(&full), (0, 2 * 1024 * 1024 * 1024));

        let capped = JobObjectEnclosure::new(TabId(44)).unwrap();
        capped.apply_resource_limits(&make_test_quota(512 * 1024 * 1024, 5000)).unwrap();
        assert_eq!(
            query(&capped),
            (JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, 512 * 1024 * 1024)
        );
    }

    #[test]
    fn test_security_island_facade_and_enclosure_management() {
        let mut island = SecurityIsland::new();
        let tab1 = TabId(101);
        let tab2 = TabId(102);

        let quota = make_test_quota(256 * 1024 * 1024, 3000);

        let h1 = island
            .create_and_configure_enclosure(tab1, &quota)
            .expect("Creación de enjaulamiento para Tab 101");
        assert_ne!(h1, 0);

        let h2 = island
            .create_and_configure_enclosure(tab2, &quota)
            .expect("Creación de enjaulamiento para Tab 102");
        assert_ne!(h2, 0);

        assert_eq!(island.active_enclosure_count(), 2);
        assert_eq!(island.get_enclosure_handle(tab1), Some(h1));

        // Destrucción determinista de enjaulamiento
        assert!(island.destroy_enclosure(tab1));
        assert_eq!(island.active_enclosure_count(), 1);
        assert_eq!(island.get_enclosure_handle(tab1), None);

        assert!(island.destroy_enclosure(tab2));
        assert_eq!(island.active_enclosure_count(), 0);
    }

    #[test]
    fn test_mitigation_partial_failure_is_rejected() {
        // Regresión D-6: antes solo fallaba si las tres políticas fallaban a la vez.
        for failing in 0..3 {
            let mut rets = [1, 1, 1];
            rets[failing] = 0;
            assert!(
                matches!(
                    ProcessMitigationEngine::evaluate_mitigation_results(&rets),
                    Err(GalapagosError::SandboxEnforcementFailed { .. })
                ),
                "política {failing} fallida debe rechazar al worker"
            );
        }
        assert!(ProcessMitigationEngine::evaluate_mitigation_results(&[1, 1, 1]).is_ok());
    }

    #[test]
    fn test_process_mitigation_engine_probe() {
        assert!(ProcessMitigationEngine::is_job_enclosure_supported());
    }
}
