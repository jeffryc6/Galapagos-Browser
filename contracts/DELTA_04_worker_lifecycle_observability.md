# DELTA 04 — Ciclo de vida del worker, observabilidad y arranque sin simulacros

**Estado:** CONGELADO (2026-10-25) · D12, D13, D14, D15 aprobadas por el Director (recomendaciones) · **Origen:** informe `/znve-audit` pegado por el Director + verificación independiente (log `app_trace.log` del 2026-10-25 19:37 UTC, código y sondas) · **Extiende:** `DELTA_01` (H1-H7), `DELTA_02`, `DELTA_03` (solo aditivo; ninguna firma pública ni variante existente cambia)

## 0. Verificación del informe contra la evidencia

| Afirmación del informe | Resultado | Evidencia |
|---|---|---|
| **P1** el worker muere por `ERROR_MORE_DATA` al leer la cabecera de 6 bytes en un pipe de mensajes | **REFUTADA** | Sonda real (test temporal, ya borrado): servidor con `PIPE_TYPE_MESSAGE\|PIPE_READMODE_MESSAGE` como `channel.rs`, cliente abierto con `CreateFileW` sin `SetNamedPipeHandleState` como el worker. `ReadFile(6 bytes)` → `ok=1 n=6`; la lectura del resto → `ok=1 n=16`. El cliente lee en modo byte; no hay `ERROR_MORE_DATA`. El worker actual (paso 4) además inspecciona con `PeekNamedPipe` antes de leer. **No se adopta P1.** |
| El canal cae ~600 ms tras el arranque | **CONFIRMADA** | Log: `WORKER_BOUND` 19:37:13.042, `CONTENT_CHANNEL_VIOLATION` (ChannelTimeout = `PeekNamedPipe` falló) 19:37:13.677, luego `NAVIGATION_DISPATCH_FAILED` en cada intento |
| La causa es un fallo de lectura IPC | **NO DEMOSTRADA** | Nada en el log distingue "el worker terminó/falló al iniciar Servo" de "cerró el pipe". El Coordinador no registra el código de salida del worker. La causa raíz sigue **sin identificar** |
| El worker usado es el actual | **FALSA** | `find_worker_binary` prefiere `target/debug/galapagos-content.exe` (2026-10-23 21:34, anterior a los pasos 3-4); la reconstrucción vigente está en `crates/galapagos-content/target/debug/` (2026-10-25 12:30). La corrida del log usó el binario viejo |
| `engine-stub` activo por defecto contamina producción | **CONFIRMADA** | `Cargo.toml: default = ["engine-stub"]`; `ServoEngineAdapter::new` cae a `Stub` (PID falso = el del Coordinador) si no halla el worker en un binario no-test. Contradice H1 y H7 y el Guardrail 9 |
| `send_frame` oculta el error Win32 | **CONFIRMADA** | `ipc/channel.rs`: todo fallo de `WriteFile` → `MalformedMessage`, sin `GetLastError()` |
| Job Object duplicado | **CONFIRMADA (impacto menor)** | `Coordinator::spawn_tab` y `initialize_sandboxed_worker` crean `Galapagos_TabJob_<tab>`; el objeto del Coordinador nunca recibe el proceso del worker |
| `WM_SIZE` no redimensiona la superficie | **CONFIRMADA** | `window/mod.rs` `wnd_proc`; ya en backlog como `UX-05` |
| El hilo de UI se bloquea al arrancar | **CONFIRMADA** | Log: ventana 19:37:07 → worker enlazado 19:37:13 (≈6 s) por `ConnectNamedPipe` + handshake síncronos |
| Escenario `--smoke-test` y host `phishing.evil.com` en producción | **YA CORREGIDO** (R10) | `main.rs` los carga solo con `--smoke-test`; las líneas citadas son de una versión anterior |
| Bucle de certificación sintético antes de la ventana | **PARCIAL** | La navegación inicial a `https://servo.org` y la muestra de rendimiento con `ProcessId(1)` fijo siguen ejecutándose siempre |

## 1. Auditoría contratos ↔ src

| Id | Discrepancia | Sev. | Cubo |
|---|---|---|---|
| O-1 | Se ignora por qué muere el worker: ni código de salida ni qué binario se lanzó | Alta | A |
| O-2 | `WORKER_BOUND` no registra la ruta ni la fecha del binario | Media | A |
| C-1 | Errores de pipe colapsados en `MalformedMessage` (H3 exige distinguir `ChannelTimeout`) | Alta | A |
| B-1 | Degradación silenciosa a `Stub` en un binario no-test (H1, H7, Guardrail 9) | Alta | A (D15) |
| V-1 | Viewport congelado en 800×554 (`UX-05`) | Media | A |
| S-1 | Navegación y muestra de rendimiento sintéticas al arrancar | Media | A (D12) |
| W-1 | Selección del worker por orden de rutas, no por vigencia | Media | A (D13) |
| J-1 | Job Object duplicado; el del Coordinador es decorativo | Media | B |
| U-1 | Arranque del motor bloquea el hilo de UI ≈6 s (Guardrail 4) | Media | B |

## 2. CONTRATO DE ENTRADA Y SALIDA (Cubo A, aditivo)

```rust
// src/engine/mod.rs — adiciones públicas
impl ServoEngineAdapter {
    /// None si el worker sigue vivo o no existe; Some(código) si ya terminó (GetExitCodeProcess).
    pub fn worker_exit_code(&self, tab_id: TabId) -> Option<u32>;
    /// Ruta absoluta del binario con el que se lanzó el worker de la pestaña.
    pub fn worker_binary(&self, tab_id: TabId) -> Option<&std::path::Path>;
}

// src/window/mod.rs — nuevo evento
UiEvent::Resize { width: u32, height: u32 }   // WM_SIZE del área cliente, > 0
```

