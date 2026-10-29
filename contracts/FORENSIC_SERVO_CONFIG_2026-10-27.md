# Forense — Superficie de configuración de Servo 0.7 en el worker

**Tipo:** `/znve-forensic`, solo lectura. No se ha modificado código ni configuración. **Fecha:** 2026-10-27. **Objetivo:** cómo configura el worker (`crates/galapagos-content`) a Servo frente a lo que Servo ofrece, para explicar con evidencia los huecos que mide el arnés COMPAT ([HARNESS.md](../tests/characterization/compat/HARNESS.md)).

**Origen de cada afirmación:**
- **[O] observado** en el código o medido por el arnés.
- **[I] inferido**, todavía sin verificar.

## 1. RESUMEN DE DOMINIO

El worker construye Servo con `ServoBuilder::default().preferences(worker_preferences())` (`crates/galapagos-content/src/main.rs`). [O]
- **Preferencias:** cambia una sola, `layout_threads = 1`; el resto son las de `servo::Preferences::default()`.
- **Opciones de proceso (`Opts`):** no se pasan, así que rige `Opts::default()`.
- **Features de Cargo:** `default-features = false` más `bundled`, `multiprocess`, `js_jit` y `no-wgl`.

Frente a servoshell, el navegador de referencia de Servo, faltan tres cosas: [O]
- las `default_web_features` (`webcrypto`, `webgl`, `webgpu`, `webxr`, `clipboard`, `brotli-compression-stream`) y `gamepad`;
- el modo `--enable-experimental-web-platform-features`, que activa 18 preferencias;
- `hard_fail = false`, que servoshell usa por defecto.

Casi todos los huecos del arnés salen de estas tres diferencias, no de carencias del motor. Las excepciones son MSE y EME, que no existen en Servo 0.7.

## 2. MATRIZ DE ENTRADAS, SALIDAS Y ESTADO

### 2.1 Features de Cargo (en tiempo de compilación)

| Feature | Worker | servoshell | Efecto medido o verificado |
|---|---|---|---|
| `webcrypto` | No | Sí | **`window.crypto` no existe**: el campo y el getter van tras `#[cfg(feature = "webcrypto")]` (`script/dom/window/window.rs:301,1602`). Arnés: `crypto.getRandomValues=E:ReferenceError`. Explica `crypto.getRandomValues() not supported` en nacion.com [O] |
| `webgl` | No | Sí | `webgl=0`, `webgl2=0` [O] |
| `webgpu` | No | Sí | `webgpu=0` [O] |
| `clipboard` | No | Sí | `clipboard=0` [O] (además depende de `dom_async_clipboard_enabled`) |
| `brotli-compression-stream` | No | Sí | `CompressionStream` sin brotli [I] |
| `webxr`, `gamepad` | No | Sí | Sin impacto en navegación general [I] |
| `media-gstreamer` | No | No (opcional en ambos) | Sin backend multimedia: `canPlayType` vacío para MP4, WebM y MP3 [O]. `servo-media-auto` incluye GStreamer en x86_64 (`components/media/backends/auto/Cargo.toml:16`), pero el reproductor necesita la feature y las DLL de GStreamer en el sistema [I] |
| (no existe) | — | — | **MSE / EME / WebCodecs**: Servo 0.7 no tiene `MediaSource.webidl` [O]. No hay palanca de configuración |

### 2.2 Preferencias (en tiempo de ejecución, `servo::Preferences`)

Desactivadas por defecto y activadas por el modo experimental de servoshell (`ports/servoshell/prefs.rs:34–53`) [O]:

| Preferencia | Hueco relacionado | Evidencia |
|---|---|---|
| `dom_intersection_observer_enabled` | Carga diferida de imágenes y bloques | Arnés `IntersectionObserver=0`; 6 errores en nacion.com; hipótesis para las **miniaturas de YouTube** [I] |
| `layout_container_queries_enabled` | Container queries | Arnés `container.w=10` (esperado 77) [O] |
| `layout_columns_enabled` | CSS multicolumna (p. ej., referencias de Wikipedia) | [I] |
| `dom_indexeddb_enabled` | IndexedDB | Arnés `indexedDB=0` [O] |
| `dom_webgl2_enabled`, `dom_webgpu_enabled`, `dom_offscreen_canvas_enabled` | Gráficos | Requieren además las features de §2.1 [O] |
| `dom_fontface_enabled` | API `FontFace` (carga de fuentes desde JS) | [I] |
| `dom_notification_enabled`, `dom_permissions_enabled`, `dom_async_clipboard_enabled` | APIs con permiso | Hoy `request_permission` lo deniega todo (D-delegates) [O] |
| `layout_variable_fonts_enabled`, `dom_exec_command_enabled`, `dom_sanitizer_enabled`, `dom_storage_manager_api_enabled`, `dom_navigator_protocol_handlers_enabled`, `layout_css_ellipse_corners_enabled`, `layout_css_progress_function_enabled` | Varios | [I] |

