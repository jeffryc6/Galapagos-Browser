# DELTA 02 — Navegación interactiva y entrada de teclado

**Estado:** CONGELADO (2026-10-25) · D6, D7, D8 aprobadas por el Director (recomendaciones) · **Pasos 1-7 ejecutados** (paso 8, tests de `tests/`, pendiente). Smoke test contra binario real: **omitido por decisión del Director**; los binarios (debug/MSIX) se reconstruyen al final; sigue sin verificación de integración · **Origen:** auditoría `/znve-audit` de `galapagos-content/src` + `app_trace.log` (2026-10-24) · **Extiende:** `contracts/galapagos_core_v1.rs` y `DELTA_01` (solo aditivo; ninguna firma ni variante existente cambia)

## 0. Síntoma y causa raíz (verificada leyendo código)

La barra de navegación "no responde" por **tres roturas independientes en cadena**; arreglar solo una no alcanza.

| Id | Hallazgo | Evidencia | Sev. | Cubo |
|---|---|---|---|---|
| N-1 | La UI no genera comandos. `wnd_proc` solo hace `trace_os_event` en `WM_COMMAND` (botón Navegar y `EN_*` del edit) y retorna 0. Nunca lee la URL ni llama a `dispatch_navigation`. | `src/window/mod.rs:236-251`; log: 4 clics en Navegar sin efecto | Crítica | A |
| N-2 | Enter en un `EDIT` de una línea no emite `WM_COMMAND`; `TranslateMessage`/`DispatchMessageW` lo descartan. No hay pre-traducción de `VK_RETURN`. | `src/window/mod.rs:211-222`; log: sin evento de Enter | Crítica | A |
| N-3 | La única navegación ocurre una vez, en `main()`, desde argumentos CLI o `https://servo.org`, antes del bucle de mensajes. El bucle no tiene ningún camino hacia `coordinator`/`engine`. | `src/main.rs:205-359` | Crítica | A |
| N-4 | El worker descarta `TitleChanged`/`UrlChanged`; el Coordinador no tiene mensaje para recibirlos ni bucle que lea el pipe. La barra y el título nunca reflejan el estado real (redirecciones, enlaces clicados). | `galapagos-content/src/main.rs:250-251`; `ipc/dispatcher.rs:25-29` (solo tipos 1-5) | Alta | A |
| N-5 | El teclado llega al worker sin identidad: `Key::Unidentified`, `Code::Unidentified`, modificadores vacíos. La página nunca ve Enter, Tab ni flechas. | `galapagos-content/src/input.rs:60-76` | Crítica | A |
| N-6 | No existe evento de texto (`Char`). Aun con N-5 resuelto, no se puede teclear en inputs de la página. | `engine/input_dispatcher.rs:243-258` (solo `WM_KEYDOWN/UP`) | Alta | A |
| N-7 | El Coordinador no reenvía `WM_KEYDOWN/UP`, ratón ni rueda de la superficie al worker: `handle_win32_input` no tiene llamadores. Además `dispatch_input` descarta `MouseMove`/`Wheel`/`MouseUp`/`Resize` (`_ => return Ok(())`). | `engine/mod.rs:322`, `:330-343` | Alta | A (teclado, clic) · B (move/wheel) |
| N-8 | `validate_url_scheme` rechaza texto sin esquema (`servo.org`). Sin normalización, escribir un dominio falla siempre. | `galapagos_core_v1.rs:185-201` | Media | A |
| N-9 | Worker: un mensaje IPC por vuelta + espera 16 ms ⇒ tope ≈ 60 msg/s; `payload_len` sin tope (alloc hasta 4 GiB); lectura bloqueante de tramas parciales. | `galapagos-content/src/main.rs:273-310`, `:278`, `:80-99` | Alta | A (tope y vaciado) · B (I/O solapada) |

## 1. CONTRATO DE ENTRADA Y SALIDA (aditivo)

```rust
// contracts/galapagos_core_v1.rs — adiciones

/// Origen del texto escrito en la barra de direcciones. Cerrado: no hay búsqueda web en este delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressBarInput { pub raw: String }          // 1..=2048 bytes UTF-8, validado en frontera

/// Resultado determinista de normalizar `AddressBarInput` → URL absoluta validable.
/// Regla (única, sin heurísticas de búsqueda):
///   1. trim; vacío → Err(MalformedMessage)
///   2. contiene "://" o == "about:blank" → se usa tal cual
///   3. forma `esquema:resto` sin "//" y resto no numérico (javascript:, data:, file:…) → InvalidUrlScheme
///   4. si no, se antepone "https://" (cubre `servo.org` y `localhost:8080`)
///   5. el resultado pasa SIEMPRE por `UiToCoordinatorCmd::validate_url_scheme`
pub fn normalize_address_input(input: &AddressBarInput) -> Result<String, IpcViolationReason>;

/// NO vive en el contrato núcleo (este no puede depender de `InputEvent`, que es de `engine`).
/// Se define en `src/window/mod.rs` (paso 5):
///   pub enum UiEvent { NavigateRequested(AddressBarInput), SurfaceInput(InputEvent), CloseRequested }
/// y `InputEvent` gana la variante `Char { scalar: char }` en `engine/input_dispatcher.rs` (paso 6).

/// Extensión de ContentToCoordinatorIpc (variantes nuevas al final).
///   UrlChanged   { tab_id: TabId, url: String }     // MSG_TYPE_URL_CHANGED   = 6, url ≤ 2048 B
///   TitleChanged { tab_id: TabId, title: String }   // MSG_TYPE_TITLE_CHANGED = 7, title ≤ 512 B (truncado en UTF-8 válido)
```

