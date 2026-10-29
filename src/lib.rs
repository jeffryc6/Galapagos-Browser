//! src/lib.rs - Biblioteca del Coordinador de Confianza de Galapagos Browser v0.1
//! Conforme al estándar ZNVE v2.4.0 (Axiomas 1 y 2, ARC-01, SEC-01, PERF-01).

#[allow(dead_code)]
#[path = "../contracts/galapagos_core_v1.rs"]
pub mod galapagos_core_v1;

pub mod coordinator;
pub mod defense;
pub mod diagnostics;
pub mod engine;
pub mod ipc;
pub mod islands;
pub mod storage;
pub mod window;

pub use coordinator::Coordinator;

