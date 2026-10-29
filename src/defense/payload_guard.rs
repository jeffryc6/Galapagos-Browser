//! src/defense/payload_guard.rs
//! Galapagos Ocean Defense — Payload & Download Guard (F4.2)
//! Gobernado bajo estándar ZNVE v2.4.0 (Axiomas 1 y 2, SEC-01, SEC-02, DEFENSE_POLICY_MODEL.md).
//!
//! Responsabilidades:
//! 1. Inspección de Cabeceras MIME: Detección de discrepancias entre Content-Type anunciado
//!    y la firma de bytes inicial (magic bytes) para prevenir MIME-Confusion y binarios camuflados.
//! 2. Barrera de Binarios Peligrosos: Suspensión síncrona con veredicto RequireConfirmation
//!    o Block ante ejecutables (.exe, .dll, .bat, .ps1, .scr, .msi, etc.).
//! 3. Marcado de Seguridad Win32 (Mark of the Web): Aplicación obligatoria del Alternate Data
//!    Stream `Zone.Identifier` (`[ZoneTransfer]\r\nZoneId=3\r\n`) en todo archivo descargado.

#![forbid(unsafe_code)]

use crate::defense::request_guard::{DefenseDecision, DefenseEvaluation};
use std::fs;
use std::path::Path;

/// Lista inmutable de extensiones de archivo ejecutables de alto riesgo en Windows
const DANGEROUS_EXTENSIONS: &[&str] = &[
    ".exe", ".dll", ".bat", ".cmd", ".scr", ".ps1", ".msi", ".vbs", ".vbe", ".wsf", ".wsh",
    ".jar", ".com", ".cpl", ".pif", ".hta",
];

