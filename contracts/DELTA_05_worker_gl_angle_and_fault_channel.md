# DELTA 05 — Contexto GL del worker (ANGLE / `no-wgl`) y canal de fallos del worker

**Estado:** CONGELADO (2026-10-25) · D16, D17, D18, D19 aprobadas por el Director (recomendaciones) · **Origen:** `app_trace.log` (2026-10-25 20:59 UTC) + sonda con stderr capturado · **Extiende:** `DELTA_01` (H1-H7), `DELTA_04` (solo aditivo; ninguna firma ni variante existente cambia)

## 0. Evidencia y causa raíz

| Hecho | Evidencia |
|---|---|
| El worker vigente (14:55 local) cae ≈0,4 s después de enlazarse, con código **101** (panic de Rust) | `CONTENT_CHANNEL_VIOLATION ... worker terminó con código 101 (0x65) \| binario: ...galapagos-content.exe (modificado 2026-10-25T20:55:28Z)` |
| El panic ocurre al crear el contexto GL, **antes** de procesar ningún comando | Sonda (test temporal ya borrado) que lanzó el worker real con stderr a archivo: `glow-0.17.0\src\native.rs:69: Reading GL_VERSION failed. Make sure there is a valid GL context currently active.` ← `SurfmanRenderingContext::new` (`vendor/servo/.../rendering_context.rs:152`) ← `WindowRenderingContext::new` ← `renderer.rs:60` ← `main.rs:284` |
| No depende de la ventana ajena | Control con `--surface 0`: el mismo panic en `SoftwareRenderingContext::new` (`renderer.rs:75`) |
| El respaldo de `renderer.rs` nunca actúa | El fallo es un `panic!` dentro de Servo, no un `Err`; no se puede capturar con `match` |
| El worker no usa ANGLE | `crates/galapagos-content/Cargo.toml`: `servo = { ..., features = ["bundled","multiprocess","js_jit"] }`. Sin `no-wgl`, Servo usa WGL (OpenGL del sistema). `vendor/servo/ports/servoshell/Cargo.toml:154` (el embedder de referencia) activa `no-wgl` |
| El motivo del panic solo es visible con una sonda | El worker no tiene consola ni log; el Coordinador solo conoce el código de salida |

**Hipótesis a verificar (no probada):** `no-wgl` (ANGLE vía `mozangle`) resuelve el panic. Es lo que usa el embedder de referencia de Servo, pero no se ha compilado ni ejecutado en este equipo. El paso 8 es la puerta de verificación; si el panic persiste, el canal de fallos del paso 1-5 ya entregará el motivo real al log del Coordinador.

## 1. Auditoría contratos ↔ src

| Id | Discrepancia | Sev. | Cubo |
|---|---|---|---|
| G-1 | Worker sin `no-wgl`: no crea contexto GL en este equipo; el navegador no renderiza ni navega | Crítica | A |
| F-1 | Un panic del worker solo deja el código 101; el motivo se pierde (Guardrail 6: no hay supresión, pero tampoco telemetría por excepción) | Alta | A |
| F-2 | `renderer.rs` degrada a contexto de software **offscreen** cuando falla el de ventana: éxito aparente sin imagen (Guardrail 9) | Alta | A (D16) |
| P-1 | Las DLL de ANGLE (`libEGL.dll`, `libGLESv2.dll`) no se copian junto al worker ni al paquete MSIX; `build_msix.ps1` solo copia dos `.exe` | Alta | A |
| P-2 | Faltando una DLL, el fallo sería un panic o `LoadLibrary` opaco, no un error legible | Media | A |
| E-1 | Tras morir el worker, navegar devuelve `MalformedMessage` ("estructura corrupta"), engañoso | Baja | B |

## 2. CONTRATO DE ENTRADA Y SALIDA (Cubo A, aditivo)

```rust
// contracts/galapagos_core_v1.rs — adición (variante al final de ContentToCoordinatorIpc)
/// Worker -> Coordinador, justo antes de terminar por un fallo propio (MSG_TYPE_WORKER_FAULT = 8).
/// Texto plano <= 512 B (MAX_ALLOWED_TITLE_LEN), UTF-8 válido, sin caracteres de control.
WorkerFault { tab_id: TabId, message: String }
```

| Dirección | Tipo | Payload (BE) |
|---|---|---|
| Worker → Coord. | 8 `WORKER_FAULT` | `tab_id u64 · len u16 · message` (mismo formato que los tipos 6 y 7) |

