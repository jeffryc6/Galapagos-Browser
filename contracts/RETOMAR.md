# RETOMAR — punto de reanudación (2026-10-26)

Léelo primero al abrir una sesión nueva. Resume el estado sin tener que releer el chat anterior. Los detalles están en los archivos enlazados: léelos por rango, solo lo que haga falta.

## 1. Dónde quedamos

**En curso: DELTA_08 (interfaz nativa Galapagos)**, congelado. Ver [DELTA_08_native_ui_galapagos.md](DELTA_08_native_ui_galapagos.md). Pasos 1–5 hechos (el 4, probado con ventana por el Director el 2026-10-26: «la UI se ve superbien»; el 5, pendiente de probar con ventana junto con el DELTA_10).

| Paso | Estado | Qué |
|---|---|---|
| 1 | HECHO | `build.rs` + `assets/galapagos.manifest` (controles comunes v6, PerMonitorV2) → test `tests/app_manifest.rs` |
| 2 | HECHO | `packaging/generate_ui_assets.ps1` → `assets/galapagos-logo.png` (128 px) y `assets/galapagos.ico` (DIB de 32 bits, 16–128 px) → `tests/ui_assets.rs` |
| 3 | HECHO | `src/window/theme.rs`: `THEME` (paleta), `contrast_ratio`, `scale(px, dpi)`, `UiFonts::for_dpi`, `glyph::`, `GdiPlus`, `Canvas` (`fill_round_rect`, `stroke_round_rect`, `draw_image`), `Image::load` |
| 4 | HECHO (probado con ventana) | `src/window/mod.rs`: DWM oscuro, icono, DPI (`ToolbarLayout`, `WM_DPICHANGED`), barra dibujada, D32, D33 (`SecurityBadge`, `BlockReason`, `EngineFault`). También `engine/mod.rs` (`set_toolbar_height`) y `main.rs` (aprobado). Atrás, adelante y recargar se dibujan deshabilitados; Inicio navega a `galapagos://dashboard` |
| 5 | HECHO (sin probar con ventana) | Mensajes IPC 9 (`NAV_STATE`) y 14–17 (`GO_BACK`, `GO_FORWARD`, `RELOAD`, `REPAINT`) en `crates/galapagos-ipc` + `src/engine/mod.rs` + `crates/galapagos-content/src/main.rs`. El 17 resuelve R-3 (repintar cuando el sistema lo pide) |
| 6 | PENDIENTE | `src/window/dashboard.rs`: barra lateral (Seguridad, Privacidad, Rendimiento), tarjetas con estado real y gráfico CPU/RAM del worker a 1 Hz (`GetProcessMemoryInfo` + `GetProcessTimes` sobre el handle del worker; búfer circular de 60 muestras) |
| 7 | PENDIENTE | Retirar del contrato `GalapagosTheme`, `GALAPAGOS_DARK`, `DASHBOARD_HISTORY_LEN` y `LiveDashboardState` (`contracts/galapagos_core_v1.rs:512-584`, R-18) |

