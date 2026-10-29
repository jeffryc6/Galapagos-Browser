//! src/defense/behavior_sentinel.rs
//! Galapagos Ocean Defense — Behavior Sentinel (MVP)
//! Gobernado bajo estándar ZNVE v2.4.0 (Axiomas 1 y 2, SEC-01, SEC-02).
//! Principio: Shared Signals, Specialized Decisions. Cero IA en el camino crítico.
//!
//! Responsabilidad: detección síncrona en memoria de bucles de redirección rápida
//! (> 10 saltos consecutivos). Al detectarlo emite un `SecurityAlert` y conmuta el veredicto
//! de la pestaña a `Block`.
//!
//! R-10 (INC-20261025-CORE-DEBLOAT): retirados los detectores de inyección masiva de scripts y
//! de criptominería por CPU, que nada alimentaba (el primero además reservaba un `Instant` por
//! script declarado, sin tope).

#![forbid(unsafe_code)]

use crate::defense::request_guard::DefenseDecision;
use crate::galapagos_core_v1::TabId;
use std::collections::{HashMap, VecDeque};

/// Severidad de la alerta de seguridad
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Tipo de anomalía detectada por el centinela de comportamiento
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnomalyKind {
    /// Bucle de redirecciones que excede el umbral seguro
    RedirectionLoop {
        hop_count: u32,
        last_url: String,
    },
}

/// Alerta de seguridad generada ante una transgresión de comportamiento
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityAlert {
    pub alert_id: u64,
    pub tab_id: TabId,
    pub origin: String,
    pub anomaly: AnomalyKind,
    pub severity: AlertSeverity,
    pub action: DefenseDecision,
    pub description: &'static str,
}

/// Configuración de umbrales deterministas para el centinela de comportamiento
#[derive(Debug, Clone)]
pub struct BehaviorConfig {
    /// Máximo de saltos de redirección permitidos antes de considerar bucle (default: 10)
    pub max_redirection_hops: u32,
    /// Capacidad máxima del búfer circular de alertas en memoria (default: 64)
    pub max_alert_history: usize,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            max_redirection_hops: 10,
            max_alert_history: 64,
        }
    }
}

/// Estado interno volátil por pestaña para seguimiento en tiempo real
#[derive(Debug, Clone)]
struct TabState {
    current_origin: String,
    redirection_hops: u32,
    active_verdict: Option<DefenseDecision>,
}

impl TabState {
    fn new(origin: String) -> Self {
        Self {
            current_origin: origin,
            redirection_hops: 0,
            active_verdict: None,
        }
    }
}

/// Centinela de Comportamiento en Memoria de Ocean Defense
pub struct BehaviorSentinel {
    config: BehaviorConfig,
    tabs: HashMap<TabId, TabState>,
    alerts: VecDeque<SecurityAlert>,
    next_alert_id: u64,
}

impl Default for BehaviorSentinel {
    fn default() -> Self {
        Self::new()
    }
}

impl BehaviorSentinel {
    /// Inicializa una nueva instancia de BehaviorSentinel con configuración por defecto
    pub fn new() -> Self {
        Self::with_config(BehaviorConfig::default())
    }

    /// Inicializa con configuración parametrizada
    pub fn with_config(config: BehaviorConfig) -> Self {
        let max_history = config.max_alert_history;
        Self {
            config,
            tabs: HashMap::new(),
            alerts: VecDeque::with_capacity(max_history),
            next_alert_id: 1,
        }
    }

    /// Registra un salto de navegación o redirección HTTP para una pestaña.
    /// Si los saltos superan `max_redirection_hops` (10), conmuta automáticamente a `Block`.
    pub fn record_navigation_hop(
        &mut self,
        tab_id: TabId,
        target_url: &str,
    ) -> Option<SecurityAlert> {
        let state = self
            .tabs
            .entry(tab_id)
            .or_insert_with(|| TabState::new(target_url.to_string()));

        state.redirection_hops = state.redirection_hops.saturating_add(1);

        if state.redirection_hops > self.config.max_redirection_hops {
            let alert_id = self.next_alert_id;
            self.next_alert_id = self.next_alert_id.saturating_add(1);

            let alert = SecurityAlert {
                alert_id,
                tab_id,
                origin: state.current_origin.clone(),
                anomaly: AnomalyKind::RedirectionLoop {
                    hop_count: state.redirection_hops,
                    last_url: target_url.to_string(),
                },
                severity: AlertSeverity::Critical,
                action: DefenseDecision::Block,
                description: "Bucle de redirecciones consecutivas detectado (> 10 saltos). Navegación bloqueada.",
            };

            // Conmutación automática de veredicto a Block
            state.active_verdict = Some(DefenseDecision::Block);
            self.push_alert(alert.clone());
            Some(alert)
        } else {
            None
        }
    }

