# DELTA 10 — COMPAT-01: peticiones del motor que el worker ignora

**Estado:** CONGELADO (2026-10-26) · D41–D44 aprobadas por el Director · **Origen:** prueba con ventana tras DELTA_09 («la navegación mejoró mucho») y el análisis de compatibilidad aportado por el Director · **Extiende:** `src/delegates.rs` y el bucle del worker (DELTA_01/02/05), sin tocar el contrato IPC · **Depende de:** DELTA_08 paso 5 (MSG_GO_BACK y MSG_NAV_STATE), que se ejecuta antes

## 0. Auditoría `--delta`: el análisis frente a lo que ya existe

Revisado contra `contracts/`, `crates/galapagos-content/src/` y `vendor/servo/components/servo/webview_delegate.rs`. Cada punto del análisis queda en una de tres columnas: **ya hecho o ya contratado** (no se repite), **Cubo A** o **Cubo B**.

| Punto del análisis | Estado real en el código | Destino |
|---|---|---|
| DPI correcto | Resuelto: INC-20261026-WORKER-DPI (PerMonitorV2 + `hidpi_scale_factor`) | Hecho |
| Clics y hit testing | Reenvío de ratón y teclado en DELTA_02 (D8). Pendiente solo la entrada en formularios | Ya contratado: `ENG-11` |
| `request_navigation` atendido | Sí: `delegates.rs:91`, permite http/https/`about:blank` | Hecho |
| Política de `data:` (SEC-04) | Ya aplicada en `is_navigation_allowed` (`delegates.rs:34`) con su test | Hecho; la fila del backlog estaba obsoleta |
| Atrás / Adelante / Recargar | Contratado en DELTA_08 §2.2 (IPC 9 y 14–16), paso 5. **Sin pila propia `Vec<Url>`:** Servo ya emite `notify_history_changed(entries, current)`; una pila paralela se desincroniza con `pushState` de las SPA | Ya contratado: DELTA_08 paso 5 |
| **`window.open` y `target="_blank"`** | **No implementado.** `request_create_new` usa el método por defecto, y Servo documenta: «si se ignora, no se abre ningún WebView». Esos enlaces no hacen nada | **Cubo A: D41** |
| **Consola JS, excepciones, fallos de red** | **No implementado.** `show_console_message` se descarta; no hay forma de ver por qué falla YouTube | **Cubo A: D42** |
| **Caída del contenido** | **No implementado.** `notify_crashed` se descarta; la página se queda en blanco sin aviso | **Cubo A: D43** |
| `<select>`, selector de archivos, `alert`/`confirm`/`prompt`, menú contextual | **No implementado.** `show_embedder_control` se descarta: Servo responde solo al soltar la petición (Aceptar o Cancelar sin mostrar nada) y los desplegables no se abren | **Cubo A: D44** (dejar constancia). Presentarlos va al Cubo B: `UX-08` |
| Cookies, `localStorage` e IndexedDB persistentes | El worker no fija un directorio de datos: todo vive en memoria y se pierde al cerrar. Coherente con el compartimento efímero; persistir es una decisión de privacidad por perfil (DELTA_07) | Cubo B: `PERS-05` |
| Caché HTTP | Ya existe en Servo: `components/net/http_cache.rs`, en memoria, con `Cache-Control`, `ETag` y peticiones condicionales. No se reimplementa. Lo que falta es una caché en disco, ligada a la misma decisión de privacidad | Cubo B: `PERS-05` |
| User-Agent de Chrome | Hoy se usa el de Servo (estilo Firefox), y el test `worker_preferences_*` exige `prefs.user_agent == defaults.user_agent`. Fingir Chrome hace que los sitios sirvan código solo para Chrome, que Servo no ejecuta | Cubo B: `COMPAT-02`, experimento medido con aprobación (Guardrail 9) |
| Service Workers, IndexedDB | Desactivados por omisión en Servo (`dom_serviceworker_enabled: false` y `dom_indexeddb_enabled: false`, `prefs.rs:448` y `:461`). Activarlos es experimental | Cubo B: `COMPAT-03`, medido con la sonda |
| Sonda de capacidades (`navigator.*`) | No existe. Daría datos reales para comparar | Cubo B: `DEV-03` (dentro de `DEV-01`) |
| Descargas | `WebViewDelegate` de Servo 0.7 no tiene un método de descarga (verificado en la lista de métodos); habría que interceptar con `load_web_resource` | Cubo B: `NET-03` |
| Bloqueo de anuncios | Ya en el backlog | Ya contratado: `NET-02` |
| Composición por GPU | La superficie es `WindowRenderingContext` (ANGLE) presentando directamente en el HWND hijo, sin copias por CPU | Hecho; `ENG-02` solo si cambia el transporte |
| DevTools | Ya en el backlog | Ya contratado: `DEV-01`, `DEV-02` |
| MSE, EME/DRM, WebCodecs | Funciones del motor, no del embebedor. EME necesita un CDM propietario (Widevine) que no se puede incluir | Fuera de perímetro (upstream Servo). EME descartado |