### Notas del paso 4 para el paso 5
- Habilitar Atrás, Adelante y Recargar con `MSG_NAV_STATE` (hoy `ToolbarButton::enabled` solo devuelve `true` para Inicio). El clic ya llega al bucle como `LoopAction::Toolbar(_)` y hoy se ignora.
- **Pendiente detectado (T-04b):** cuando la página navega por su cuenta, la insignia sigue diciendo «Ocean Defense: permitido» para una URL que no se evaluó.
- **Empaquetado:** `build_msix.ps1` no copia aún `assets/galapagos.ico` ni `galapagos-logo.png` junto al exe. La UI los busca en `<exe>\`, en `<exe>\assets\` y, en debug, en `CARGO_MANIFEST_DIR\assets`.

### Paso 4 en detalle (`src/window/mod.rs`), ya hecho
- **Ventana:**
  - barra de título oscura con `DwmSetWindowAttribute(DWMWA_USE_IMMERSIVE_DARK_MODE=20)`; añadir la feature `Win32_Graphics_Dwm`; si falla en Windows 10 antiguos, no es error;
  - icono con `LoadImageW(assets\galapagos.ico, LR_LOADFROMFILE)` + `WM_SETICON`; resolver la ruta junto al exe y, en debug, en `CARGO_MANIFEST_DIR/assets`;
  - fondo Volcanic Black.
- **Escala por DPI:** con `GetDpiForWindow` + `WM_DPICHANGED` (feature `Win32_UI_HiDpi`). Hoy la barra mide **46 px fijos** en tres sitios (`window/mod.rs`, `engine/mod.rs: resize_viewport` y la creación de la superficie); hay que centralizar la altura escalada y pasarla al motor.
- **Barra dibujada:**
  - botones ← → ⟳ ⌂ con glifos (`theme::glyph`) y estados normal, hover y deshabilitado; atrás y adelante quedan deshabilitados hasta el paso 5;
  - campo de dirección redondeado (`Canvas::fill_round_rect` en Lava Stone) con el `EDIT` dentro, sin borde, con `WM_SETFONT` (UiFonts.text) y `WM_CTLCOLOREDIT` (texto `#E6EDF3` sobre `#222B34`);
  - **quitar el botón «Navegar»** (D32; Enter ya navega).
- **Insignia (D33, UI-I0):** solo estados medidos: `Allowed`, `Blocked` con su motivo e `Internal`. Nada de «SEGURO».
- **Logo pequeño (Image) a la izquierda de la barra:** opcional, si cabe.
- **Tests:**
  - el estilo y los atributos DWM de la ventana real;
  - el layout escalado a 96, 144 y 192 DPI (lógica pura);
  - que no exista el control «Navegar»;
  - que los textos de la insignia salgan de estados reales.
- **Ejecución:** compilar, ejecutar la suite y que el Director pruebe con ventana.

## 2. Reglas del Director (obligatorias)
- **ZNVE:** contrato antes que código (`/znve-contract --delta` para lo nuevo); un `TARGET_FILE` por paso; no tocar tests sin aprobación (Guardrail 9).
- **UI-I0 / sin simulaciones:** la app muestra solo datos de producción medidos. Prohibidos los valores de ejemplo, gráficos de relleno, estados fijos en el código y modos de demostración. Los datos de test solo viven en `#[cfg(test)]` y en `tests/`.
- **Simplicidad:** sin dependencias nuevas salvo que un delta las justifique.

## 2b. Incidente resuelto el 2026-10-26: INC-20261026-WORKER-CONSOLE
- **Síntoma:** la UI carga pero ninguna página lo hace. Al arrancar aparece `[error 2147942632 (0x800700e8) when launching "...galapagos-content.exe" ...]`.
- **Causa:**
  - el worker es una app de consola y el Coordinador no tiene consola;
  - `CreateProcessW` lo lanzaba solo con `CREATE_SUSPENDED`, así que Windows le creaba una consola visible;
  - el traspaso de esa consola a Windows Terminal fallaba (Win32 232) y el worker se bloqueaba al escribir en stdout;
  - medido: ~5 s con 1 hilo al arrancar, 0,1 s de CPU, ninguna conexión TCP y nunca un título.
- **Hotfix** (`src/engine/mod.rs`): `WORKER_CREATION_FLAGS = CREATE_SUSPENDED | CREATE_NO_WINDOW`. Test de regresión `worker_launches_suspended_without_console_window_inc20261026`.
- **Verificado con ventana:**
  - el worker arranca en < 1 s;
  - Google y Wikipedia cargan (título a los 0,6 s y < 1 s);
  - no se abre Windows Terminal.