### Códigos de salida del worker (tabla cerrada)
| Código | Significado |
|---|---|
| 1 | Sin nombre de pipe ni `--tab-id` |
| 2 | No se pudo conectar al pipe |
| 3 | No se pudo escribir el handshake |
| 4 | No se pudo crear el waker |
| 5 | Fallo al crear el contexto de render (error o panic) — **envía `WORKER_FAULT`** |
| 6 | Trama entrante > 1 MiB |
| 7 | **Nuevo:** faltan `libEGL.dll` o `libGLESv2.dll` junto al worker — envía `WORKER_FAULT` |
| 101 | Cualquier otro panic — envía `WORKER_FAULT` desde el gancho |

### Invariantes
- **A-I1** `crates/galapagos-content/Cargo.toml` activa `no-wgl` en `servo`. Sin otras dependencias nuevas fuera del árbol de Servo (`mozangle` ya es dependencia opcional de `servo`).
- **A-I2** Antes de inicializar Servo, el worker comprueba que `libEGL.dll` y `libGLESv2.dll` existen en el directorio de su ejecutable. Si falta alguna: envía `WORKER_FAULT` ("faltan DLL de ANGLE: ...") y sale con código 7. No hay panic ni `LoadLibrary` opaco.
- **A-I3** El gancho de panic se instala antes que cualquier otro paso posterior al handshake. Formatea `ubicación: mensaje` (sin backtrace), reemplaza caracteres de control por espacio, recorta a 512 B en límite UTF-8, escribe **una** trama tipo 8 por el pipe y deja que el gancho estándar siga (código 101). Si la escritura falla, no entra en pánico de nuevo.
- **A-I4** La creación del contexto de render se ejecuta bajo `catch_unwind`: un panic o `Err` termina con código 5 y `WORKER_FAULT`. Con `--surface <hwnd>` ≠ 0 **no** hay respaldo a software offscreen (D16). Con `--surface 0` (modo sin ventana de pruebas) se conserva `SoftwareRenderingContext`.
- **A-I5** El Coordinador decodifica el tipo 8 con los mismos controles que 6 y 7 (longitud exacta, ≤ 512 B, UTF-8, sin controles, `tab_id` = el del canal, N-I8). `pump_worker` registra `WORKER_FAULT` (`log_error`, categoría `Ipc`) con el mensaje íntegro y muestra `[Motor falló]` en la etiqueta de estado. Una trama 8 repetida o posterior al primer fallo se ignora.
- **A-I6** Empaquetado: `libEGL.dll` y `libGLESv2.dll` (salida de `mozangle`, `OUT_DIR`) se copian junto a `galapagos-content.exe` en desarrollo (`crates/galapagos-content/target/<perfil>/`) y en `packaging/staging/`. `build_msix.ps1` falla si no están (igual que ya hace con los `.exe`). `d3dcompiler_47.dll` se toma de System32 (presente en Windows 10/11); no se redistribuye.

## 3. CONTRATO DE PERSISTENCIA

Sin cambios.

## 4. CONTRATO DE ERRORES

Sin variantes nuevas de `GalapagosError`. Mensaje 8 inválido ⇒ `IpcBoundaryViolation { MalformedMessage | PayloadTooLarge | ControlCharacterDetected }`, y el Coordinador cierra el canal como con 6 y 7.

## 5. ANTI-BLOAT FENCE

- **Entra:** la característica `no-wgl` de `servo` (arrastra `mozangle` y compila ANGLE: unos minutos más de build y 2 DLL en el paquete) y un mensaje IPC.
- **Justificación de ANGLE:** WGL no inicializa en este equipo; ANGLE es el backend que Servo usa en Windows en su embedder de referencia. Alternativa nativa descartada: WGL (falla).
- **Descartado:** archivo de log escrito por el worker (no hay carpeta escribible garantizada en AppContainer y habría que gestionar rutas/rotación); variante de error nueva; un `Stub` de render; reintentos o `sleep` para tapar el arranque.
- **Prohibido:** seguir declarando "integrado" un worker que no ha mostrado al menos un cuadro (Guardrail 9).

## 6. Plan de ejecución (un `TARGET_FILE` por paso)

| Paso | TARGET_FILE | Cubre |
|---|---|---|
| 1 | `contracts/galapagos_core_v1.rs` (`WorkerFault`) | F-1 |
| 2 | `src/ipc/dispatcher.rs` (tipo 8: codec) | F-1 |
| 3 | `src/engine/mod.rs` (`content_event_tab` para `WorkerFault`) | F-1 |
| 4 | `src/main.rs` (`pump_worker`: `WORKER_FAULT` → log + etiqueta) | F-1 |
| 5 | `crates/galapagos-content/src/main.rs` + `renderer.rs` (gancho de panic, comprobación de DLL, `catch_unwind`, códigos 5 y 7, sin respaldo offscreen con HWND) | F-1, F-2, P-2 |
| 6 | `crates/galapagos-content/Cargo.toml` (`no-wgl`; se regenera `Cargo.lock`) | G-1 |
| 7 | `packaging/stage_angle_dlls.ps1` (nuevo: copia las DLL de `mozangle` junto al worker) y `packaging/build_msix.ps1` (las incluye en `staging/` y falla si faltan) | P-1 |
| 8 | **Verificación con binario real** (§8) | G-1 |

