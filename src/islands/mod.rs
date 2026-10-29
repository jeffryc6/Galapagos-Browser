//! src/islands/mod.rs
//! Islas Especializadas de Galapagos Browser v0.1
//! Conforme al estándar ZNVE v2.4.0 (Axiomas 1 y 2, SYSTEM_ARCHITECTURE.md).
//! INC-20261025-CORE-DEBLOAT: retiradas Privacy (almacén paralelo que Servo no usa; PRIV-02
//! diferido) y Research (DEV-01 diferido) por no tener ruta de ejecución.

pub mod security;
pub mod performance;
pub mod freedom;
pub mod connectivity;