- **Fallo aparte, sin resolver (comprobado tras el hotfix):** una URL pasada por línea de comandos no se carga; en 20 s el título sigue siendo el inicial. La barra sí funciona.

## 2c. Incidente resuelto el 2026-10-26: INC-20261026-WORKER-DPI
- **Síntoma:** las páginas cargan pero no se ven nítidas.
- **Causa:**
  - el worker no declaraba DPI (`GetProcessDpiAwareness` = 0) y el Coordinador sí (2);
  - Windows le daba un tamaño de superficie dividido por la escala (1514×1027 físicos a 144 DPI pasaban a ~1009×685), así que Servo dibujaba más pequeño y la imagen se estiraba ×1,5.
- **Hotfix** (`crates/galapagos-content/src/main.rs`):
  - `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` al arrancar;
  - `hidpi_scale_factor(dpi/96)` al crear el WebView y `set_hidpi_scale_factor` en cada `Resize` si cambia el DPI;
  - FFI de user32 declarada a mano;
  - en `Cargo.toml`, `euclid = "0.22"`, que ya estaba en el árbol (0.22.14), para `Scale`.
  - Test: `hidpi_scale_follows_monitor_dpi_inc20261026`.
- **Verificado con ventana:**
  - el worker declara DPI por monitor (2);
  - la captura ampliada de Wikipedia sale nítida;
  - el tiempo no empeora: Wikipedia tarda 16,5 s (antes 18,5 s) y gasta ~40 MB más.
- **Visto de paso:** algunos iconos de Wikipedia salen como cuadrados negros (máscaras o SVG que Servo no dibuja). No se ha investigado.

## 3. Pendientes fuera del DELTA_08