**Lectura:** la causa determinista de «los enlaces no funcionan» que el análisis sospechaba existe, pero no está en `request_navigation`, sino en `request_create_new`. Los otros dos huecos de Cubo A (consola y caídas) son la telemetría por excepción que hace falta para diagnosticar YouTube con datos reales en lugar de suposiciones.

## 1. CONTRATO DE ENTRADA Y SALIDA (Cubo A)

**No hay mensajes IPC nuevos.** La caída del contenido usa el canal `MSG_WORKER_FAULT` (8) que ya existe. El resto queda dentro del worker.

### 1.1 Eventos internos del delegado (`delegates.rs`, aditivo)

```rust
pub enum OutgoingWorkerEvent {
    FrameReady { frame_seq: u64 },
    TitleChanged(String),
    UrlChanged(Url),
    /// D41: la página pide un contexto auxiliar (`window.open`, `target="_blank"`)
    AuxiliaryRequested(servo::CreateNewWebViewRequest),
    /// D41: un WebView auxiliar se cerró (`window.close()`)
    WebViewClosed(servo::WebViewId),
    /// D43: el contenido falló; el motivo ya va saneado y recortado
    ContentCrashed(String),
}
```

- **Filtro del WebView activo (D41):** el delegado guarda el `WebViewId` activo. `TitleChanged` y `UrlChanged` de cualquier otro WebView se descartan, porque la barra solo muestra el activo.
- `AuxiliaryRequested` lleva la petición al bucle principal, que es el único que tiene el `RenderingContext`. Los delegados de Servo se invocan en ese mismo hilo, dentro de `spin_event_loop`.

### 1.2 Modelo de WebView auxiliar (D41, `main.rs`)

Sin pestañas todavía (`UX-02`), un contexto auxiliar **sustituye en pantalla al actual sin destruirlo**:

```rust
/// D41: el último es el activo; los anteriores siguen vivos como «opener» (OAuth y pagos
/// devuelven el resultado al opener con postMessage)
const MAX_WEBVIEW_STACK: usize = 4;
struct WebViewStack { views: Vec<servo::WebView> } // views.len() en 1..=MAX_WEBVIEW_STACK
```

| Evento | Efecto |
|---|---|
| `AuxiliaryRequested` con pila < 4 | `request.builder(rendering_context).delegate(delegado compartido).hidpi_scale_factor(escala actual).build()`; se apila, pasa a ser el activo y recibe el foco y la entrada. Se emiten su URL y título |
| `AuxiliaryRequested` con pila llena | Se suelta la petición (Servo no crea nada) y se registra una línea (§3) |
| `WebViewClosed(id)` | Se quita de la pila. Si era el activo, se reactiva el anterior y se emiten su URL y título. El WebView raíz nunca se cierra por petición de la página |
| `MSG_GO_BACK` (DELTA_08) con el activo sin historial atrás y pila > 1 | Se cierra el auxiliar activo y se vuelve al anterior |
| `MSG_NAV_STATE` (DELTA_08) | `can_back = activo.can_go_back() \|\| pila > 1` |
| `MSG_RESIZE` | Se aplica a todos los WebView de la pila (comparten superficie) |
| `request_resize_to` / `request_move_to` | Se ignoran a propósito: el contenido no mueve ni redimensiona la ventana del Coordinador |

- La política de navegación (`is_navigation_allowed`) se aplica igual a los auxiliares, porque comparten delegado.
- `MSG_NAVIGATE` desde la barra navega el WebView activo.

### 1.3 Consola por excepción (D42)

```rust
/// D42: solo `ConsoleLogLevel::Error`; Warn, Info, Log y Debug se descartan (cero ruido)
const MAX_CONSOLE_LINE_BYTES: usize = 512;
const MAX_CONSOLE_LINES_PER_DOCUMENT: u32 = 50;
```

- Cada error va a `worker_stderr.log` como `[console:error] <host> <mensaje>`. El mensaje pasa por el mismo saneado que el título (sin caracteres de control) y se recorta a 512 B. El host sale de la URL activa, sin ruta ni parámetros (Privacidad: no se registran rutas ni consultas).
- Al cambiar la URL del activo, el contador vuelve a 0. Si se superó el tope, antes se escribe una línea `[console:error] <host> <n> mensajes omitidos`.
- No se envía nada al Coordinador: es diagnóstico local, no estado de la UI.