Otras preferencias relevantes que el modo experimental no toca [O]:

| Preferencia | Valor | Observación |
|---|---|---|
| `dom_serviceworker_enabled` | `false` | Fuera incluso del modo experimental: madurez baja |
| `dom_web_animations_enabled`, `dom_visual_viewport_enabled` | `false` | APIs que usan sitios modernos [I] |
| `viewport_meta_enabled` | `false` | Solo afecta a móvil (`webview.rs:675`) |
| `network_http_disk_cache` | `""` | Caché HTTP solo en memoria (PERS-05) |
| `session_history_max_length` | `20` | Tope de Atrás/Adelante |
| `js_timers_minimum_duration` | `1000` | Lo aplica `Timers::slow_down` (`script/event_loop/timers.rs:448`) en ventanas estranguladas [O]. Probable causa de que la sonda de popups con temporizadores de 300 ms no terminara con el opener oculto [I] |
| `js_mem_gc_incremental_enabled`, `js_mem_gc_per_zone_enabled` | `false` | GC no incremental: pausas en páginas pesadas [I] |
| `layout_threads` | `1` (nuestro) / `3` (Servo) | Pendiente D40 (DELTA_09) |

**Preferencias de Stylo que `servo::Preferences` no expone** [O]: `prefs::set()` solo traslada 9 preferencias a Stylo (`components/config/prefs.rs:52–76`).
- `layout.css.has-selector.enabled = false` (`stylo_static_prefs/preferences.toml:24`) controla `:has()`: `style/servo/selector_parser.rs:598` lo consulta en `parse_has()`. **No hay campo en `Preferences`**. Arnés: `has.w=10`. Explica los `SyntaxError` de selectores con `:has()` en nacion.com.
- `layout.css.grid-template-subgrid-value.enabled = false`: arnés `css.subgrid=0`.
- Cambiarlas exige llamar a `stylo_static_prefs::set_pref!`, lo que supone una **dependencia directa nueva** (ya está en el árbol vía Servo) o un parche en `components/config/prefs.rs`.

### 2.3 Opciones de proceso (`Opts`, no las pasamos)

| Opción | Valor efectivo | Observación |
|---|---|---|
| `hard_fail` | **`true`** (`config/opts.rs:248`) | Ver Zona Roja Z-1 |
| `config_dir` / `temporary_storage` | `None` / `false` | Sin directorio de datos: cookies y almacenamiento solo en memoria (PERS-05) |
| `multiprocess` | `false` | La feature está compilada pero no se usa; Servo corre en un solo proceso dentro del worker, que ya es el proceso aislado |
| `ignore_certificate_errors`, `certificate_path`, `host_file` | Por defecto | Correcto para seguridad |

### 2.4 API del embebedor sin usar

| Punto | Estado | Efecto |
|---|---|---|
| `ServoDelegate` (no se registra) | `notify_error` (`LostConnectionWithBackend`, `ResponseFailedToSend`), `show_console_message` y `load_web_resource` de los **workers** se pierden | Los errores de Web Workers y Service Workers no llegan a D42; los fallos internos de Servo no se notifican [O] |
| `WebViewDelegate::notify_cursor_changed` | Sin implementar | UX-04 |
| `notify_favicon_changed`, `notify_status_text_changed` | Sin implementar | Sin favicon ni URL de enlace al pasar el ratón |
| `request_authentication` | Sin implementar | Autenticación HTTP básica imposible [I] |
| `load_web_resource` | Sin implementar | Única vía para descargas (NET-03) y para T-04b |

## 3. EFECTOS SECUNDARIOS

- **Red:** Servo hace las peticiones HTTP directamente desde el worker (con `rustls`/`aws_lc_rs`). El Coordinador no las ve (T-04b, NET-02). Lee `http_proxy`/`https_proxy`/`no_proxy` del entorno (`Preferences::default`, `prefs.rs:619–629`) [O]. **Una variable de entorno del sistema cambia la ruta de red del worker sin pasar por NetworkRoutePolicy.**
- **Disco:** ninguno de Servo (sin `config_dir` ni caché en disco). Solo nuestro `worker_stderr.log` [O].
- **Proceso:** con `hard_fail`, un pánico de pipeline llama a `process::exit(1)` dentro de Servo (`constellation.rs:3020`) [O].

## 4. EQUILIBRIOS ACCIDENTALES