/// Resultado detallado de la inspección de cabeceras MIME y magic bytes
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MimeInspectionResult {
    /// La firma inicial de bytes coincide perfectamente con el tipo MIME anunciado
    Consistent { detected_kind: &'static str },
    /// Los magic bytes revelan un binario ejecutable PE camuflado como documento o imagen
    DisguisedExecutable {
        declared_mime: String,
        detected_kind: &'static str,
    },
    /// Discrepancia entre el MIME anunciado y la firma de datos reconocida
    MimeMismatch {
        declared_mime: String,
        detected_kind: &'static str,
    },
    /// Tipo de archivo sin magic bytes binarios específicos conocidos
    UnknownOrUnchecked,
}

/// Guardián de Carga Útil y Descargas de Galapagos Ocean Defense
#[derive(Debug, Default, Clone)]
pub struct PayloadGuard;

impl PayloadGuard {
    /// Inicializa una instancia de PayloadGuard
    pub fn new() -> Self {
        Self
    }

    /// Inspecciona el nombre de archivo, el Content-Type declarado y los primeros bytes del archivo
    pub fn inspect_payload(
        &self,
        filename: &str,
        declared_mime: Option<&str>,
        initial_bytes: &[u8],
    ) -> DefenseEvaluation {
        let lower_name = filename.to_ascii_lowercase();

        // 1. Verificación por extensión de archivo (Barrera de Binarios Peligrosos)
        for ext in DANGEROUS_EXTENSIONS {
            if lower_name.ends_with(ext) {
                return DefenseEvaluation {
                    decision: DefenseDecision::RequireConfirmation,
                    reason_code: "EXECUTABLE_PAYLOAD_CONFIRMATION_REQUIRED",
                    applied_rule: "RULE-DOWNLOAD-01-EXECUTABLE-BARRIER",
                };
            }
        }

        // 2. Inspección de firmas binarias (Magic Bytes) y detección de binarios camuflados
        let inspection = self.detect_mime_mismatch(declared_mime, initial_bytes);

        match inspection {
            MimeInspectionResult::DisguisedExecutable { .. } => {
                // Binario ejecutable disfrazado de archivo inocuo -> Bloqueo preventivo crítico
                DefenseEvaluation {
                    decision: DefenseDecision::Block,
                    reason_code: "DISGUISED_EXECUTABLE_PAYLOAD_BLOCKED",
                    applied_rule: "RULE-PAYLOAD-02-ANTI-MIME-CONFUSION",
                }
            }
            MimeInspectionResult::MimeMismatch { .. } => {
                // Discrepancia genérica -> Requiere confirmación por potencial exploit de parser
                DefenseEvaluation {
                    decision: DefenseDecision::RequireConfirmation,
                    reason_code: "MIME_SIGNATURE_MISMATCH_SUSPICIOUS",
                    applied_rule: "RULE-PAYLOAD-03-SIGNATURE-VERIFICATION",
                }
            }
            MimeInspectionResult::Consistent { .. } | MimeInspectionResult::UnknownOrUnchecked => {
                // Si los bytes iniciales son 'MZ' de Windows PE aunque el nombre no termine en .exe
                if initial_bytes.len() >= 2 && initial_bytes[0] == 0x4D && initial_bytes[1] == 0x5A {
                    return DefenseEvaluation {
                        decision: DefenseDecision::RequireConfirmation,
                        reason_code: "EXECUTABLE_MAGIC_BYTES_DETECTED",
                        applied_rule: "RULE-PAYLOAD-01-PE-HEADER-CHECK",
                    };
                }

                DefenseEvaluation {
                    decision: DefenseDecision::Allow,
                    reason_code: "SAFE_PAYLOAD_APPROVED",
                    applied_rule: "RULE-DOWNLOAD-00-DEFAULT-PERMITTED",
                }
            }
        }
    }

    /// Detecta discrepancias de MIME mediante firmas de bytes iniciales
    pub fn detect_mime_mismatch(
        &self,
        declared_mime: Option<&str>,
        bytes: &[u8],
    ) -> MimeInspectionResult {
        if bytes.is_empty() {
            return MimeInspectionResult::UnknownOrUnchecked;
        }

        let is_pe = bytes.len() >= 2 && bytes[0] == 0x4D && bytes[1] == 0x5A; // "MZ"
        let is_png = bytes.len() >= 8
            && bytes[0] == 0x89
            && bytes[1] == 0x50
            && bytes[2] == 0x4E
            && bytes[3] == 0x47
            && bytes[4] == 0x0D
            && bytes[5] == 0x0A
            && bytes[6] == 0x1A
            && bytes[7] == 0x0A;
        let is_jpeg = bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF;
        let is_gif = bytes.len() >= 6
            && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"));
        let is_pdf = bytes.len() >= 5 && bytes.starts_with(b"%PDF-");
        let is_zip = bytes.len() >= 4 && bytes[0] == 0x50 && bytes[1] == 0x4B && bytes[2] == 0x03 && bytes[3] == 0x04;

        if let Some(mime) = declared_mime {
            let lower_mime = mime.to_ascii_lowercase();

            // Si es un ejecutable 'MZ' anunciado como imagen o texto
            if is_pe
                && (lower_mime.starts_with("image/")
                    || lower_mime.starts_with("text/")
                    || lower_mime == "application/pdf")
            {
                return MimeInspectionResult::DisguisedExecutable {
                    declared_mime: mime.to_string(),
                    detected_kind: "application/vnd.microsoft.portable-executable",
                };
            }

            // Validar consistencias esperadas
            if lower_mime.contains("png") {
                if is_png {
                    return MimeInspectionResult::Consistent {
                        detected_kind: "image/png",
                    };
                } else if is_jpeg || is_gif || is_pdf || is_zip || is_pe {
                    return MimeInspectionResult::MimeMismatch {
                        declared_mime: mime.to_string(),
                        detected_kind: "non-png-binary",
                    };
                }
            } else if lower_mime.contains("jpeg") || lower_mime.contains("jpg") {
                if is_jpeg {
                    return MimeInspectionResult::Consistent {
                        detected_kind: "image/jpeg",
                    };
                } else if is_png || is_gif || is_pdf || is_zip || is_pe {
                    return MimeInspectionResult::MimeMismatch {
                        declared_mime: mime.to_string(),
                        detected_kind: "non-jpeg-binary",
                    };
                }
            } else if lower_mime.contains("pdf") {
                if is_pdf {
                    return MimeInspectionResult::Consistent {
                        detected_kind: "application/pdf",
                    };
                } else if is_png || is_jpeg || is_zip || is_pe {
                    return MimeInspectionResult::MimeMismatch {
                        declared_mime: mime.to_string(),
                        detected_kind: "non-pdf-binary",
                    };
                }
            }
        }

        if is_pe {
            MimeInspectionResult::Consistent {
                detected_kind: "application/vnd.microsoft.portable-executable",
            }
        } else if is_png {
            MimeInspectionResult::Consistent {
                detected_kind: "image/png",
            }
        } else if is_jpeg {
            MimeInspectionResult::Consistent {
                detected_kind: "image/jpeg",
            }
        } else if is_pdf {
            MimeInspectionResult::Consistent {
                detected_kind: "application/pdf",
            }
        } else {
            MimeInspectionResult::UnknownOrUnchecked
        }
    }

    /// Aplica el Mark of the Web (Zone.Identifier con ZoneId=3 URLZONE_INTERNET) en Windows NTFS
    /// Conforme al estándar ZNVE de integración nativa con el sistema operativo
    pub fn apply_mark_of_the_web(&self, file_path: &Path) -> std::io::Result<()> {
        let ads_path_str = format!("{}:Zone.Identifier", file_path.to_string_lossy());
        let ads_path = Path::new(&ads_path_str);
        let zone_content = b"[ZoneTransfer]\r\nZoneId=3\r\n";
        fs::write(ads_path, zone_content)
    }

    /// Lee y verifica la presencia del Mark of the Web en un archivo descargado
    pub fn read_mark_of_the_web(&self, file_path: &Path) -> std::io::Result<String> {
        let ads_path_str = format!("{}:Zone.Identifier", file_path.to_string_lossy());
        let ads_path = Path::new(&ads_path_str);
        fs::read_to_string(ads_path)
    }

    /// Verifica si un archivo posee la etiqueta ZoneId=3 (Internet)
    pub fn has_internet_mark_of_the_web(&self, file_path: &Path) -> bool {
        self.read_mark_of_the_web(file_path)
            .map(|content| content.contains("ZoneId=3"))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_executable_extension_barrier() {
        let guard = PayloadGuard::new();

        // Archivos ejecutables deben exigir confirmación
        let dangerous_names = ["setup.exe", "library.dll", "script.bat", "powershell.ps1", "installer.msi"];
        for name in dangerous_names {
            let eval = guard.inspect_payload(name, Some("application/octet-stream"), &[]);
            assert_eq!(
                eval.decision,
                DefenseDecision::RequireConfirmation,
                "Archivo {} debe exigir confirmación",
                name
            );
            assert_eq!(eval.reason_code, "EXECUTABLE_PAYLOAD_CONFIRMATION_REQUIRED");
        }

        // Archivo seguro
        let eval = guard.inspect_payload("document.pdf", Some("application/pdf"), b"%PDF-1.7");
        assert_eq!(eval.decision, DefenseDecision::Allow);
    }

    #[test]
    fn test_disguised_executable_mime_confusion_blocked() {
        let guard = PayloadGuard::new();

        // Archivo con extensión .png y Content-Type image/png pero cuyos bytes son MZ (PE Executable)
        let pe_bytes = [0x4D, 0x5A, 0x90, 0x00, 0x03, 0x00, 0x00, 0x00];
        let eval = guard.inspect_payload("cute_cat.png", Some("image/png"), &pe_bytes);

        // Debe ser BLOQUEADO de inmediato
        assert_eq!(eval.decision, DefenseDecision::Block);
        assert_eq!(eval.reason_code, "DISGUISED_EXECUTABLE_PAYLOAD_BLOCKED");
    }

    #[test]
    fn test_mime_mismatch_detection() {
        let guard = PayloadGuard::new();

        // Anunciado como image/png pero los bytes son JPEG
        let jpeg_bytes = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let eval = guard.inspect_payload("photo.png", Some("image/png"), &jpeg_bytes);

        assert_eq!(eval.decision, DefenseDecision::RequireConfirmation);
        assert_eq!(eval.reason_code, "MIME_SIGNATURE_MISMATCH_SUSPICIOUS");
    }

    #[test]
    fn test_win32_mark_of_the_web_application() {
        let guard = PayloadGuard::new();
        let temp_dir = env::temp_dir();
        let test_file = temp_dir.join("galapagos_motw_test.bin");

        // Crear archivo temporal
        fs::write(&test_file, b"sample untrusted payload from internet").unwrap();

        // Aplicar Mark of the Web
        guard.apply_mark_of_the_web(&test_file).expect("Mark of the web debe escribirse en NTFS");

        // Verificar que el stream contiene ZoneId=3
        let motw = guard.read_mark_of_the_web(&test_file).expect("Zone.Identifier stream legible");
        assert!(motw.contains("[ZoneTransfer]"));
        assert!(motw.contains("ZoneId=3"));
        assert!(guard.has_internet_mark_of_the_web(&test_file));

        // Limpieza de prueba
        let _ = fs::remove_file(&test_file);
    }
}