Pruebas (D14): solo lógica pura — codec del tipo 8, saneado y recorte del mensaje del gancho, comprobación de DLL sobre una lista de rutas, tabla de códigos de salida. Ninguna prueba lanza procesos.

**Ejecución (2026-10-25):** pasos 1-7 hechos; `cargo test` del Coordinador en verde (92 lib, 94 bin, 6 y 3 de integración) y del worker (12). Paso 8 parcial: el worker con `no-wgl` compiló (7 min 53 s, `mozangle v0.7.1` añadido al `Cargo.lock` del worker) y las DLL quedaron junto a él (`stage_angle_dlls.ps1`).

**Smoke test del worker real (fuera del Coordinador).** Sonda PowerShell: servidor de pipe en modo mensaje (.NET `NamedPipeServerStream`), ventana WinForms de otro proceso como `--surface`, worker de 15:18:
- Sin `no-wgl` (worker de 14:55): panic 101 en `glow ... GL_VERSION` a los ≈0,4 s.
- Con `no-wgl`: el worker **sobrevive los 30 s** observados, sin stderr. Tramas: handshake (5), `UrlChanged about:blank` (6), `FrameReady` (1) repetidas. Tras enviar `Navigate https://example.com`: `UrlChanged https://example.com/`, `TitleChanged "Example Domain"` y más `FrameReady`.
- Conclusión: la hipótesis queda **confirmada para el worker**: ANGLE crea el contexto sobre un HWND de otro proceso, la página carga y se pintan cuadros.
- **Pendiente:** verificar el Coordinador completo (`galapagos-browser.exe`) con este worker. El árbol fuente actual del Coordinador no compila por un trabajo ajeno en curso en `window/mod.rs` (import de `DrawTextW`); el `.exe` de 15:22 sí contiene los pasos 1-4. Hasta esa prueba, el estado global sigue **NO INTEGRADO**.

## 7. Diferido (Cubo B → `CONTRACT_BACKLOG.md`)

| Código | Descripción |
|---|---|
| `ENG-10` | Navegar con el worker terminado devuelve `MalformedMessage`; debería indicar canal caído (`E-1`). |
| `ENG-02` | (ya existente) transporte de cuadros entre procesos: la renderización directa a un HWND ajeno puede no funcionar con ANGLE y habrá que re-evaluarla en el paso 8. |

## 8. Verificación y riesgos

1. Compilar el worker con `no-wgl` (varios minutos; requiere MSVC y Python, ya presentes) y ejecutar `packaging/stage_angle_dlls.ps1`.
2. Abrir la app: `WORKER_BOUND` y **ausencia** de `CONTENT_CHANNEL_VIOLATION` durante ≥ 30 s; escribir `example.com` + Enter: `UrlChanged` y `TitleChanged` en `app_trace.log` y página visible.
3. **Si el worker vuelve a caer:** el log mostrará `WORKER_FAULT` con el motivo real (A-I3/A-I4). Con eso se decide entre ajustes de ANGLE/GPU o pasar a `ENG-02`.
4. **Riesgo conocido:** crear el contexto sobre un HWND de otro proceso puede seguir fallando incluso con ANGLE; el paso 8 lo confirma o descarta.
5. Hasta ejecutar el paso 8 el estado sigue **NO INTEGRADO**.

## 9. Decisiones para el Director

| # | Decisión | Recomendación |
|---|---|---|
| D16 | Con ventana real, eliminar el respaldo a `SoftwareRenderingContext` offscreen (falla explícita, código 5) | Sí (Guardrail 9); se conserva solo con `--surface 0` |
| D17 | Capturar el panic por el pipe (mensaje 8) vs. escribir un archivo de log desde el worker | Pipe |
| D18 | Activar `no-wgl`: build más largo (ANGLE) y dos DLL en el paquete | Sí |
| D19 | Script de copia de DLL (`stage_angle_dlls.ps1`) y `build_msix.ps1` que falla sin ellas | Sí |

**Contrato Δ5 en borrador. Responde D16-D19 (o acepta las recomendaciones) para congelarlo y pasar a /znve-execute.**
