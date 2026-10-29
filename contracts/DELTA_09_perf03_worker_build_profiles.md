# DELTA 09 — PERF-03: perfiles de compilación del worker

**Estado:** CONGELADO (2026-10-26) · D36–D40 aprobadas por el Director, incluido el cambio del test de `layout_threads` con su puerta · **Origen:** «la navegación sigue siendo el mayor desafío»; el Director aporta la documentación de Cargo sobre perfiles, dependencias, rendimiento de compilación y `cargo build` · **Extiende:** `PERF-03` (CONTRACT_BACKLOG, DELTA_06), `P-09` (TRIAGE_INC-20261025-CORE-DEBLOAT) y la mitigación `LAYOUT_THREADS = 1` · **Alcance:** solo compilación y una preferencia de Servo; sin cambios en IPC, datos ni UI

## 0. Línea base (medida, no estimada)

Fuente: [docs/performance/BASELINE_PERF03_DEBUG.md](../docs/performance/BASELINE_PERF03_DEBUG.md), tabla «Tras el hotfix INC-20261026-WORKER-DPI». Mediana de 3 pasadas en frío, worker en debug, 144 DPI:

| Página | Título | Reposo | CPU del worker | Confirmación máx. |
|---|---|---|---|---|
| www.google.com | 0,6 s | 2,9 s | 1,7 s | 236 MB |
| es.wikipedia.org (Islas Galápagos) | 0,6 s | 16,5 s | 19,8 s | 353 MB |

**Lectura:** en Wikipedia, la CPU iguala al tiempo de carga, es decir, un núcleo saturado. El límite es la CPU de Servo sin optimizar, no la red (el título llega en ≤ 0,6 s).

## 1. Auditoría `--delta`: configuración actual frente a lo que Cargo y Servo prevén

