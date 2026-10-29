//! src/islands/performance/mod.rs
//! Tarea F2.3: Performance Island — Monitor en Caliente de Recursos y Auditor de Presupuesto
//! Estándar ZNVE v2.4.0 (Axiomas 1 y 2, PERF-01, RESOURCE_BUDGETS.md, ADR-002).
//!
//! Responsabilidades críticas:
//! 1. Monitor en Caliente de Recursos: Lectura de WorkingSetSize, PeakWorkingSetSize y tiempos de CPU (GetProcessTimes / GetProcessMemoryInfo).
//! 2. Auditor de Violaciones de Presupuesto (ResourceBudgetValidator): Detección proactiva de superación de cuotas (RAM > 512 MiB o timeout > 5 000 ms).
//! 3. Invariante ZNVE: Cero dependencias externas; uso exclusivo de APIs nativas de windows-sys.
//! R-04 (INC-20261025-CORE-DEBLOAT): retirado el gestor de estrangulamiento e hibernación (PERF-02 diferido).

use crate::galapagos_core_v1::{
    GalapagosError, PerformanceSampleDto, ProcessId, ResourceQuota, TabId,
};
use std::collections::HashMap;
use std::mem::size_of;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{FILETIME, HANDLE};
use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

/// Métricas de consumo de recursos físicos observadas en un proceso Win32
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessResourceMetrics {
    pub working_set_bytes: u64,
    pub peak_working_set_bytes: u64,
    pub user_cpu_us: u64,
    pub kernel_cpu_us: u64,
    pub total_cpu_us: u64,
}

impl ProcessResourceMetrics {
    /// Convierte una estructura FILETIME de Win32 (intervalos de 100ns) a microsegundos
    #[inline]
    fn filetime_to_micros(ft: &FILETIME) -> u64 {
        let raw = ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64);
        raw / 10 // 100ns / 10 = 1µs
    }

    /// Muestrea el uso de memoria y CPU para el handle de proceso especificado
    pub fn sample_process(process_handle: HANDLE) -> Result<Self, GalapagosError> {
        unsafe {
            // 1. Memoria física (WorkingSetSize y PeakWorkingSetSize)
            let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            let ret_mem = GetProcessMemoryInfo(
                process_handle,
                &mut pmc,
                size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            );

            if ret_mem == 0 {
                return Err(GalapagosError::SandboxEnforcementFailed {
                    win32_error_code: 1400,
                    target_level: crate::galapagos_core_v1::WindowsSandboxLevel::AppContainerStrict,
                });
            }

            // 2. Tiempos de CPU (User y Kernel)
            let mut creation_time: FILETIME = std::mem::zeroed();
            let mut exit_time: FILETIME = std::mem::zeroed();
            let mut kernel_time: FILETIME = std::mem::zeroed();
            let mut user_time: FILETIME = std::mem::zeroed();

            let ret_cpu = GetProcessTimes(
                process_handle,
                &mut creation_time,
                &mut exit_time,
                &mut kernel_time,
                &mut user_time,
            );

            if ret_cpu == 0 {
                return Err(GalapagosError::SandboxEnforcementFailed {
                    win32_error_code: 1400,
                    target_level: crate::galapagos_core_v1::WindowsSandboxLevel::AppContainerStrict,
                });
            }

            let user_cpu_us = Self::filetime_to_micros(&user_time);
            let kernel_cpu_us = Self::filetime_to_micros(&kernel_time);

            Ok(Self {
                working_set_bytes: pmc.WorkingSetSize as u64,
                peak_working_set_bytes: pmc.PeakWorkingSetSize as u64,
                user_cpu_us,
                kernel_cpu_us,
                total_cpu_us: user_cpu_us.saturating_add(kernel_cpu_us),
            })
        }
    }

    /// Muestrea las métricas del proceso actual
    pub fn sample_current_process() -> Result<Self, GalapagosError> {
        unsafe {
            let current = GetCurrentProcess();
            Self::sample_process(current)
        }
    }
}

/// Veredicto del Auditor de Presupuesto de Recursos
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetVerdict {
    /// El consumo se encuentra dentro del presupuesto autorizado
    WithinBudget,
    /// El tiempo de procesamiento superó la cuota de timeout (máx 5 000 ms).
    /// La memoria excedida no es un veredicto: `validate` devuelve `ResourceQuotaExceeded`.
    TimeoutExceeded {
        elapsed_ms: u32,
        timeout_ms: u32,
    },
}

/// Auditor determinista de presupuestos de recursos por proceso
pub struct ResourceBudgetValidator;

impl ResourceBudgetValidator {
    /// Evalúa las métricas observadas contra la cuota configurada
    pub fn validate(
        tab_id: TabId,
        process_id: ProcessId,
        metrics: &ProcessResourceMetrics,
        quota: &ResourceQuota,
        elapsed_duration: Duration,
    ) -> Result<BudgetVerdict, GalapagosError> {
        let mem_limit = quota.max_working_set_bytes.get();
        if metrics.working_set_bytes > mem_limit {
            return Err(GalapagosError::ResourceQuotaExceeded {
                tab_id,
                process_id,
                observed_bytes: metrics.working_set_bytes,
                limit_bytes: mem_limit,
            });
        }

        let timeout_limit_ms = quota.ipc_timeout_ms.get();
        let elapsed_ms = elapsed_duration.as_millis() as u32;
        if elapsed_ms > timeout_limit_ms {
            return Ok(BudgetVerdict::TimeoutExceeded {
                elapsed_ms,
                timeout_ms: timeout_limit_ms,
            });
        }

        Ok(BudgetVerdict::WithinBudget)
    }
}

