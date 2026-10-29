//! src/diagnostics/mod.rs
//! Implementación quirúrgica del subsistema de logging, trazabilidad y telemetría de aplicación y SO
//! Conforme al contrato contracts/galapagos_diagnostics_v1.rs y estándar ZNVE v2.4.2.
//!
//! Características inviolables:
//! 1. Cero dependencias parásitas (Anti-Bloat Fence estricto).
//! 2. Telemetría por excepción: advertencias, errores y fallos críticos (Guardrail 2).
//! 3. Persistencia con rotación a 5 MiB en %LOCALAPPDATA%/GalapagosBrowser/logs, evaluada en
//!    cada escritura (P-05, INC-20261025-CORE-DEBLOAT; antes solo al abrir el archivo).
//! 4. Captura global de pánicos (std::panic::set_hook) y notificación nativa (MessageBoxW).

#[path = "../../contracts/galapagos_diagnostics_v1.rs"]
pub mod contract;

pub use contract::{
    DiagnosticCategory, DiagnosticRecordDto, DiagnosticSeverity, DiagnosticSink,
    DIAGNOSTIC_CRASH_FILE_NAME, DIAGNOSTIC_TRACE_BACKUP_NAME, DIAGNOSTIC_TRACE_FILE_NAME,
    MAX_DIAGNOSTIC_TRACE_SIZE_BYTES,
};

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

static GLOBAL_TRACE_SINK: OnceLock<DiagnosticFileWriter> = OnceLock::new();

/// Formatea la fecha y hora UTC actual en formato estándar ISO-8601 (sin crates externos)
pub fn format_iso8601_now() -> String {
    format_iso8601(SystemTime::now())
}

/// Formatea un instante UTC en ISO-8601 (sin crates externos)
pub fn format_iso8601(instant: SystemTime) -> String {
    let dur = instant.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();

    let days = (secs / 86400) as i64;
    let time_in_day = (secs % 86400) as u32;

    // Algoritmo civil Gregoriano determinista
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1020 + doe / 1461 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    let hours = time_in_day / 3600;
    let minutes = (time_in_day % 3600) / 60;
    let seconds = time_in_day % 60;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y, m, d, hours, minutes, seconds, millis
    )
}

/// Archivo de traza abierto y bytes que contiene
struct TraceFile {
    file: File,
    bytes: u64,
}

/// Sumidero de diagnóstico persistente en disco con rotación determinista por tamaño
pub struct DiagnosticFileWriter {
    log_dir: PathBuf,
    max_bytes: u64,
    trace: Mutex<Option<TraceFile>>,
}

impl DiagnosticFileWriter {
    /// Inicializa el sumidero apuntando a una ruta específica (tope `MAX_DIAGNOSTIC_TRACE_SIZE_BYTES`)
    pub fn new<P: AsRef<Path>>(log_dir: P) -> io::Result<Self> {
        Self::with_limit(log_dir, MAX_DIAGNOSTIC_TRACE_SIZE_BYTES)
    }

    fn with_limit<P: AsRef<Path>>(log_dir: P, max_bytes: u64) -> io::Result<Self> {
        let dir = log_dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        let sink = Self {
            log_dir: dir,
            max_bytes,
            trace: Mutex::new(None),
        };
        let opened = sink.open_trace()?;
        *sink.trace.lock().unwrap_or_else(|e| e.into_inner()) = Some(opened);
        Ok(sink)
    }

    /// Retorna la ruta al archivo de log activo
    pub fn log_file_path(&self) -> PathBuf {
        self.log_dir.join(DIAGNOSTIC_TRACE_FILE_NAME)
    }

