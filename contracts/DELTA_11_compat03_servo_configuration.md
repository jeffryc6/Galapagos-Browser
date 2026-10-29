# DELTA 11 — COMPAT-03: configuración de Servo en el worker

**Estado:** CONGELADO (2026-10-27) · aprobadas D47–D51 y D53; **D52 (webgl) rechazada** por el Director · **Origen:** [FORENSIC_SERVO_CONFIG_2026-10-27.md](FORENSIC_SERVO_CONFIG_2026-10-27.md), palancas 1–4 · **Extiende:** DELTA_10 (D43 se modifica con D49), DELTA_01/05 (gancho de pánico), contrato IPC (`galapagos-ipc`) · **Validación:** arnés COMPAT ([HARNESS.md](../tests/characterization/compat/HARNESS.md)) con diferencias **previstas de antemano** (§6)

## 0. Verificación adicional al forense (solo lectura, 2026-10-27)

Desactivar `hard_fail` **no basta**. Así recupera Servo el pánico de una página [O]:
1. El hilo de script (`Script#N`, `script/event_loop/script_thread.rs:489`) entra en pánico. Ese hilo tiene instalado un `ScriptEventLoopId` (`:498`).
2. Un gancho de pánico del embebedor emite `log::error!` mientras `thread::panicking()`. servoshell lo hace en `ports/servoshell/panic_hook.rs:52`.
3. El logger que instala `Servo::setup_logging()` (`servo/servo.rs:1081`) convierte ese registro en `LogEntry::Panic` (`constellation/logging.rs:121`) y lo envía a la constelación.
4. `handle_log_entry` → `handle_panic` (`constellation.rs:3168`). Con `hard_fail = true` llama a `process::exit(1)` (`:3020`). Con `false`, sustituye el documento por la página de fallo (`:3092–3122`) y avisa con `notify_crashed` (`servo.rs:861`).

**Hoy el worker no hace ni el paso 2 ni el 3.** El pánico de un hilo de script solo dispara nuestro gancho (`WORKER_FAULT`), y en la ventana «Motor falló» queda fijo: `set_engine_fault` anula además el estado de navegación. El aviso D43 (`notify_crashed` por `MSG_WORKER_FAULT`) dejaría la ventana en ese mismo estado aunque el worker siga vivo.

## 1. CONTRATO DE ENTRADA Y SALIDA (Cubo A)

### 1.1 Recuperación de pánicos de página (palanca 1: D47, D48, D49)

```rust
// crates/galapagos-content/src/main.rs
let servo = servo::ServoBuilder::default()
    .opts(servo::Opts { hard_fail: false, ..servo::Opts::default() })   // D47
    .preferences(worker_preferences())
    .event_loop_waker(Box::new(waker.clone()))
    .build();
servo.setup_logging();                                                  // D47

/// D48: solo el pánico de un hilo de script lo recupera Servo (página de fallo + notify_crashed);
/// el resto (hilo principal, constelación, red, pintado) sigue siendo fatal para el worker.
const RECOVERABLE_THREAD_PREFIX: &str = "Script#";
fn panic_is_recoverable(thread_name: Option<&str>) -> bool;
```

**Gancho de pánico (D48):**

| Hilo | Antes | Después |
|---|---|---|
| `Script#N` | `WORKER_FAULT` y gancho por defecto (stderr) | `log::error!("{mensaje}")`, que va a la constelación, y gancho por defecto (stderr). **No** se envía `WORKER_FAULT` ni se activa `FAULT_SENT` |
| Cualquier otro | `WORKER_FAULT` y gancho por defecto | Sin cambios |

**Aviso de página caída (D49, modifica D43):** mensaje IPC nuevo, del worker al Coordinador.

| Tipo | Nombre | Payload |
|---|---|---|
| 18 | `MSG_CONTENT_CRASHED` | Texto con pestaña (como los tipos 6–8): `tab_id u64 · len u16 · motivo UTF-8 ≤ 512 B`, saneado con `sanitize_title` |

- `crates/galapagos-ipc`: `pub const MSG_CONTENT_CRASHED: u16 = 18;`.
- `contracts/galapagos_core_v1.rs`: `ContentToCoordinatorIpc::ContentCrashed { tab_id, reason }`, con el mismo códec y las mismas validaciones que `WorkerFault`.
- **Worker:** `OutgoingWorkerEvent::ContentCrashed(texto)` envía el tipo 18 en lugar de `MSG_WORKER_FAULT`. El delegado escribe siempre una línea `[content:crash] <host> <motivo>` (D42/D46), además del backtrace recortado a 8 KiB si lo hay.
- **Coordinador:**
  - `diagnostics::log_warn("CONTENT_CRASHED", …)` en `app_trace.log`;
  - insignia `SecurityBadge::PageCrashed` con el texto «La página falló». **No** es permanente: la siguiente navegación la sustituye;
  - **no** se toca `EngineFault`, así que el estado de navegación sigue vivo.