| Id | Pendiente | Vía |
|---|---|---|
| **Cuota del worker** | **En prueba**: 2 GiB de memoria y CPU sin tope (`src/main.rs`). Decidir los valores definitivos (Estándar y Hardened) y el modelo de CPU (tope duro, peso o ninguno). Nota en `docs/performance/RESOURCE_BUDGETS.md` | Decisión del Director tras medir el «Tamaño de confirmación» |
| **`layout_threads = 1`** | Mitigación del desbordamiento de pila de `StyleThread#N` (pila fija de 512 KiB de Stylo en debug), en `crates/galapagos-content/src/main.rs` (`LAYOUT_THREADS`) | Volver a 3 con PERF-03 |
| **PERF-03** | Optimizar Servo. **Línea base en debug medida** en [BASELINE_PERF03_DEBUG.md](../docs/performance/BASELINE_PERF03_DEBUG.md); la de referencia es la tomada tras el hotfix de DPI: Wikipedia tarda 16,5 s con un núcleo saturado; Google, 2,9 s. Propuesta: `[profile.dev.package."*"] opt-level = 2`, quitar `codegen-units=1` global y volver a `LAYOUT_THREADS = 3` | [DELTA_09](DELTA_09_perf03_worker_build_profiles.md) CONGELADO (D36–D40 aprobadas). **Pausado por el Director (2026-10-26 12:40)** tras los pasos 1–3. Hecho: compilación incremental previa del worker 31,9 s; D36–D39 aplicadas; compilación limpia correcta en 58,7 min sin error de 4 GB; binario de 129 MB (antes 210 MB). **Siguiente:** suites (worker y Coordinador) y paso 4 (medir con 1 hilo). D40 sin aplicar. Prueba con ventana del Director: «la navegación mejoró mucho» |
| **COMPAT-01** | Peticiones del motor que el worker ignora: `request_create_new` (`window.open`/`target="_blank"` no hacen nada), `show_console_message`, `notify_crashed`, `show_embedder_control` | [DELTA_10](DELTA_10_web_compat_embedder_requests.md) CONGELADO (D41–D44 aprobadas). Pasos 1–2 HECHOS el 2026-10-26 (worker 29/29, Coordinador en verde, sonda en localhost correcta). Prueba con ventana del Director hecha el 2026-10-26: «la navegación ya es fluida y estable»; los vídeos de YouTube no cargan (Servo 0.7 sin MSE y worker sin `media-gstreamer`). **Siguiente:** decisiones F-1 y F-2 de [COMPAT01_FINDINGS.md](../docs/compat/COMPAT01_FINDINGS.md) y la fase recomendada en [INFORME_AVANCES_2026-10-26.md](../docs/reports/INFORME_AVANCES_2026-10-26.md) §6 (arnés → forense → delta). **Arnés COMPAT capturado el 2026-10-27** ([tests/characterization/compat/HARNESS.md](../tests/characterization/compat/HARNESS.md)): 4 sondas, 100 % verde en 3 ejecuciones idénticas; cualquier cambio en Servo, preferencias o features se valida con `Run-CompatHarness.ps1`. **F-1 hecho el 2026-10-27** (D45: consulta y fragmento de las URL recortados en `[console:error]`; worker 30/30; arnés sin fallos; comprobado en el binario real). Prueba con ventana del Director: Google, Wikipedia, servo.org y YouTube cargan sin errores; faltan los vídeos y las miniaturas de YouTube. **F-2 hecho el 2026-10-27** (D46: diagnóstico por WebView, contador vaciado al empezar una carga; worker 32/32). `snapshots/popups.txt` recapturado con la aprobación del Director (`about` → `localhost`); arnés completo sin fallos. **Forense hecho el 2026-10-27:** [FORENSIC_SERVO_CONFIG_2026-10-27.md](FORENSIC_SERVO_CONFIG_2026-10-27.md). Zona roja Z-1: `hard_fail = true` (el pánico de cualquier página mata el worker). **[DELTA_11](DELTA_11_compat03_servo_configuration.md) CONGELADO el 2026-10-27** (D47–D51 y D53 aprobadas; D52/webgl rechazada). **PAUSA del Director (2026-10-27).**
- **Hecho:**
  - pasos 1–5 de código (IPC 18, códec y test, insignia `PageCrashed`, `[content:crash]`, `Opts`/`setup_logging`/gancho/preferencias/D53 en el worker, `webcrypto` + `log` en `Cargo.toml`);
  - Coordinador con 122 tests en verde y recompilado;
  - sondas `layout` (`columns.w`) y `crash.html`, y el ejecutor con D53.
- **Incidencia:** `pkcs1` fijado a `0.8.0-rc.4` en el `Cargo.lock` del worker.
- **En curso al pausar:** la recompilación completa del worker en segundo plano.
- **Al retomar:**
  1. comprobar que la compilación terminó (fecha de `crates/galapagos-content/target/debug/galapagos-content.exe` posterior al 2026-10-27 22:14; si no, `cargo build -j 2` en el worker);
  2. `cargo test -q` del worker (esperados 37 = 32 + 5 nuevos);
  3. `Run-CompatHarness.ps1`: solo las diferencias de §6 del DELTA_11 (y `crash` sin snapshot);
  4. recapturar con la aprobación del Director;
  5. `-Mode Sites` y prueba con ventana (miniaturas de YouTube) |
