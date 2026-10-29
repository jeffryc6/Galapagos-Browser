//! contracts/galapagos_diagnostics_v1.rs
//! Contrato de Telemetría, Logging Estructurado y Trazabilidad de Procesos y Eventos de SO
//! Conforme al estándar ZNVE v2.4.2 (Axiomas 1 y 2, Guardrail 1 Anti-Bloat Fence, Guardrail 2 Cero Ruido).
//!
//! Restricciones inviolables:
//! 1. Cero dependencias externas: sin crates como log, tracing, o chrono.
//! 2. Trazabilidad integral de ciclo de vida de aplicación y eventos de SO (Win32).
//! 3. Búfer acotado en disco con rotación determinista a 5 MiB.

use std::fmt;

/// Nivel de severidad estandarizado para la telemetría de aplicación y SO
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticSeverity {
    /// Hitos normales de ciclo de vida, transiciones de estado y eventos operativos
    Info = 1,
    /// Advertencias no críticas, eventos inesperados o consumos elevados
    Warning = 2,
    /// Errores recuperables de subsistemas, IPC o persistencia
    Error = 3,
    /// Fallos irrecuperables, pánicos o eventos críticos que detienen el proceso
    Critical = 4,
}

impl fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARN"),
            Self::Error => write!(f, "ERROR"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Categoría de procedencia del evento
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCategory {
    /// Ciclo de vida del proceso y subsistemas de la aplicación
    AppLifecycle,
    /// Mensajes y eventos del sistema operativo (Win32 WndProc, HWND, etc.)
    OsEvent,
    /// Protocolo y transporte de canal IPC entre Coordinador y Worker
    Ipc,
    /// Persistencia SQLite y gestión de cuotas de disco
    Storage,
    /// Aislamiento de seguridad, defensas de frontera y Job Objects
    Security,
    /// Capturador de pánicos no controlados y anomalías críticas
    Crash,
}

impl fmt::Display for DiagnosticCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AppLifecycle => write!(f, "APP_LIFECYCLE"),
            Self::OsEvent => write!(f, "OS_EVENT"),
            Self::Ipc => write!(f, "IPC_TRACE"),
            Self::Storage => write!(f, "STORAGE"),
            Self::Security => write!(f, "SECURITY"),
            Self::Crash => write!(f, "CRASH_HANDLER"),
        }
    }
}

/// DTO inmutable de trazabilidad y diagnóstico estructurado
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRecordDto {
    pub timestamp_utc_iso8601: String,
    pub process_id: u32,
    pub thread_id: u64,
    pub severity: DiagnosticSeverity,
    pub category: DiagnosticCategory,
    pub event_name: &'static str,
    pub message: String,
    pub win32_code: Option<u32>,
    pub file: Option<&'static str>,
    pub line: Option<u32>,
}

impl DiagnosticRecordDto {
    /// Formato determinista de una línea compatible con analizadores forenses tipo syslog
    pub fn to_log_line(&self) -> String {
        let code_str = match self.win32_code {
            Some(code) => format!(" [Win32: 0x{:08X}/{}]", code, code),
            None => String::new(),
        };
        let loc_str = match (self.file, self.line) {
            (Some(f), Some(l)) => format!(" at {}:{}", f, l),
            (Some(f), None) => format!(" at {}", f),
            _ => String::new(),
        };
        format!(
            "[{}] [{}] [PID:{}] [TID:{}] [{}] [{}] {}{}{}\n",
            self.timestamp_utc_iso8601,
            self.severity,
            self.process_id,
            self.thread_id,
            self.category,
            self.event_name,
            self.message,
            code_str,
            loc_str
        )
    }
}

/// Límites estrictos de persistencia diagnóstica (Anti-Bloat Fence)
pub const MAX_DIAGNOSTIC_TRACE_SIZE_BYTES: u64 = 5 * 1024 * 1024; // 5 MiB acotado
pub const DIAGNOSTIC_TRACE_FILE_NAME: &str = "app_trace.log";
pub const DIAGNOSTIC_TRACE_BACKUP_NAME: &str = "app_trace.log.old";
pub const DIAGNOSTIC_CRASH_FILE_NAME: &str = "coordinator_error.log";

/// Trait inmutable para el sumidero de trazabilidad diagnóstica
pub trait DiagnosticSink: Send + Sync {
    /// Escribe un registro estructurado hacia el medio de persistencia
    fn record(&self, record: &DiagnosticRecordDto) -> Result<(), std::io::Error>;
    /// Fuerza el volcado determinista a disco
    fn flush(&self) -> Result<(), std::io::Error>;
}