- **E-1 · Gancho de pánico + `hard_fail`.**
  - Hoy cualquier pánico de Servo dispara nuestro gancho (`WORKER_FAULT`) y después Servo termina el proceso. Las dos cosas son coherentes: «Motor falló» y el worker muere.
  - Con `hard_fail = false`, Servo recuperaría el pánico de un pipeline (página `about:failure` y `notify_crashed`, D43), pero **nuestro gancho seguiría enviando `WORKER_FAULT`**. El Coordinador marcaría «Motor falló» con el worker vivo, y el primer fallo bloquea los siguientes (`WORKER_FAULT_REPORTED`).
  - **No conviene cambiar uno sin el otro.**
- **E-2 · `layout_threads = 1` y los perfiles de DELTA_09.** La mitigación de pila sigue activa aunque las dependencias ya están optimizadas. Retirarla es D40, con su puerta.
- **E-3 · `CSS.supports` frente al render.** `aspect-ratio` dice estar soportado, pero la caja de flujo de bloque mide 0 de alto (arnés `aspect-ratio.h=0`). Servo usa la proporción en la ruta de `preferred_aspect_ratio` (`layout/flow/mod.rs:1211`); falta ver por qué el bloque sin contenido no la aplica [I]. Es un caso de motor, no de configuración.

## 5. ZONAS ROJAS

| Id | Zona | Evidencia | Riesgo |
|---|---|---|---|
| **Z-1** | **`hard_fail = true` en producción** | `Opts::default()` (`opts.rs:248`) + `constellation.rs:3016–3020`. servoshell lo deja en `false` salvo con `-f` | **Alto:** un pánico de script en **cualquier** página (o en un iframe de anuncios) mata el worker entero. El usuario pierde todas las ventanas de la pila, y D43 (`notify_crashed`) nunca se ejecuta |
| Z-2 | Proxy desde variables de entorno | `prefs.rs:619–629` | Medio: ruta de red no controlada por el Coordinador (privacidad, NET-01) |
| Z-3 | `ServoDelegate` sin registrar | §2.4 | Medio: errores internos de Servo y consola de los workers, invisibles |
| Z-4 | Preferencias experimentales | Las 18 de §2.2 son «experimentales» para Servo | Medio: activarlas puede introducir pánicos (y con Z-1, caídas del worker). **Activarlas solo después de resolver Z-1 y E-1**, una a una y con el arnés |
| Z-5 | `:has()` y subgrid fuera de `Preferences` | §2.2 | Bajo: requiere una dependencia directa o un parche, con su delta |

## 6. Palancas ordenadas (insumo para `/znve-contract --delta`; no es una orden de ejecución)

| Orden | Palanca | Coste | Qué valida el arnés |
|---|---|---|---|
| 1 | **Z-1 + E-1:** `Opts { hard_fail: false, .. }` y gancho de pánico que distinga el pánico de un pipeline (recuperable, D43) del pánico del propio worker | Bajo; 2 archivos del worker | Nueva sonda que provoque un pánico de script (si existe una forma determinista) o, como mínimo, que el arnés siga sin fallos |
| 2 | Feature `webcrypto` | Recompilación completa del worker (≈1 h) | `caps`: `crypto.*=1` |
| 3 | `dom_intersection_observer_enabled` | Preferencia, recompilación incremental | `caps`: `IntersectionObserver=1`; miniaturas de YouTube con ventana |
| 4 | `layout_container_queries_enabled`, `layout_columns_enabled` | Preferencia | `layout`: `container.w=77` |
| 5 | Features `webgl` (+ `dom_webgl2_enabled`) | Recompilación completa; superficie ANGLE ya presente | `caps`: `webgl=1` |
| 6 | `dom_indexeddb_enabled` | Preferencia; con PERS-05 para persistir | `caps`: `indexedDB=1` |
| 7 | `:has()` / subgrid (Z-5) | Dependencia directa `stylo_static_prefs` o parche | `layout`: `has.w=50` |
| 8 | Registrar `ServoDelegate` (Z-3) | Bajo | Errores de workers en D42 |
| 9 | Proxy de entorno (Z-2) | Bajo: fijar `network_http(s)_proxy_uri` desde la política del Coordinador | — |
| — | `media-gstreamer` | Dependencia externa (GStreamer en Windows): delta propio | `caps`: `canPlayType` |
| — | MSE / EME | Sin palanca: upstream o upgrade de Servo | — |

**Recomendación:** un DELTA_11 «COMPAT-03: configuración de Servo» con las palancas 1–4 en ese orden. La 1 va primero porque las demás aumentan la probabilidad de pánicos, y hoy cada pánico mata el worker. Las palancas 2 y 5 comparten una única recompilación completa del worker.