### 1.4 Caída del contenido (D43)

- `notify_crashed(webview, reason, backtrace)` → `ContentCrashed(sanitize_title(reason))` → trama `MSG_WORKER_FAULT` con el texto `contenido falló: <motivo>`. El Coordinador ya la muestra como `EngineFault::WorkerFault`.
- El `backtrace` se escribe solo en `worker_stderr.log`, recortado a 8 KiB; no cruza el IPC.

### 1.5 Controles del embebedor sin soporte (D44)

- `show_embedder_control` registra **una** línea por tipo y documento: `[control:sin-soporte] <host> SelectElement|ColorPicker|FilePicker|SimpleDialog(Alert|Confirm|Prompt)|ContextMenu`. Después suelta la petición, y Servo responde lo que hace hoy (Aceptar o Cancelar, sin selección).
- `InputMethod` (IME) no se registra: es la ruta normal al enfocar un campo y pertenece a `UX-03`.
- Así hay evidencia medida de qué sitios necesitan `UX-08`, en lugar de un fallo silencioso.

## 2. CONTRATO DE PERSISTENCIA

Sin cambios. El worker sigue sin directorio de datos (`PERS-05` en el Cubo B).

## 3. CONTRATO DE ERRORES

Sin variantes nuevas en tipos públicos.

| Modo de fallo | Detección | Respuesta |
|---|---|---|
| Pila de WebView llena | `views.len() == MAX_WEBVIEW_STACK` | Se suelta la petición y se escribe una línea en `worker_stderr.log`: `[aux:rechazado] pila llena (4)` |
| `build()` del auxiliar falla o entra en pánico | El manejador de pánico existente → `WORKER_FAULT` | Igual que hoy en el WebView raíz |
| `WebViewClosed` de un id desconocido | No está en la pila | Se ignora (Servo puede notificar el cierre de un WebView ya retirado). Es la única ignorancia permitida, y está documentada |
| Contenido caído | `notify_crashed` | D43 |
| Inundación de consola | Contador por documento | D42: tope y línea de resumen |

## 4. ANTI-BLOAT FENCE

- **Sin crates nuevos ni features nuevas.** Todo usa la API pública de `servo` 0.7 que ya está vendorizada.
- **Descartado:**
  - Pila de historial propia (`Vec<Url>` + índice): Servo ya es la fuente de verdad (`notify_history_changed`).
  - Abrir los auxiliares en una ventana Win32 nueva: crea una superficie más y una ventana sin la barra de confianza del Coordinador. Espera a `UX-02`.
  - Reutilizar el WebView actual para el auxiliar: `CreateNewWebViewRequest` no lleva URL, porque el opener navega el contexto nuevo después. Sin un WebView nuevo, `window.open` no funciona.
  - Mostrar en la barra los errores de consola: sería ruido en la UI; van al log.
  - Reenviar al Coordinador `Warn`/`Info`/`Log`: ruido en una ruta caliente (YouTube emite cientos).
  - User-Agent falso, prefs experimentales, cookies persistentes, descargas: Cubo B, cada uno con su medición o decisión de privacidad.

## 5. Plan de ejecución (un `TARGET_FILE` por paso)

**Prerrequisito:** DELTA_08 paso 5 hecho (botones Atrás, Adelante y Recargar y `MSG_NAV_STATE`).

| Paso | TARGET_FILE | Resultado | Verificación |
|---|---|---|---|
| 1 | `crates/galapagos-content/src/delegates.rs` | Variantes de §1.1, filtro del activo, `request_create_new`, `notify_closed`, `notify_crashed`, `show_console_message` (D42) y `show_embedder_control` (D44) | Tests nuevos (sin tocar los existentes): `console_only_errors_are_kept_delta10`, `console_line_is_sanitized_and_capped_delta10`, `console_flood_is_summarized_per_document_delta10`, `title_and_url_from_inactive_webview_are_dropped_delta10`, `crash_reason_becomes_worker_fault_frame_delta10`. `cargo test -q` del worker |
| 2 | `crates/galapagos-content/src/main.rs` | `WebViewStack` (D41), activación, cierre, regla de `MSG_GO_BACK`, `can_back` y redimensionado de toda la pila | Tests de la pila como función pura: `aux_push_activates_and_caps_at_four_delta10`, `close_active_aux_reactivates_opener_delta10`, `go_back_without_history_closes_aux_delta10`. `cargo test -q` del worker; recompilación incremental |
| 3 | — (prueba con ventana) | El Director prueba: un enlace `target="_blank"` (p. ej. los enlaces externos de servo.org), un `window.open`, YouTube | `worker_stderr.log` tiene líneas `[console:error]` y `[control:sin-soporte]` reales de YouTube. Sus conteos se apuntan en `docs/compat/COMPAT01_FINDINGS.md`: es la entrada medida para priorizar `UX-08`, `COMPAT-02` y `COMPAT-03` |

