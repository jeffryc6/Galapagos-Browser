//! src/defense/mod.rs
//! Galapagos Ocean Defense — Capa Transversal de Políticas Defensivas (MVP)
//! Gobernado bajo estándar ZNVE v2.4.0 (Bloque 1.5 y 1.6 de la especificación maestra).
//! Principio: Shared Signals, Specialized Decisions. Cero IA en el camino crítico.

#![forbid(unsafe_code)]

pub mod behavior_sentinel;
pub mod payload_guard;
pub mod request_guard;
pub mod threat_intel;

pub use behavior_sentinel::{
    AlertSeverity, AnomalyKind, BehaviorConfig, BehaviorSentinel, SecurityAlert,
};
pub use payload_guard::{MimeInspectionResult, PayloadGuard};
pub use request_guard::{
    DefenseAction, DefenseContext, DefenseDecision, DefenseEvaluation, RequestGuard,
};
pub use threat_intel::{
    compute_sha256, hex_encode, ThreatFeedUpdateSummary, ThreatIntelFeedUpdater,
    GALAPAGOS_MASTER_PUBKEY,
};
