//! src/islands/connectivity/mod.rs
//! Tarea F3.2: Connectivity Island — Validación de destinos y de políticas de ruta
//! Estándar ZNVE v2.4.0 (Axiomas 1 y 2, NET-01, SYSTEM_ARCHITECTURE.md, ADR-006).
//!
//! Responsabilidades:
//! 1. Validación previa del destino de navegación (camino N-I1: Connectivity -> Ocean Defense).
//! 2. Validación de `NetworkRoutePolicy` al crear un perfil (host de proxy RFC 1123).
//!
//! R-06 (INC-20261025-CORE-DEBLOAT): retirados el enrutador por perfil y las políticas WebRTC,
//! que nunca se aplicaban a Servo (NET-01 diferido en CONTRACT_BACKLOG.md).

use crate::galapagos_core_v1::{GalapagosError, IpcViolationReason, NetworkRoutePolicy, TabId};

/// Fachada de la Isla de Conectividad (Connectivity Island) en el Trusted Coordinator
#[derive(Debug, Default)]
pub struct ConnectivityIsland;

impl ConnectivityIsland {
    pub fn new() -> Self {
        Self
    }

    /// Valida una política de ruta antes de asociarla a un perfil (host de proxy RFC 1123)
    pub fn validate_route(&self, policy: &NetworkRoutePolicy) -> Result<(), GalapagosError> {
        match policy {
            NetworkRoutePolicy::DirectSystem => Ok(()),
            NetworkRoutePolicy::CustomProxy { host, .. } if NetworkRoutePolicy::is_valid_proxy_host(host) => Ok(()),
            NetworkRoutePolicy::CustomProxy { .. } => Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::MalformedHost,
            }),
        }
    }

    /// Valida un destino de conexión saliente antes del handshake de red
    pub fn validate_destination(&self, host: &str, port: u16) -> bool {
        if host.is_empty() || port == 0 {
            return false;
        }
        if host.starts_with("galapagos://") {
            return true;
        }
        // No permitir caracteres de control o inyecciones en el destino
        !host.chars().any(|c| c.is_ascii_control() || c.is_whitespace())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU16;

    #[test]
    fn test_route_policy_validation() {
        let island = ConnectivityIsland::new();
        assert_eq!(island.validate_route(&NetworkRoutePolicy::DirectSystem), Ok(()));

        let proxy = |host: &str| NetworkRoutePolicy::CustomProxy {
            host: host.to_string(),
            port: NonZeroU16::new(8080).unwrap(),
            enforce_remote_dns: true,
            block_webrtc_leaks: true,
        };
        // Host inválido con guión al inicio -> rechazo
        assert!(island.validate_route(&proxy("-invalid.proxy.lan")).is_err());
        // Host válido RFC 1123 -> aprobado
        assert_eq!(island.validate_route(&proxy("proxy.internal.corp")), Ok(()));
    }

    #[test]
    fn test_destination_validation() {
        let island = ConnectivityIsland::new();
        assert!(island.validate_destination("servo.org", 443));
        assert!(!island.validate_destination("", 443));
        assert!(!island.validate_destination("servo.org", 0));
        assert!(!island.validate_destination("bad\nhost.com", 80));
    }
}