    /// Reinicia los contadores de redirección ante navegación completada o gesto de usuario
    pub fn reset_navigation_hops(&mut self, tab_id: TabId, new_origin: &str) {
        if let Some(state) = self.tabs.get_mut(&tab_id) {
            state.redirection_hops = 0;
            state.current_origin = new_origin.to_string();
            // Si el veredicto estaba bloqueado solo por bucle de redirección, se restaura
            if state.active_verdict == Some(DefenseDecision::Block) {
                state.active_verdict = None;
            }
        } else {
            self.tabs.insert(tab_id, TabState::new(new_origin.to_string()));
        }
    }

    /// Consulta el veredicto activo forzado por el centinela para una pestaña
    pub fn active_verdict_for_tab(&self, tab_id: TabId) -> Option<DefenseDecision> {
        self.tabs.get(&tab_id).and_then(|s| s.active_verdict)
    }

    /// Deshecho explícito de recursos en memoria al cerrar una pestaña (Invariante ZNVE Anti-Leak)
    pub fn on_tab_closed(&mut self, tab_id: TabId) {
        self.tabs.remove(&tab_id);
    }

    /// Retorna las alertas recientes almacenadas en el búfer circular
    pub fn recent_alerts(&self) -> &VecDeque<SecurityAlert> {
        &self.alerts
    }

    /// Inserción interna en el búfer circular con desalojo de las más antiguas
    fn push_alert(&mut self, alert: SecurityAlert) {
        if self.alerts.len() >= self.config.max_alert_history {
            self.alerts.pop_front();
        }
        self.alerts.push_back(alert);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redirection_loop_threshold_trigger() {
        let mut sentinel = BehaviorSentinel::new();
        let tab = TabId(101);

        // Saltos 1 al 10 deben permitirse sin emitir alerta
        for i in 1..=10 {
            let res = sentinel.record_navigation_hop(tab, &format!("https://hop{}.com", i));
            assert!(res.is_none(), "Salto {} no debería disparar alerta", i);
            assert_eq!(sentinel.active_verdict_for_tab(tab), None);
        }

        // Salto 11 (> 10) debe disparar SecurityAlert con Block
        let alert = sentinel
            .record_navigation_hop(tab, "https://hop11.com")
            .expect("Salto 11 debe disparar alerta de bucle");

        assert_eq!(alert.severity, AlertSeverity::Critical);
        assert_eq!(alert.action, DefenseDecision::Block);
        let AnomalyKind::RedirectionLoop { hop_count, .. } = alert.anomaly;
        assert_eq!(hop_count, 11);

        // Veredicto conmutado automáticamente a Block
        assert_eq!(
            sentinel.active_verdict_for_tab(tab),
            Some(DefenseDecision::Block)
        );

        // Reset restablece el estado
        sentinel.reset_navigation_hops(tab, "https://destination.org");
        assert_eq!(sentinel.active_verdict_for_tab(tab), None);
    }

    #[test]
    fn test_resource_cleanup_on_tab_closed() {
        let mut sentinel = BehaviorSentinel::new();
        let tab = TabId(404);

        sentinel.record_navigation_hop(tab, "https://test.com");
        assert!(sentinel.tabs.contains_key(&tab));

        // Cierre explícito de pestaña
        sentinel.on_tab_closed(tab);
        assert!(!sentinel.tabs.contains_key(&tab));
    }

    #[test]
    fn test_bounded_alert_history() {
        let config = BehaviorConfig {
            max_alert_history: 2,
            max_redirection_hops: 1,
        };
        let mut sentinel = BehaviorSentinel::with_config(config);

        // Generar 3 alertas
        for i in 1..=3 {
            let tab = TabId(i);
            sentinel.record_navigation_hop(tab, "https://a.com");
            sentinel.record_navigation_hop(tab, "https://b.com");
        }

        // Solo deben conservarse las últimas 2
        assert_eq!(sentinel.recent_alerts().len(), 2);
    }
}