### 1.2 Features de Cargo (palanca 2: D50, D52)

```toml
# crates/galapagos-content/Cargo.toml
servo = { path = "../../vendor/servo/components/servo", default-features = false,
          features = ["bundled", "multiprocess", "js_jit", "no-wgl", "webcrypto"] }   # D50
log = "0.4"                                                                            # D48
```

- **D52 (opcional, se decide en la aprobación):** añadir `"webgl"` en la misma recompilación completa. Recomendación: **no** en este delta. WebGL abre una superficie de ataque nueva hacia la GPU; mejor un delta propio con su análisis.

### 1.3 Preferencias (palancas 3 y 4: D51)

```rust
fn worker_preferences() -> servo::Preferences {
    servo::Preferences {
        layout_threads: LAYOUT_THREADS,                 // sin cambios (D40 pendiente)
        dom_intersection_observer_enabled: true,        // D51
        layout_container_queries_enabled: true,         // D51
        layout_columns_enabled: true,                   // D51
        ..servo::Preferences::default()
    }
}
```

El test existente `worker_preferences_disable_style_thread_pool_option_b` comprueba `layout_threads == 1` y el User-Agent. Sigue siendo válido: **no se modifica**.

### 1.4 Disparador de pánico para el arnés (D53)

- Solo en compilaciones debug (`#[cfg(debug_assertions)]`): si el worker recibe `GALAPAGOS_TEST_SERVO_HELPERS=1` en su entorno, activa `dom_servo_helpers_enabled`, que expone `ServoTestUtils.panic()` (`webidls/ServoTestUtils.webidl:10`).
- En release, el código no existe. Sin esa variable, nada cambia.
- **Motivo:** es la única forma determinista de provocar un pánico real de página. Sin ella, D47–D49 se declararían **NO INTEGRADOS** (Guardrail 9).

## 2. CONTRATO DE PERSISTENCIA

Sin cambios.

## 3. CONTRATO DE ERRORES

| Modo de fallo | Antes | Después |
|---|---|---|
| Pánico en `Script#N` | Worker muerto, «Motor falló» fijo | Página de fallo de Servo en esa ventana; `[content:crash]` en el log; insignia «La página falló»; el worker sigue vivo |
| Pánico de la página de fallo | — | Servo no reintenta (`constellation.rs:3092`); queda registrado |
| Pánico en otro hilo | `WORKER_FAULT` | Sin cambios (fatal) |
| `MSG_CONTENT_CRASHED` malformado | — | `MalformedMessage`, igual que los tipos 6–8 (canal cerrado, ARC-02) |
| Una preferencia experimental provoca pánicos | Worker muerto | Recuperado por D47–D49 y visible en `[content:crash]`. Si el arnés muestra caídas nuevas, se revierte esa preferencia |

## 4. ANTI-BLOAT FENCE

- **`log = "0.4"`, dependencia directa nueva (excepción justificada):**
  - **Qué resuelve:** es el único canal por el que Servo recibe los pánicos de página (§0, paso 2).
  - **Peso:** cero código nuevo. El crate ya se compila en el árbol (0.4.32, `vendor/servo/Cargo.toml:149`) y la versión se unifica.
  - **Alternativa descartada:** reimplementar el logger de la constelación, que no es público.
- `Servo::setup_logging()` usa `env_logger`, también ya presente: escribe en stderr los `error!` de Servo, es decir, en `worker_stderr.log`, con su tope de 5 MiB. **Riesgo de ruido:** se mide con el arnés y los sitios reales (§6); si supera 50 líneas por carga, va al Cubo B (logger propio).
- **Descartado en este delta:**
  - `webgl` (D52, recomendación no);
  - `:has()` y subgrid (necesitan `stylo_static_prefs` directo o un parche: Cubo B);
  - `media-gstreamer` (dependencia externa);
  - el resto de preferencias experimentales (una a una, en deltas posteriores, con el arnés);
  - `ServoDelegate`.

## 5. Plan de ejecución (un `TARGET_FILE` por paso)