/// Historial local de muestreo de una pestaña
struct TabSampleHistory {
    last_sample_instant: Instant,
    last_cpu_time_us: u64,
}

/// Fachada de la Isla de Rendimiento (Performance Island) en el Trusted Coordinator
pub struct PerformanceIsland {
    sampler_history: HashMap<TabId, TabSampleHistory>,
}

impl PerformanceIsland {
    pub fn new() -> Self {
        Self {
            sampler_history: HashMap::new(),
        }
    }

    /// Registra y genera un PerformanceSampleDto a partir del muestreo físico del proceso
    pub fn record_sample(
        &mut self,
        tab_id: TabId,
        process_id: ProcessId,
        process_handle: HANDLE,
        nav_latency_ms: u32,
        sec_overhead_us: u32,
        blocked_trackers: u32,
        quota: &ResourceQuota,
    ) -> Result<(PerformanceSampleDto, BudgetVerdict), GalapagosError> {
        let metrics = ProcessResourceMetrics::sample_process(process_handle)?;
        let now = Instant::now();

        let (cpu_delta_us, elapsed) = if let Some(prev) = self.sampler_history.get_mut(&tab_id) {
            let delta = metrics.total_cpu_us.saturating_sub(prev.last_cpu_time_us);
            let elapsed = now.duration_since(prev.last_sample_instant);
            prev.last_sample_instant = now;
            prev.last_cpu_time_us = metrics.total_cpu_us;
            (delta, elapsed)
        } else {
            self.sampler_history.insert(
                tab_id,
                TabSampleHistory {
                    last_sample_instant: now,
                    last_cpu_time_us: metrics.total_cpu_us,
                },
            );
            (0, Duration::from_millis(nav_latency_ms as u64))
        };

        // Auditoría de presupuesto
        let verdict = ResourceBudgetValidator::validate(
            tab_id,
            process_id,
            &metrics,
            quota,
            elapsed,
        )?;

        let timestamp_monotonic_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let dto = PerformanceSampleDto {
            tab_id,
            process_id,
            timestamp_monotonic_ns,
            private_working_set_bytes: metrics.working_set_bytes,
            cpu_time_delta_us: cpu_delta_us,
            navigation_latency_ms: nav_latency_ms,
            security_filter_overhead_us: sec_overhead_us,
            blocked_trackers_count: blocked_trackers,
        };

        Ok((dto, verdict))
    }

    /// Limpia el historial de una pestaña al cerrarse
    pub fn remove_tab(&mut self, tab_id: TabId) {
        self.sampler_history.remove(&tab_id);
    }
}

impl Default for PerformanceIsland {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};

    fn make_test_quota(ram_bytes: u64) -> ResourceQuota {
        ResourceQuota::new(
            NonZeroU64::new(ram_bytes).unwrap(),
            NonZeroU16::new(5000).unwrap(),
            NonZeroU32::new(1024 * 1024).unwrap(),
            NonZeroU32::new(5000).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn test_current_process_resource_sampling() {
        let metrics = ProcessResourceMetrics::sample_current_process()
            .expect("Muestreo exitoso de memoria y CPU del proceso actual");

        // El proceso de test en ejecución siempre tiene más de 1 MiB de working set
        assert!(metrics.working_set_bytes > 1024 * 1024);
        assert!(metrics.peak_working_set_bytes >= metrics.working_set_bytes);
    }

    #[test]
    fn test_budget_validator_within_budget() {
        let tab_id = TabId(1);
        let pid = ProcessId(std::num::NonZeroU32::new(100).unwrap());
        let quota = make_test_quota(512 * 1024 * 1024); // 512 MiB

        let metrics = ProcessResourceMetrics {
            working_set_bytes: 64 * 1024 * 1024, // 64 MiB (dentro de presupuesto)
            peak_working_set_bytes: 80 * 1024 * 1024,
            user_cpu_us: 1000,
            kernel_cpu_us: 500,
            total_cpu_us: 1500,
        };

        let verdict = ResourceBudgetValidator::validate(
            tab_id,
            pid,
            &metrics,
            &quota,
            Duration::from_millis(50),
        )
        .expect("Validación sin error");

        assert_eq!(verdict, BudgetVerdict::WithinBudget);
    }

    #[test]
    fn test_budget_validator_memory_exceeded_error() {
        let tab_id = TabId(2);
        let pid = ProcessId(std::num::NonZeroU32::new(101).unwrap());
        let quota = make_test_quota(512 * 1024 * 1024); // 512 MiB

        let metrics = ProcessResourceMetrics {
            working_set_bytes: 600 * 1024 * 1024, // 600 MiB (supera presupuesto)
            peak_working_set_bytes: 650 * 1024 * 1024,
            user_cpu_us: 2000,
            kernel_cpu_us: 1000,
            total_cpu_us: 3000,
        };

        let err = ResourceBudgetValidator::validate(
            tab_id,
            pid,
            &metrics,
            &quota,
            Duration::from_millis(100),
        )
        .expect_err("Debe fallar con ResourceQuotaExceeded");

        match err {
            GalapagosError::ResourceQuotaExceeded {
                tab_id: t,
                process_id: p,
                observed_bytes,
                limit_bytes,
            } => {
                assert_eq!(t, tab_id);
                assert_eq!(p, pid);
                assert_eq!(observed_bytes, 600 * 1024 * 1024);
                assert_eq!(limit_bytes, 512 * 1024 * 1024);
            }
            _ => panic!("Tipo de error inesperado"),
        }
    }
}