| # | Hallazgo | Evidencia | Efecto |
|---|---|---|---|
| A1 | **Servo y todas sus dependencias se compilan con `opt-level = 0`** | Perfil `dev` por omisión; el worker sale de `target/debug` | El cuello de botella de §0. Además, los marcos de pila sin optimizar desbordaban los 512 KiB de `StyleThread#N`, de ahí `LAYOUT_THREADS = 1` |
| A2 | **Los perfiles de Servo no se aplican** | `vendor/servo/Cargo.toml` define `[profile.dev.package.servo-script] codegen-units = 16`, `crypto-bigint opt-level = 3` y un perfil `medium` (`opt-level = 2`, `debug = "line-tables-only"`). Cargo solo lee los perfiles del paquete raíz (la referencia de perfiles lo indica), y aquí la raíz es `galapagos-content` | Lo que Servo ya resolvió para Windows se pierde |
| A3 | **`-C codegen-units=1` global** en `crates/galapagos-content/.cargo/config.toml` (`[build] rustflags`) | Afecta a todos los crates y perfiles. Según la guía de rendimiento de Cargo, elimina el paralelismo de rustc | Compilaciones muy largas (P-09). **Motivo probable** (no documentado en el proyecto): el error de rlib de más de 4 GB de `servo-script` en Windows (rust-lang/rust#151184), que Servo resuelve solo para ese crate con `codegen-units = 16` |
| A4 | `[profile.dev] incremental = false` en el worker | `crates/galapagos-content/Cargo.toml` | Ninguna recompilación del código propio aprovecha resultados previos. Con optimización en las dependencias, el incremental del crate propio vuelve a ser útil |
| A5 | `LAYOUT_THREADS = 1` y su test `worker_preferences_disable_style_thread_pool_option_b`, que exige `layout_threads == 1` | `crates/galapagos-content/src/main.rs` | Estilos en un solo hilo; Servo usa 3 por omisión (`prefs.rs:564`) |
| A6 | `target/` del worker: 25,5 GB en debug y 3,5 GB de un intento de release que no llegó a producir el `.exe` | Medido el 2026-10-26; 737 GB libres en D: | No bloquea, pero la primera recompilación es completa |

## 2. CONTRATO DE ENTRADA Y SALIDA (Cubo A)

No cambia ningún tipo, mensaje IPC ni API. El «contrato» de este delta es la configuración de compilación, que queda así:

### 2.1 `crates/galapagos-content/Cargo.toml` (D36, D37, D38)

```toml
[profile.dev]
# D38: el código propio del worker recompila de forma incremental
incremental = true

# D36: dependencias (Servo, Stylo, SpiderMonkey, WebRender) optimizadas, con solo tablas de
# líneas para los backtraces; el crate propio queda en debug completo. Mismo criterio que el
# perfil `medium` de Servo.
[profile.dev.package."*"]
opt-level = 2
debug = "line-tables-only"

# D37: replicados del `Cargo.toml` de Servo (no se heredan porque el worker es la raíz)
[profile.dev.package.servo-script]
codegen-units = 16          # rust-lang/rust#151184: rlib de más de 4 GB en Windows
[profile.dev.package.crypto-bigint]
opt-level = 3
```

- El binario sigue saliendo en `target/debug/galapagos-content.exe`. El Coordinador lo encuentra sin cambios (`worker_candidates`, T-05).
- **Descartado: un perfil propio (`--profile medium`).** Saldría en `target/medium/`, y habría que tocar la búsqueda del worker en `src/engine/mod.rs`, que está bajo T-05. `package."*"` consigue lo mismo sin cambiar ninguna ruta.

### 2.2 `crates/galapagos-content/.cargo/config.toml` (D39)

```toml
[target.x86_64-pc-windows-msvc]
linker = "lld-link.exe"
```

- Se retira `[build] rustflags = ["-C", "codegen-units=1"]`. Su único motivo plausible (A3) queda cubierto por D37 solo para `servo-script`.
- **Si la compilación vuelve a fallar por el tamaño del rlib** (`LNK1248` o un error de archivo de más de 4 GB), el ejecutor no improvisa. Baja `codegen-units` **solo** en el `[profile.dev.package.<crate>]` afectado, informa del crate y del error real, y espera la aprobación del Director.

### 2.3 `crates/galapagos-content/src/main.rs` (D40)

- `worker_preferences()` deja de fijar `layout_threads`; se queda el valor de Servo (3). Se borra la constante `LAYOUT_THREADS` y su comentario de la «Opción B».
- **Requiere modificar un test existente (Guardrail 9), y la aprobación de este contrato lo autoriza explícitamente:** `worker_preferences_disable_style_thread_pool_option_b` pasa a llamarse `worker_preferences_keep_servo_layout_threads_delta09` y comprueba `prefs.layout_threads == servo::Preferences::default().layout_threads`. Se conserva la comprobación de que el resto de preferencias son las de Servo.
- **Condición:** D40 solo se aplica si, con D36–D39 ya compilados, no hay desbordamiento de pila en las pasadas de §6, paso 6. Si lo hay, D40 se revierte y se documenta (puerta de §6).

## 3. CONTRATO DE PERSISTENCIA

Sin cambios.

## 4. CONTRATO DE ERRORES

Sin variantes nuevas. Los fallos posibles se detectan con los mecanismos que ya existen:

| Modo de fallo | Detección existente | Respuesta |
|---|---|---|
| Desbordamiento de pila en `StyleThread#N` con 3 hilos | `stack_overflow_handler` → `WORKER_FAULT` («desbordamiento de pila en hilo …») en `app_trace.log`; `worker_stderr.log` | Puerta de D40: se revierte a 1 y se informa |
| rlib de más de 4 GB al enlazar | Error de `cargo build` | Ver D39: ajuste por crate, con aprobación |
| Worker obsoleto (otra carpeta de `target`) | L-I7: se lanza el más reciente; `describe_worker_binary` en los logs | Comprobar la fecha del `.exe` antes de medir |

## 5. ANTI-BLOAT FENCE

- **Sin crates nuevos ni features nuevas.** Solo perfiles de Cargo y la eliminación de un flag.
- **Descartado:**
  - `--release` / `lto` / `production` para el día a día: compilación mucho más larga, sin aserciones de depuración ni backtraces útiles. Queda para el empaquetado (Cubo B, `PERF-04`).
  - `opt-level = 3` en todo el árbol: el perfil `medium` de Servo usa 2 para equilibrar tiempo de compilación y de ejecución; 3 solo donde Servo ya lo usa.
  - `sccache` o cachés de terceros: dependencia externa sin necesidad demostrada.
  - Subir `SERVO_STYLE_THREAD_STACK_SIZE_KB` en vez de optimizar: tapa el síntoma (pilas enormes en debug) y deja la lentitud.
  - Unificar el worker en el workspace raíz (P-09, dos `target/` y dos `Cargo.lock`): es un cambio de estructura y merece su propio delta (Cubo B, `BUILD-01`).

## 6. Plan de ejecución (un `TARGET_FILE` por paso)

| Paso | TARGET_FILE | Resultado | Verificación |
|---|---|---|---|
| 1 | — (solo medir) | Línea base de **compilación**: recompilar el crate del worker tras tocar `main.rs`, con `cargo build -j 2 --timings` | Informe `cargo-timing.html` y minutos totales, en el documento de la línea base |
| 2 | `crates/galapagos-content/Cargo.toml` | D36, D37, D38 | — (se compila junto con el paso 3) |
| 3 | `crates/galapagos-content/.cargo/config.toml` | D39 | `cargo build -j 2 --timings` completo en segundo plano: tiempo total y crates más lentos. Después `cargo test -q` del worker (19/19) y la suite del Coordinador |
| 4 | — (solo medir) | `packaging/measure_load.ps1`: Google y Wikipedia, 3 pasadas, **todavía con `LAYOUT_THREADS = 1`** | Aísla el efecto de los perfiles. Tamaño del `.exe` |
| 5 | `crates/galapagos-content/src/main.rs` | D40 y el test ajustado | `cargo test -q` del worker |
| 6 | — (solo medir) | Misma medición con 3 hilos, más example.com y servo.org | **Puerta de D40:** cero `WORKER_FAULT` de pila en `app_trace.log` y nada nuevo en `worker_stderr.log` en las 12 pasadas |
| 7 | `docs/performance/BASELINE_PERF03_DEBUG.md` → nuevo `PERF03_RESULTS.md` | Tabla antes/después (carga, CPU, memoria, tamaño del binario, compilación limpia e incremental) | El Director prueba con ventana |

**Criterio de cierre:**
- (a) Suites en verde: Coordinador y worker.
- (b) Mediana de «Reposo» de Wikipedia menor que 16,5 s. La mejora concreta se informa con su valor medido, sin objetivo inventado de antemano.
- (c) Puerta de D40 cumplida, o D40 revertida y documentada.
- (d) Visto bueno del Director con ventana.

## 7. Decisiones para aprobar

| Id | Decisión | ¿Requiere aprobación? |
|---|---|---|
| D36 | Dependencias con `opt-level = 2` y `debug = "line-tables-only"` en el perfil `dev` | Sí |
| D37 | Replicar los ajustes por paquete de Servo (`servo-script codegen-units = 16`, `crypto-bigint opt-level = 3`) | Sí |
| D38 | `incremental = true` para el crate propio del worker | Sí |
| D39 | Retirar `-C codegen-units=1` global, con el ajuste por crate como plan B | Sí |
| D40 | `layout_threads` al valor de Servo (3), **modificando el test `worker_preferences_disable_style_thread_pool_option_b`**, con la puerta de §6 | Sí (Guardrail 9) |

## 8. Cubo B (al backlog)

| Id | Pendiente |
|---|---|
| `PERF-04` | Perfil de distribución (`release` o `production` de Servo con LTO) para el MSIX, con su propia medición |
| `BUILD-01` | Meter `galapagos-content` en el workspace raíz (un `target/` y un `Cargo.lock`), siguiendo P-09 |
| `BUILD-02` | Limpiar `target/release` del worker (3,5 GB de un intento fallido) cuando el Director lo autorice |
| `RENDER-01` | Iconos de Wikipedia que salen como cuadrados negros (máscaras o SVG en Servo), visto en INC-20261026-WORKER-DPI |
| `CLI-01` | `galapagos-browser.exe <url>` no carga la URL (comprobado tras INC-20261026-WORKER-CONSOLE) |