### Invariantes
- **L-I1** Al detectar `violation` en `pump_worker`, la traza `CONTENT_CHANNEL_VIOLATION` incluye `worker_exit_code` (o "en ejecución") y la ruta del binario, **antes** de terminar el worker.
- **L-I2** `WORKER_BOUND` registra ruta absoluta y fecha de modificación del binario (UTC, ISO-8601).
- **L-I3** Mapeo de fallos de pipe (sin variantes nuevas): `ERROR_BROKEN_PIPE`, `ERROR_NO_DATA`, `ERROR_PIPE_NOT_CONNECTED` ⇒ `IpcBoundaryViolation { ChannelTimeout }`; `ERROR_MORE_DATA` y tramas inválidas ⇒ `MalformedMessage`. Todo fallo registra además el código Win32 (`log_error`, categoría `Ipc`).
- **L-I4** (D15) Un binario no-test **nunca** instancia `Stub` por omisión: sin worker localizable, `new()` sigue siendo `Real` y `initialize_sandboxed_worker` devuelve `SandboxEnforcementFailed { win32_error_code: 2 }`. `Stub` solo se obtiene con `with_backend(.., EngineBackend::Stub)` explícito (pruebas).
- **L-I5** `WM_SIZE` con ancho y alto > 0 emite `UiEvent::Resize`; `main` llama a `resize_viewport(tab, w, h)`. Minimizar (0×0) no emite evento.
- **L-I6** (D12) La muestra de rendimiento con `ProcessId` fijo y la navegación de arranque a un sitio externo solo existen bajo `--smoke-test`; el arranque normal abre la URL de la CLI o `about:blank`.
- **L-I7** (D13) Selección de worker: todos los candidatos existentes se evalúan y se elige el de **modificación más reciente**; el elegido se registra (L-I2).

## 3. CONTRATO DE PERSISTENCIA

Sin cambios.

## 4. CONTRATO DE ERRORES

Sin variantes nuevas; ver L-I3 y L-I4.

## 5. ANTI-BLOAT FENCE

- **Entra:** dos accesores, un evento de UI y un mapeo de errores. Cero dependencias.
- **Descartado:** reescribir la lectura IPC del worker (P1 refutado); variantes `BrokenPipe`/`WorkerCrashed` en `GalapagosError` (obliga a tocar todos los `match` del contrato); espera con `sleep` para tapar la carrera de arranque.
- **Prohibido:** reintroducir `Stub` por omisión; ocultar el código de salida del worker.

## 6. Plan de ejecución (un `TARGET_FILE` por paso)

| Paso | TARGET_FILE | Cubre |
|---|---|---|
| 1 | `src/ipc/channel.rs` (mapeo L-I3 + traza del código Win32) | C-1 |
| 2 | `src/engine/mod.rs` (`worker_exit_code`, `worker_binary`, selección L-I7, `new()` sin Stub L-I4) | O-1, O-2, W-1, B-1 |
| 3 | `src/window/mod.rs` (`UiEvent::Resize`) | V-1 |
| 4 | `src/main.rs` (traza L-I1/L-I2, `Resize`→`resize_viewport`, L-I6) | O-1, O-2, V-1, S-1 |
| 5 | **Diagnóstico con el binario real** (ver §8): reproducir con el worker vigente y leer el código de salida | causa raíz del canal caído |

**Ejecución (2026-10-25):** pasos 1-4 hechos; `cargo test` completo en verde (91 lib, 93 bin, 6 y 3 de integración). Paso 5 pendiente del Director. Se añadió `diagnostics::format_iso8601(SystemTime)` (L-I2) y se retiró la traza rutinaria `WM_SIZE` (Guardrail 2). Con L-I7, `find_worker_binary` ya no prefiere `target/debug/` sino el worker más reciente.

## 7. Diferido (Cubo B → `CONTRACT_BACKLOG.md`)

| Código | Descripción |
|---|---|
| `ENG-08` | Consolidar el Job Object en un único dueño (hoy `Coordinator` y `ServoEngineAdapter` crean el mismo objeto con nombre). Toca R8 y la Security Island. |
| `ENG-09` | Arrancar el worker sin bloquear el hilo de UI (`ConnectNamedPipe` + handshake ≈6 s). Requiere hilo o espera asíncrona. |

## 8. Nota de verificación (Guardrail 9)

Este delta **no** demuestra que la navegación funcione. Hasta que el paso 5 se ejecute con el binario real y el worker vigente, el estado sigue siendo **NO INTEGRADO**. La corrida del log del 2026-10-25 usó un worker anterior a los pasos 3-4 de DELTA_02, así que no es evidencia sobre el código actual.

## 9. Decisiones para el Director

| # | Decisión | Recomendación |
|---|---|---|
| D12 | Página de inicio: `about:blank` (o URL de la CLI) vs. `https://servo.org` | `about:blank` |
| D13 | Selección del worker: la más reciente por fecha (L-I7) vs. solo registrar la elegida | La más reciente |
| D14 | Política de pruebas del delta: el informe pegado pide "cero código de test". ZNVE exige test de regresión en hotfix y verificación atómica en execute | Pruebas solo para lógica pura (mapeo de errores, selección de worker); sin tests nuevos que lancen procesos |
| D15 | `new()` siempre `Real` en binarios no-test. Efecto: `tests/production_certification.rs` (no se modifica) dejará de usar el `Stub` de respaldo y necesitará el worker compilado | Sí, es lo que exige el Guardrail 9; compilar el worker antes de `cargo test` |

**Contrato Δ4 en borrador. Responde D12-D15 (o acepta las recomendaciones) para congelarlo y pasar a /znve-execute.**