### Protocolo IPC (aditivo; tipos 6 y 7 libres; 12 ya existe)

| Dirección | Tipo | Payload (BE) |
|---|---|---|
| Worker → Coord. | 6 `URL_CHANGED` | `tab_id u64 · len u16 · url` |
| Worker → Coord. | 7 `TITLE_CHANGED` | `tab_id u64 · len u16 · title` |
| Coord. → Worker | 12 `INPUT_EVENT` evento 1/2 (KeyDown/KeyUp) | **sin cambio de forma**: `[tipo u8][key_code u32][modifiers u8]`; el worker **debe** mapear `key_code` (VK) → `Key`/`Code` y `modifiers` → `Modifiers` |
| Coord. → Worker | 12 `INPUT_EVENT` evento 7 `Char` | `[7][scalar u32]` — valor escalar Unicode (≤ U+10FFFF, sin sustitutos); inválido ⇒ descartar con telemetría |

Tabla VK→Key cerrada de esta versión: `VK_RETURN→Enter`, `VK_BACK→Backspace`, `VK_TAB→Tab`, `VK_ESCAPE→Escape`, `VK_DELETE`, flechas, `VK_HOME/END/PRIOR/NEXT`, `VK_SPACE`. Cualquier otro VK ⇒ `Key::Named(Unidentified)` **pero** con `Code` y modificadores correctos; el texto llega por `Char`.

## 2. Invariantes

- **N-I1** La UI nunca invoca `dispatch_navigation` directamente: emite `UiEvent`; el bucle principal valida (`normalize_address_input` → `validate_url_scheme` → Ocean Defense `evaluate` → `coordinator.navigate` → `dispatch_navigation`), en ese orden. Mismo camino que hoy usa `main.rs:225-308`, extraído a una función única reutilizada por CLI y UI.
- **N-I2** Enter en la barra y clic en Navegar producen **exactamente un** `UiEvent::NavigateRequested`. Enter se intercepta antes de `TranslateMessage` cuando `msg.hwnd == url_edit_hwnd`, `message == WM_KEYDOWN` y `wParam == VK_RETURN`, y no se propaga al `EDIT` (sin beep).
- **N-I3** Veredicto `Block`/`RequireConfirmation`/`Unavailable` ⇒ no se envía nada al worker; la etiqueta de estado se actualiza (ya existe `set_security_status`).
- **N-I4** La barra se actualiza con `UrlChanged` del worker (fuente de verdad tras redirecciones), no con el texto tecleado. Mientras el edit tenga foco, `UrlChanged` no sobrescribe el texto en edición.
- **N-I5** El bucle de mensajes sirve al pipe sin bloquear la UI: temporizador `WM_TIMER` de 16 ms que hace lectura **no bloqueante** (`PeekNamedPipe` + lectura solo si hay trama completa). Sin `sleep`. Se vacía el pipe hasta agotar o hasta 64 tramas por tick.
- **N-I6** Worker: `payload_len ≤ 1 MiB` (coherente con H4 y `ResourceQuota`); mayor ⇒ cerrar el pipe y salir con código 6. Se vacían todos los comandos disponibles antes de esperar.
- **N-I7** Teclas: el foco de teclado decide el destino. Foco en el edit ⇒ el edit; foco en la superficie ⇒ worker. Nunca ambos.
- **N-I8** Mensajes IPC con `tab_id` distinto del tab del canal ⇒ `IpcBoundaryViolation { MalformedMessage }` (el contenido es hostil, ARC-02).

## 3. CONTRATO DE PERSISTENCIA

Sin cambios. No se persiste historial en este delta (ver Cubo B).

## 4. CONTRATO DE ERRORES

Sin variantes nuevas. Mapeo:
- Texto vacío / >2048 B / carácter de control ⇒ `IpcBoundaryViolation { MalformedMessage | PayloadTooLarge | ControlCharacterDetected }`, mostrado en la etiqueta de estado, nada llega al worker.
- Esquema no permitido ⇒ `InvalidUrlScheme` (ya existente).
- Trama entrante >1 MiB, tipo desconocido o `tab_id` ajeno ⇒ `IpcBoundaryViolation` y el worker/coordinador cierra el canal.
- Fallo de envío por el pipe ⇒ se propaga (hoy `dispatch_input` se traga el error con `let _ =` en `handle_win32_input`; debe reportarse por telemetría de excepción).

## 5. ANTI-BLOAT FENCE

