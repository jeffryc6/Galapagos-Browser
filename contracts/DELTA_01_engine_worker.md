# DELTA 01 — Motor Servo real y worker confinado

**Estado:** CONGELADO (2026-10-23) · **Incidente origen:** INC-20261023-001 · **Extiende:** `contracts/galapagos_core_v1.rs` (solo aditivo; ninguna firma ni variante existente cambia)
**ADR relacionados:** ADR-001 · `docs/research/SERVO_WINDOWS_FEASIBILITY.md`

## 0. Decisiones del Director (opción recomendada)

| # | Decisión | Resolución |
|---|---|---|
| D1 | Sandbox del worker | AppContainer + Job Object, **sin ACG** (SpiderMonkey JIT activo). `ProhibitDynamicCode` queda fuera del nivel `AppContainerStrict` y se reserva a un futuro nivel de seguridad "Strict" con JIT desactivado (backlog ENG-05). |
| D2 | Fuente de Servo | Copia vendorizada en `vendor/servo` (versión y hash a registrar en `contracts/SERVO_VENDOR.md`). **Precondición abierta:** el Director debe proveer la copia; Δ1.1 en adelante no se ejecutan sin ella. |
| D3 | Conflicto SQLite (`links = "sqlite3"`) | El worker es un crate **aparte** con su propio `Cargo.lock` (`crates/galapagos-content/`), fuera del workspace del coordinador. Coordinador conserva `rusqlite`; el worker no depende de storage. |
| D4 | Alcance del hito 1 | Worker real que arranca confinado, completa handshake y carga `about:blank`. Sin render, sin input, sin red. |

## 1. Auditoría contratos ↔ src (resumen)

| Id | Discrepancia | Sev. | Cubo |
|---|---|---|---|
| D-1 | `initialize_sandboxed_worker` no crea proceso; `ProcessId` sale de un contador | Crítica | A |
| D-2 | No existe `galapagos-content`; Servo no es dependencia; `servo_ffi.rs` es simulacro in-process | Crítica | A |
| D-3 | `sandbox` solo se almacena, no se aplica | Alta | A |
| D-4 | Sin proceso hostil al otro extremo del IPC | Alta | A |
| D-5 | ACG incompatible con JIT de SpiderMonkey | Alta | A (resuelto por D1) |
| D-6 | Mitigaciones: falla solo si fallan las 3 | Media | A — **CERRADO** en `src/islands/security/mod.rs` (`evaluate_mitigation_results`, test de regresión) |

## 2. CONTRATO DE ENTRADA Y SALIDA (aditivo)

```rust
// contracts/galapagos_core_v1.rs — adiciones

/// Backend del motor. `Stub` solo existe con cfg(test) o feature "engine-stub".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineBackend { Real, Stub }

/// Primer mensaje worker → coordinador (MSG_TYPE_HANDSHAKE = 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerHandshake {
    pub process_id: ProcessId,                   // PID real (GetCurrentProcessId en el worker)
    pub applied_sandbox: WindowsSandboxLevel,    // nivel REALMENTE aplicado
    pub protocol_version: NonZeroU16,            // v1 = 1
}
```

Invariantes:
- **H1** `ProcessId` devuelto por `initialize_sandboxed_worker` == `WorkerHandshake.process_id` == PID del proceso hijo creado. Un contador nunca es un PID.
- **H2** Si `applied_sandbox` < nivel solicitado → el coordinador mata el proceso y devuelve `SandboxEnforcementFailed`. Sin degradación silenciosa.
- **H3** Handshake con timeout ≤ `ResourceQuota.ipc_timeout_ms` (≤ 5 000 ms); vencido → `IpcBoundaryViolation { reason: ChannelTimeout }` y kill.
- **H4** Payload IPC ≤ 1 MiB (framer existente). Handshake duplicado o posterior a la fase inicial → `UnauthorizedCommand`.
- **H5** Orden de arranque: `CreateProcessW(CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT)` → asignar a Job Object → `ResumeThread` → esperar handshake. Cualquier fallo intermedio termina el proceso y libera handles (RAII).
- **H6** El mensaje de arranque de `main.rs` imprime el nivel de `applied_sandbox` recibido, no una constante.
- **H7** Build release sin feature `engine-stub` exige backend `Real`; si no está vinculado, la compilación falla.

## 3. CONTRATO DE PERSISTENCIA

Sin cambios. `schema_v1.sql` no se toca; el worker no accede a SQLite.

## 4. CONTRATO DE ERRORES

Sin variantes nuevas. Mapeo:
- Fallo de CreateProcess/token/Job/AppContainer → `SandboxEnforcementFailed { win32_error_code: GetLastError(), target_level }`
- Handshake ausente/tardío → `IpcBoundaryViolation { ChannelTimeout }`
- Handshake malformado o de nivel menor → `IpcBoundaryViolation { MalformedMessage }` / `SandboxEnforcementFailed`
- Worker supera cuota (Job Object) → `ResourceQuotaExceeded` (ya existente)

## 5. ANTI-BLOAT FENCE

- **Entra:** crate `servo` (vendorizado, versión fijada), **solo** en `crates/galapagos-content`. Justificación: motor decidido en ADR-001; sin alternativa nativa. Costo: cientos de crates transitivos, compilación pesada; aislado del coordinador por D3.
- **Prohibido:** `tokio`/async runtime en el coordinador; crates de sandboxing de terceros (`windows-sys` basta); cualquier dependencia nueva en el crate raíz.
- **Descartado:** CEF, WebView2 (ADR-001); worker in-process.
- **Campos descartados:** versionado dinámico de protocolo (solo v1), negociación de capacidades en el handshake.

## 6. Plan de ejecución (un `TARGET_FILE` por paso)

| Paso | TARGET_FILE | Depende de |
|---|---|---|
| 0 | `src/islands/security/mod.rs` (D-6) | — **HECHO** |
| 1 | `contracts/galapagos_core_v1.rs` (EngineBackend, WorkerHandshake) | contrato |
| 2 | `src/ipc/dispatcher.rs` (MSG_TYPE_HANDSHAKE, codec) | 1 |
| 3 | `src/engine/servo_ffi.rs` (detrás de `engine-stub`) + `Cargo.toml` (feature) | 1 |
| 4 | `src/engine/mod.rs` (spawn real, H1–H5) | 2, 3 |
| 5 | `src/main.rs` (H6) | 4 |
| 6 | `crates/galapagos-content/*` (worker mínimo) | **D2 provisto** |
| 7 | `tests/` nuevos: PID real ≠ PID coordinador; kill por cuota; rechazo por nivel menor | 4, 6 |

Pasos 1–5 pueden ejecutarse con un worker de prueba sin Servo (handshake only); el paso 6 requiere `vendor/servo`.

## 7. Diferido (Cubo B → `CONTRACT_BACKLOG.md`)

ENG-02 transporte de frames entre procesos · ENG-03 site isolation · ENG-04 GPU en AppContainer / fallback Low IL · ENG-05 nivel "Strict" (JIT off + ACG).

**Contrato Δ1 sólido y cerrado. Listo para /znve-execute.**
