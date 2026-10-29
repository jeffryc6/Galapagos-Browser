//! src/islands/freedom/mod.rs
//! Tarea F3.1: Freedom Island — Consentimiento Explícito
//! Estándar ZNVE v2.4.0 (Axiomas 1 y 2, SEC-02, DEFENSE_POLICY_MODEL.md, ADR-004).
//!
//! Responsabilidad: diálogo modal nativo Win32 (MessageBoxW) ante decisiones
//! `RequireConfirmation` de Ocean Defense. Sin ventana anfitriona, el rechazo es determinista.
//!
//! R-08 (INC-20261025-CORE-DEBLOAT): retirada la caché en memoria de permisos por sitio, que
//! duplicaba `site_permissions` de SQLite y no tenía consumidores, y el modo de confirmación
//! automática para pruebas que vivía en el tipo de producción.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONWARNING, MB_YESNO,
};

/// Diálogo modal nativo Win32 para solicitar consentimiento del usuario
pub struct ConsentDialog;

impl ConsentDialog {
    /// Despliega una alerta modal nativa MessageBoxW en Windows solicitando confirmación
    pub fn prompt_native_confirmation(
        parent_hwnd: isize,
        origin: &str,
        action_description: &str,
    ) -> bool {
        let title_wide: Vec<u16> = "Galapagos Browser — Confirmación de Seguridad\0"
            .encode_utf16()
            .collect();
        let prompt_text = format!(
            "El sitio '{}' solicita autorización para:\n{}\n\n¿Desea permitir esta acción?",
            origin, action_description
        );
        let prompt_wide: Vec<u16> = prompt_text
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
            let result = MessageBoxW(
                parent_hwnd as HWND,
                prompt_wide.as_ptr(),
                title_wide.as_ptr(),
                MB_YESNO | MB_ICONWARNING,
            );
            result == IDYES
        }
    }
}

/// Fachada de la Isla de Libertad (Freedom Island) en el Trusted Coordinator
#[derive(Debug, Default)]
pub struct FreedomIsland;

impl FreedomIsland {
    pub fn new() -> Self {
        Self
    }

    /// Evalúa si una acción que requiere confirmación es aprobada por el usuario
    pub fn request_user_confirmation(
        &self,
        parent_hwnd: isize,
        origin: &str,
        action_description: &str,
    ) -> bool {
        if parent_hwnd != 0 {
            ConsentDialog::prompt_native_confirmation(parent_hwnd, origin, action_description)
        } else {
            // Sin ventana anfitriona (headless real): rechazo seguro determinista
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freedom_island_rejects_without_window() {
        let island = FreedomIsland::new();
        // Sin ventana anfitriona: rechazo seguro, sin diálogo
        assert!(!island.request_user_confirmation(0, "https://example.com", "Descarga de archivo"));
    }
}