- **Entra:** nada. Cero dependencias nuevas. Win32 vía `windows-sys` ya presente (`PeekMessageW`/`SetTimer`/`VK_*`).
- **Prohibido:** `SetWindowSubclass`/comctl32 (basta pre-traducir en el bucle propio), crates de teclado (`winit`, `keyboard-types` ya viene con Servo), runtime async.
- **Descartado:** búsqueda por defecto desde la barra (implica red y política de privacidad: ver ENG-06), autocompletado, IME.
- **Campos descartados:** `UiEvent::Navigate{tab_id}` (una sola pestaña activa en v1), historial en el DTO.

## 6. Plan de ejecución (un `TARGET_FILE` por paso)

| Paso | TARGET_FILE | Depende de | Cubre |
|---|---|---|---|
| 1 | `contracts/galapagos_core_v1.rs` (`AddressBarInput`, `normalize_address_input`, `MAX_ALLOWED_TITLE_LEN`, variantes `UrlChanged`/`TitleChanged`) — **HECHO**: 7/7 tests del archivo en verde compilado aislado. El crate raíz queda sin compilar hasta el paso 2 (los `match` exhaustivos de `ipc/dispatcher.rs` líneas 87 y 331 requieren las variantes nuevas) | aprobación | — |
| 2 | `src/ipc/dispatcher.rs` (tipos 6/7: encode/decode, límites) | 1 | N-4 |
| 3 | `crates/galapagos-content/src/input.rs` (VK→Key/Code/Modifiers, evento `Char`, tests) | 1 | N-5, N-6 |
| 4 | `crates/galapagos-content/src/main.rs` (tope `payload_len`, vaciado del pipe, envío de URL/Title) y `delegates.rs` | 2, 3 | N-4, N-9 |
| 5 | `src/window/mod.rs` (`UiEvent`, intercepción Enter, `BN_CLICKED`, `WM_CHAR`, `get_url`/`set_url` sin sobrescribir con foco) | 1 | N-1, N-2, N-7 |
| 6 | `src/engine/input_dispatcher.rs` + `src/engine/mod.rs` (`Char`, `dispatch_input` sin tragar errores, reenvío de `UiEvent::SurfaceInput`) | 3, 5 | N-6, N-7 |
| 6b | **D8 / UX-04:** `src/engine/mod.rs` (`dispatch_input` serializa `MouseUp` tipo 4, `MouseMove` tipo 5, `MouseWheel` tipo 6 según el decodificador del worker) y `src/window/mod.rs` (`WM_MOUSEMOVE`, `WM_MOUSEWHEEL`, `WM_LBUTTONUP` sobre la superficie). Coordenadas con signo (`i32`) | 6 | N-7 |
| 7 | `src/main.rs` (extraer `navigate_checked()`, bucle con `WM_TIMER`, aplicar `UrlChanged`/`TitleChanged`) | 2, 4, 5, 6 | N-1, N-3, N-4 |
| 8 | `tests/`: Enter ⇒ 1 `NavigateRequested`; `servo.org` ⇒ `https://servo.org`; `javascript:`/`data:` rechazados; trama 2 MiB ⇒ canal cerrado; golden de VK_RETURN→`Key::Named(Enter)` | 2-7 | todos |

**Verificación de integración (Guardrail 9):** «integrado» solo tras un smoke test contra el binario real: teclear en una página con un `<input>` y confirmar el texto; navegar con Enter y verificar `UrlChanged` en `app_trace.log`. Sin ese test: **NO INTEGRADO**.

## 7. Diferido (Cubo B → `CONTRACT_BACKLOG.md`)

| Código | Descripción |
|---|---|
| `UX-01` | Atrás/Adelante/Recargar/Detener y estado de carga. |
| `UX-02` | Barra de pestañas (la "lista fija" de la UI actual) y multi-pestaña real. |
| `UX-03` | IME y composición (`WM_IME_*`); este delta solo cubre `WM_CHAR`. |
| `UX-04` | **PROMOVIDO a Cubo A (D8)** → paso 6b. Queda diferido solo el cambio de cursor. |
| `ENG-06` | Búsqueda desde la barra y autocompletado (decisión de privacidad/red). |
| `ENG-07` | I/O solapada en el worker (reemplaza el sondeo de 16 ms). |
| `SEC-04` | Retirar `data:` de navegaciones de nivel superior en `request_navigation` del worker. Sale de este delta por ser independiente de la interactividad; severidad media, recomendado en el siguiente. |

## 8. Decisiones para el Director

| # | Decisión | Recomendación |
|---|---|---|
| D6 | Interceptar Enter en el bucle propio (sin dependencias) vs. `SetWindowSubclass` | **Aprobado:** bucle propio (N-I2) |
| D7 | Normalizar dominios sin esquema a `https://` vs. exigir esquema escrito | **Aprobado:** normalizar a `https://` (regla única, §1) |
| D8 | Promover `UX-04` (scroll/mover ratón) a este delta | **Aprobado:** paso 6b |

**Contrato Δ2 sólido y cerrado. Siguiente: /znve-execute --target=src/ipc/dispatcher.rs (paso 2).**