| **URL por línea de comandos** | `galapagos-browser.exe <url>` no carga la URL | Triage aparte |
| **T-02** | Sandbox real: el worker se **autodeclara** `AppContainerStrict` y se lanza con un `CreateProcessW` normal. Solo el Job Object es real | `--delta` AppContainer (ENG-04) |
| **T-04b** | Las navegaciones que inicia la página (enlaces, redirecciones) no pasan por Ocean Defense | `--delta` con mensaje IPC `NavigationRequest` |
| **`request_create_new`** | Los enlaces `target=_blank` y `window.open` se descartan (YouTube, algunos resultados de Google) | Cambio pequeño en `delegates.rs`: abrir en la misma pestaña |
| **Vídeo** | Servo usa `servo-media-dummy`: no se reproduce vídeo | `--delta`: feature `media-gstreamer` + GStreamer instalado |
| **`--smoke-test`** | Modo de prueba dentro del binario (bloquea `phishing.evil.com`, `ProcessId(1)` fijo, base en memoria). Contradice UI-I0 | `--delta` para retirarlo (contrato DELTA_04 L-I6) |
| **Clave del vector de prueba RFC 8032** | `GALAPAGOS_MASTER_PUBKEY` en `src/defense/threat_intel.rs:21`. Sin efecto (fail-closed), pero es un dato de prueba | Con SEC-05 (verificador real) |
| **`SecurityIsland` / `PayloadGuard`** | Solo los usa `tests/production_certification.rs`; en runtime no se usan | Decidir si se retiran (ajustar el test, con aprobación) |
| **Doble evaluación de navegación** | La UI evalúa y el Coordinador reevalúa al confirmar (decisión de diseño) | Opcional |
| **Prueba con ventana del DELTA_07** | Hecha: sin errores en el segundo arranque | — |
| Backlog | PRIV-02, PERF-02, DEV-01/02, NET-01/02, EXT-01/02, PLAT-01, ENG-02..11, UX-01..07, SEC-04/05, PERS-01..04, CI-01 | [CONTRACT_BACKLOG.md](CONTRACT_BACKLOG.md) |

## 4. Estado verificado al pausar
- **Suite del Coordinador** (`cargo test -j 2 -q --no-fail-fast` en la raíz):
  - lib 117/117 (paso 4: +5 tests nuevos y 3 ajustados con aprobación; hotfix INC-20261026: +1), bin 7/7;
  - integración: `app_manifest` 1, `engine_stub_fence` 1, `persistence_wal` 6, `production_certification` 6, `ui_assets` 3, `worker_isolation` 3;
  - 0 fallos.
- **Worker** (`cargo test -q` en `crates/galapagos-content`): 19/19. Binario `crates/galapagos-content/target/debug/galapagos-content.exe` recompilado el 2026-10-26 11:06 (hotfix de DPI).
- **Crate compartido** `crates/galapagos-ipc`: 3/3.
- **Navegación con ventana (Director):** las páginas ocupan toda la ventana; Google carga. YouTube no reproduce vídeo.

## 5. Mapa rápido
- **Incidente de limpieza:** [TRIAGE_INC-20261025-CORE-DEBLOAT.md](TRIAGE_INC-20261025-CORE-DEBLOAT.md), con la tabla «Estado de remediación».
- **Persistencia:** [DELTA_07_persistence_wal_by_compartment.md](DELTA_07_persistence_wal_by_compartment.md). Base en `%LOCALAPPDATA%\GalapagosBrowser\profiles\personal.db`.
- **Logs:** `%LOCALAPPDATA%\GalapagosBrowser\logs\` (`app_trace.log`, `worker_stderr.log`, `coordinator_error.log`).
- **Formato IPC único:** `crates/galapagos-ipc/src/lib.rs`. El Coordinador comprueba en compilación que sus topes coinciden con el contrato.
- **Lecciones técnicas:**
  - Servo 0.7: `WebView::resize` ya redimensiona la superficie; **no** llamar antes a `rendering_context.resize`.
  - Stylo: pila de 512 KiB fijada al compilar (`SERVO_STYLE_THREAD_STACK_SIZE_KB`).
  - Windows Defender bloquea `target/` (`os error 32`): usar `cargo -j 2` y reintentar.
  - PowerShell 5.1 lee los `.ps1` sin BOM como ANSI: no escribir literales con tildes en código de script.

## 6. Cómo reanudar
1. Abrir una sesión nueva y pedir: «retomemos: lee `contracts/RETOMAR.md`».
2. Continuar con `/znve-execute` paso 5 del DELTA_08 (sección 1), una vez el Director haya probado el paso 4 con ventana.