**Avance (2026-10-26):** pasos 1 y 2 HECHOS. Worker 29/29 y Coordinador en verde, sin modificar ningún test existente. Comprobado contra los binarios reales con una página de sonda en localhost: D41 (popup creado), D42 (solo errores) y D44 (`alert`) funcionan; datos en [COMPAT01_FINDINGS.md](../docs/compat/COMPAT01_FINDINGS.md). Implementación: el redimensionado se hace sobre el activo porque Servo actualiza en esa llamada todos los WebView de la superficie (`painter.rs:1343`); la escala sí se aplica a cada WebView. El inactivo se oculta con `hide()` y cede el foco. **Siguiente:** paso 3 (prueba con ventana del Director) y decisión F-1.

**Criterio de cierre:**
- (a) Suites en verde (worker y Coordinador), sin modificar ningún test existente.
- (b) Un enlace `target="_blank"` abre su destino en la superficie, y Atrás vuelve a la página que lo abrió.
- (c) `COMPAT01_FINDINGS.md` con los datos medidos de YouTube y al menos otro sitio.
- (d) Visto bueno del Director con ventana.

## 6. Decisiones para aprobar

| Id | Decisión | ¿Requiere aprobación? |
|---|---|---|
| D41 | WebView auxiliar apilado en la misma superficie (máx. 4); Atrás sin historial cierra el auxiliar | Sí |
| D42 | Solo errores de consola, al log local, con host sin ruta, 512 B y 50 por documento | Sí |
| D43 | Caída del contenido por el canal `MSG_WORKER_FAULT` existente; backtrace solo en el log | Sí |
| D44 | Registrar los controles del embebedor sin soporte (una línea por tipo y documento) sin presentarlos todavía | Sí |
| D45 | **F-1 (aprobado el 2026-10-27):** antes de recortar a 512 B, toda URL `http(s)://` que aparezca dentro del texto de consola pierde su consulta y su fragmento, que se sustituyen por `?…`. Se conserva el sufijo `:línea:columna` que Servo añade. Motivo: el log guardó una búsqueda del usuario (`q=…`). Solo afecta a `[console:error]`; el motivo de caída (D43) no cambia | Sí (aprobada) |
| D46 | **F-2 (aprobado el 2026-10-27):** el diagnóstico de D42/D44 es **por WebView**:<br>• el host de cada línea sale de la URL actual del WebView que emite el mensaje (Servo entrega el `WebView`, no el documento ni el script);<br>• el contador y los controles vistos se vacían al empezar una carga de ese WebView (`LoadStatus::Started`), no al cambiar la URL. Así `pushState` o un cambio de `#` ya no reinician el tope de 50, que antes una página podía saltarse;<br>• la entrada se borra al cerrar el WebView, escribiendo antes su resumen de omitidos.<br>**Límite:** en una navegación dentro del mismo WebView, los mensajes tardíos del documento anterior salen con el host nuevo, porque Servo no identifica al emisor | Sí (aprobada) |

## 7. Cubo B (al backlog)

| Id | Pendiente |
|---|---|
| `UX-08` | Presentar los controles del embebedor desde el Coordinador: `<select>`, selector de archivos y `alert`/`confirm`/`prompt`, con un estilo que el contenido no pueda imitar. Necesita IPC en los dos sentidos. Priorizado con los datos de D44 |
| `PERS-05` | Cookies, almacenamiento web y caché HTTP en disco solo para el compartimento `Personal` (directorio de datos por perfil, DELTA_07). El efímero sigue en memoria |
| `COMPAT-02` | Experimento de User-Agent: medir con la sonda y una lista fija de sitios. Cambiar el valor exige modificar el test `worker_preferences_*` (Guardrail 9) |
| `COMPAT-03` | Experimento con `dom_serviceworker_enabled` y `dom_indexeddb_enabled`, medido con la sonda y con la puerta de cero `WORKER_FAULT` |
| `DEV-03` | Sonda de capacidades con `webview.evaluate_javascript`: presencia real de `serviceWorker`, `indexedDB`, `MediaSource`, `VideoDecoder`, `attachShadow`… volcada a un informe |
| `NET-03` | Descargas (`Content-Disposition`, `application/octet-stream`…) interceptadas con `load_web_resource` y guardadas por el Coordinador, con confirmación |