| Paso | TARGET_FILE | Resultado | Verificación |
|---|---|---|---|
| 1 | `crates/galapagos-ipc/src/lib.rs` | `MSG_CONTENT_CRASHED = 18` | `cargo test -q` del crate |
| 2 | `contracts/galapagos_core_v1.rs` + `src/ipc/dispatcher.rs` | Variante `ContentCrashed` y su códec | Test `test_content_crashed_codec_delta11` |
| 3 | `src/window/mod.rs` + `src/main.rs` (Coordinador) | `SecurityBadge::PageCrashed` y tratamiento del tipo 18 | Suite del Coordinador |
| 4 | `crates/galapagos-content/src/delegates.rs` | La línea `[content:crash]` se escribe siempre | Test `content_crash_line_is_logged_delta11` |
| 5 | `crates/galapagos-content/src/main.rs` | D47, D48, D49 (envío del tipo 18), D51 y D53 | Tests `panic_classification_delta11`, `worker_preferences_enable_compat03_delta11` |
| 6 | `crates/galapagos-content/Cargo.toml` | D50 (+ D52 si se aprueba) y `log` | **Recompilación completa del worker en segundo plano (≈1 h)**; luego `cargo test -q` del worker |
| 7 | `tests/characterization/compat/probes/` | `layout.html`: medida `columns` (2 columnas en 300 px con 20 px de separación, que es 140 por columna). `crash.html`: sonda nueva con `ServoTestUtils.panic()` (D53). El ejecutor pasa la variable D53 solo a esa sonda | `Run-CompatHarness.ps1`: solo deben aparecer las diferencias de §6 |
| 8 | — | Recaptura de los snapshots afectados **con la aprobación del Director**, más `-Mode Sites` y prueba con ventana (miniaturas de YouTube) | Arnés sin fallos; métricas en `runs/` |

**Incidencia de ejecución (paso 6, 2026-10-27):**
- **Síntoma:** con `webcrypto`, la compilación falló en `rsa 0.10.0-rc.18` (`E0107` sobre `pkcs1::RsaPrivateKey<'_>`).
- **Causa:** el `Cargo.lock` propio del worker (P-09/BUILD-01) resolvió `pkcs1 0.8.0-rc.5`, una prerelease con un cambio de API que el requisito `^0.8.0-rc.4` de `rsa` admite por semver.
- **Corrección:** `cargo update -p pkcs1 --precise 0.8.0-rc.4`, la versión que fija `vendor/servo/Cargo.lock`. Solo cambia el lockfile: ni dependencias nuevas ni código. Refuerza la urgencia de `BUILD-01` (un único `Cargo.lock` alineado con el de Servo).

## 6. Diferencias previstas en el arnés (todo lo demás debe quedar igual)

| Snapshot | Línea | Antes | Previsto después |
|---|---|---|---|
| `caps` | `caps-dom` | `IntersectionObserver=0` | `IntersectionObserver=1` |
| `caps` | `caps-sec` | `crypto.getRandomValues=E:ReferenceError;crypto.subtle=E:ReferenceError;crypto.randomUUID=E:ReferenceError` | `crypto.getRandomValues=1;crypto.subtle=1;crypto.randomUUID=` (1 si Servo lo implementa; se confirma con el arnés) |
| `caps` | `caps-css` | `css.container=0` | `css.container=1` |
| `layout` | `layout` | `container.w=10` | `container.w=77` (más la medida nueva `columns`) |
| `crash` | (nuevo) | — | `[content:crash] localhost …` y `worker_faults=0` |
| `errors`, `popups` | — | — | Sin cambios |

Si aparece cualquier otra diferencia, se para y se informa antes de recapturar.

## 7. Decisiones para aprobar

| Id | Decisión | Recomendación |
|---|---|---|
| D47 | `Opts { hard_fail: false }` + `servo.setup_logging()` | Sí |
| D48 | Gancho de pánico: `Script#N` recuperable (`log::error!`, sin `WORKER_FAULT`); el resto, fatal. Dependencia directa `log = "0.4"` | Sí |
| D49 | `MSG_CONTENT_CRASHED` (18) en lugar de `MSG_WORKER_FAULT` para D43; insignia «La página falló», no permanente | Sí |
| D50 | Feature `webcrypto` | Sí |
| D51 | `dom_intersection_observer_enabled`, `layout_container_queries_enabled` y `layout_columns_enabled` a `true` | Sí |
| D52 | Feature `webgl` en la misma recompilación | **No** (delta propio) |
| D53 | Disparador de pánico solo en debug (`GALAPAGOS_TEST_SERVO_HELPERS`) para el arnés | Sí |

## 8. Cubo B (al backlog)

| Id | Pendiente |
|---|---|
| `COMPAT-04` | `:has()` y subgrid: preferencias de Stylo fuera de `Preferences` (`stylo_static_prefs` o parche) |
| `COMPAT-05` | Resto de preferencias experimentales (`dom_fontface_enabled`, `dom_indexeddb_enabled` con PERS-05, `layout_variable_fonts_enabled`…), una a una con el arnés |
| `GFX-01` | WebGL/WebGL2 (D52), con su análisis de superficie de ataque |
| `ENG-12` | Registrar `ServoDelegate` (errores internos y consola de los workers) |
| `NET-04` | El proxy no debe tomarse de las variables de entorno (Z-2): fijarlo desde la política del Coordinador |
| `LOG-01` | El Coordinador debería quitar `RUST_LOG` del entorno del worker (con D47, `env_logger` lo respeta y podría inundar el log) |