    /// Mueve el archivo activo a `.old` (sustituyendo el anterior). La ausencia de archivos no es error.
    fn rotate_files(&self) -> io::Result<()> {
        let backup_path = self.log_dir.join(DIAGNOSTIC_TRACE_BACKUP_NAME);
        for result in [
            fs::remove_file(&backup_path),
            fs::rename(self.log_file_path(), &backup_path),
        ] {
            match result {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        Ok(())
    }

    /// Abre el archivo activo en modo añadir; si ya alcanzó el tope, rota antes
    fn open_trace(&self) -> io::Result<TraceFile> {
        let main_path = self.log_file_path();
        let existing = fs::metadata(&main_path).map_or(0, |m| m.len());
        let bytes = if existing >= self.max_bytes {
            self.rotate_files()?;
            0
        } else {
            existing
        };
        let file = OpenOptions::new().create(true).append(true).open(&main_path)?;
        Ok(TraceFile { file, bytes })
    }
}

impl DiagnosticSink for DiagnosticFileWriter {
    fn record(&self, record: &DiagnosticRecordDto) -> io::Result<()> {
        let line = record.to_log_line();
        let mut guard = self.trace.lock().unwrap_or_else(|e| e.into_inner());

        // P-05: la línea que superaría el tope se escribe en un archivo nuevo
        let needs_rotation = guard
            .as_ref()
            .map_or(true, |t| t.bytes > 0 && t.bytes + line.len() as u64 > self.max_bytes);
        if needs_rotation {
            *guard = None; // cerrar antes de renombrar
            if fs::metadata(self.log_file_path()).is_ok_and(|m| m.len() > 0) {
                self.rotate_files()?;
            }
            *guard = Some(self.open_trace()?);
        }

        let trace = guard.as_mut().expect("archivo de traza abierto");
        trace.file.write_all(line.as_bytes())?;
        trace.bytes += line.len() as u64;
        // Para advertencias, errores o caídas, forzar volcado inmediato
        if record.severity >= DiagnosticSeverity::Warning {
            trace.file.flush()?;
        }
        drop(guard);

        // Una caída crítica se copia también en coordinator_error.log para análisis aislado
        if record.severity == DiagnosticSeverity::Critical {
            let crash_path = self.log_dir.join(DIAGNOSTIC_CRASH_FILE_NAME);
            let mut crash_file = OpenOptions::new().create(true).append(true).open(crash_path)?;
            crash_file.write_all(line.as_bytes())?;
            crash_file.flush()?;
        }

        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        let mut guard = self.trace.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(trace) = guard.as_mut() {
            trace.file.flush()?;
        }
        Ok(())
    }
}

/// Obtiene el directorio de logs estándar de Galapagos en Windows (%LOCALAPPDATA%/GalapagosBrowser/logs)
pub fn resolve_default_log_dir() -> PathBuf {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        PathBuf::from(local_app_data)
            .join("GalapagosBrowser")
            .join("logs")
    } else {
        std::env::temp_dir().join("GalapagosBrowser").join("logs")
    }
}

/// Inicializa el subsistema de diagnóstico global y registra el capturador de pánicos
pub fn init_diagnostics() -> io::Result<()> {
    let log_dir = resolve_default_log_dir();
    let sink = DiagnosticFileWriter::new(log_dir)?;
    // Una segunda inicialización conserva el sumidero ya instalado
    let _ = GLOBAL_TRACE_SINK.set(sink);

    // Registro del capturador global de pánicos (Crash Handler ZNVE)
    std::panic::set_hook(Box::new(|panic_info| {
        let payload_msg = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unhandled panic payload".to_string()
        };

        let file = panic_info.location().map(|l| l.file());
        let line = panic_info.location().map(|l| l.line());

        let record = DiagnosticRecordDto {
            timestamp_utc_iso8601: format_iso8601_now(),
            process_id: unsafe { GetCurrentProcessId() },
            thread_id: unsafe { GetCurrentThreadId() as u64 },
            severity: DiagnosticSeverity::Critical,
            category: DiagnosticCategory::Crash,
            event_name: "UNHANDLED_PANIC",
            message: format!("FATAL_PANIC: {}", payload_msg),
            win32_code: None,
            file: file.map(|f| Box::leak(f.to_string().into_boxed_str()) as &'static str),
            line,
        };
        log_record(&record);

        // Mostrar alerta visual nativa si no estamos en entorno desatendido/test
        if !std::env::args().any(|a| a == "--headless" || a == "--smoke-test") {
            show_fatal_dialog(
                "Galapagos Browser - Fallo Crítico",
                &format!(
                    "Se ha producido un error crítico irrecuperable:\n{}\n\nRegistro forense guardado en:\n{}",
                    payload_msg,
                    resolve_default_log_dir().join(DIAGNOSTIC_TRACE_FILE_NAME).display()
                ),
            );
        }
    }));

    Ok(())
}

/// Emite un registro de diagnóstico hacia el sumidero global persistente
pub fn log_record(record: &DiagnosticRecordDto) {
    if let Some(sink) = GLOBAL_TRACE_SINK.get() {
        // Un fallo del propio sumidero no tiene un canal más fiable donde informarse
        let _ = sink.record(record);
    }
}

/// R-17: cuerpo único de los registros por severidad
fn emit(
    severity: DiagnosticSeverity,
    category: DiagnosticCategory,
    event_name: &'static str,
    message: String,
    win32_code: Option<u32>,
) {
    log_record(&DiagnosticRecordDto {
        timestamp_utc_iso8601: format_iso8601_now(),
        process_id: unsafe { GetCurrentProcessId() },
        thread_id: unsafe { GetCurrentThreadId() as u64 },
        severity,
        category,
        event_name,
        message,
        win32_code,
        file: None,
        line: None,
    });
}

/// Registro estructurado de advertencia (WARN)
pub fn log_warn(
    category: DiagnosticCategory,
    event_name: &'static str,
    message: impl Into<String>,
    win32_code: Option<u32>,
) {
    emit(DiagnosticSeverity::Warning, category, event_name, message.into(), win32_code);
}

/// Registro estructurado de error recuperable (ERROR)
pub fn log_error(
    category: DiagnosticCategory,
    event_name: &'static str,
    message: impl Into<String>,
    win32_code: Option<u32>,
) {
    emit(DiagnosticSeverity::Error, category, event_name, message.into(), win32_code);
}

/// Registro de fallo crítico irrecuperable (CRITICAL, categoría Crash)
pub fn log_fatal(event_name: &'static str, message: impl Into<String>, win32_code: Option<u32>) {
    emit(DiagnosticSeverity::Critical, DiagnosticCategory::Crash, event_name, message.into(), win32_code);
}

/// Despliega un cuadro de diálogo nativo Win32 (MessageBoxW) ante un fallo irrecuperable
pub fn show_fatal_dialog(title: &str, message: &str) {
    let title_wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let msg_wide: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        MessageBoxW(
            0,
            msg_wide.as_ptr(),
            title_wide.as_ptr(),
            MB_ICONERROR | MB_OK,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(message: &str) -> DiagnosticRecordDto {
        DiagnosticRecordDto {
            timestamp_utc_iso8601: "2026-10-23T22:00:00.000Z".to_string(),
            process_id: 1234,
            thread_id: 5678,
            severity: DiagnosticSeverity::Info,
            category: DiagnosticCategory::OsEvent,
            event_name: "WM_SIZE",
            message: message.to_string(),
            win32_code: None,
            file: Some("src/window/mod.rs"),
            line: Some(250),
        }
    }

    #[test]
    fn test_diagnostic_severity_and_iso8601_formatting() {
        let iso = format_iso8601_now();
        assert!(iso.ends_with('Z'));
        assert_eq!(iso.chars().nth(4), Some('-'));
        assert_eq!(iso.chars().nth(7), Some('-'));
        assert_eq!(iso.chars().nth(10), Some('T'));
        assert_eq!(iso.chars().nth(13), Some(':'));
        assert_eq!(iso.chars().nth(16), Some(':'));
        assert_eq!(iso.chars().nth(19), Some('.'));

        assert_eq!(DiagnosticSeverity::Info.to_string(), "INFO");
        assert_eq!(DiagnosticSeverity::Warning.to_string(), "WARN");
        assert_eq!(DiagnosticSeverity::Error.to_string(), "ERROR");
        assert_eq!(DiagnosticSeverity::Critical.to_string(), "CRITICAL");
    }

    #[test]
    fn test_app_trace_log_rotation_and_os_event_capture() {
        let temp_dir = std::env::temp_dir().join(format!("galapagos_test_trace_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        let sink = DiagnosticFileWriter::new(&temp_dir).expect("Crear sumidero de prueba");
        let log_file = sink.log_file_path();

        sink.record(&record("Reajuste dinámico de dimensiones (1024x768)")).expect("Registrar evento");
        sink.flush().expect("Volcado a disco");

        let content = fs::read_to_string(&log_file).expect("Leer archivo de log");
        assert!(content.contains("[2026-10-23T22:00:00.000Z] [INFO] [PID:1234] [TID:5678] [OS_EVENT] [WM_SIZE] Reajuste dinámico de dimensiones (1024x768)"));
        assert!(content.contains("at src/window/mod.rs:250"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    /// INC-20261025-CORE-DEBLOAT P-05: la rotación ocurre durante la sesión, no solo al abrir;
    /// el archivo activo nunca supera el tope y la línea que lo superaría abre un archivo nuevo
    #[test]
    fn test_log_rotates_by_size_within_session_inc20261025() {
        let temp_dir = std::env::temp_dir().join(format!("galapagos_test_rotate_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        let line_len = record("x").to_log_line().len() as u64;
        let limit = line_len * 3;
        let sink = DiagnosticFileWriter::with_limit(&temp_dir, limit).expect("sumidero");

        for _ in 0..7 {
            sink.record(&record("x")).expect("registrar");
        }
        sink.flush().unwrap();

        let main_len = fs::metadata(sink.log_file_path()).unwrap().len();
        let backup_len = fs::metadata(temp_dir.join(DIAGNOSTIC_TRACE_BACKUP_NAME)).unwrap().len();
        assert!(main_len <= limit, "activo {} > tope {}", main_len, limit);
        // 7 líneas con tope de 3: [3] [3] -> .old (sustituido), [1] activo
        assert_eq!(backup_len, line_len * 3);
        assert_eq!(main_len, line_len);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
