# DELTA 06 — Desbordamiento de pila del worker: causa, observabilidad y mitigación

**Estado:** CONGELADO (2026-10-25) · D20, D21, D22 aprobadas por el Director (recomendaciones) · **Origen:** `app_trace.log` (2026-10-25 21:43-21:44 UTC), Registro de eventos de Windows (WER) y sondas · **Extiende:** `DELTA_04`, `DELTA_05` (solo aditivo)

## 0. Qué pasó

El Director cargó `google.com` con el worker con ANGLE (DELTA_05): la página se pintó, aunque incompleta. A los 73 s, mientras llegaban eventos de entrada, el worker murió; desde entonces toda navegación (`vix.com`, `youtube.com`) falló porque ya no había worker.

| Hecho | Evidencia |
|---|---|
| El worker funcionó 73 s y murió durante el envío de una entrada | `21:43:22 WORKER_BOUND` → `21:44:35 PIPE_IO_FAILED WriteFile (Win32 232)` + `INPUT_DISPATCH_FAILED` → `CONTENT_CHANNEL_VIOLATION` |
| Código de salida **0xC00000FD = `STATUS_STACK_OVERFLOW`** | `worker terminó con código 3221225725 (0xC00000FD)`; WER `APPCRASH`, excepción `c00000fd` en `galapagos-content.exe` |
| Se detuvo en la sonda de pila de la CRT | Símbolo del RVA `0x8D88097` con el PDB: `_alloca_probe` (`chkstk.asm`): una función con un marco de pila enorme (compilación **debug**) agotó la pila. El hilo no se puede saber con estos datos |
| **No se registró `WORKER_FAULT`** | Un desbordamiento de pila no es un panic de Rust: el gancho de DELTA_05 no se ejecuta |
| Los errores de navegación posteriores son consecuencia | `NAVIGATION_DISPATCH_FAILED` desde 21:44:51, después de la caída |
| Cargar `google.com` solo no lo reproduce | Sonda aparte: 100 s en `google.com`, 120 s con clics y tecleo → worker vivo. El disparador real sigue **sin identificar** |
| Las entradas llegan a la página, pero Enter en la caja de búsqueda no navegó en la sonda | Tras `vix.com` + Enter no hubo `UrlChanged` nuevo; no se verificó el foco del cuadro de texto |

Contexto: Servo compilado en **debug** usa marcos de pila mucho mayores y es muy lento, lo que explica en parte el "contenido incompleto". El hilo principal de Windows tiene 1 MiB de pila por omisión; el hilo de scripts de Servo ya usa 8 MiB (`script_thread.rs:490`).

## 1. Auditoría contratos ↔ src

| Id | Discrepancia | Sev. | Cubo |
|---|---|---|---|
| X-1 | Un desbordamiento de pila no deja motivo ni hilo: el worker muere sin `WORKER_FAULT` | Alta | A |
| X-2 | El stderr del worker se pierde (no hay consola); el aviso "thread '…' has overflowed its stack" de Rust nunca se ve | Alta | A |
| X-3 | Pila del hilo principal del worker = 1 MiB por omisión | Alta | A (D21) |
| X-4 | La entrada hacia la página (foco en cuadros de texto, Enter en formularios) no está verificada de extremo a extremo | Media | B |

## 2. CONTRATO DE ENTRADA Y SALIDA (Cubo A, aditivo)

Sin mensajes IPC nuevos: se reutiliza `WORKER_FAULT` (tipo 8, DELTA_05).

### Invariantes
- **X-I1** El worker registra un manejador de excepciones vectorizado para `EXCEPTION_STACK_OVERFLOW` (0xC00000FD). Sin asignar memoria en el manejador, envía una trama `WORKER_FAULT` con el texto `desbordamiento de pila en hilo <id>` (y el nombre del hilo si la API lo permite sin asignar) y devuelve `EXCEPTION_CONTINUE_SEARCH`, de modo que Rust conserve su aviso por stderr y el proceso termine con 0xC00000FD. Si el envío falla, no vuelve a fallar.
- **X-I2** (D20) Al arrancar, el worker redirige su stderr (`SetStdHandle`) a `worker_stderr.log` en `%LOCALAPPDATA%\GalapagosBrowser\logs\`, abierto en modo añadir. Si no puede abrirlo (p. ej., AppContainer futuro), sigue sin archivo y lo registra en su primer `WORKER_FAULT` solo si hay un fallo posterior; no aborta. Sin rotación en este delta (tope: si el archivo supera 5 MiB al arrancar, se trunca).
- **X-I3** (D21) El enlace del worker reserva una pila de **16 MiB** para el hilo principal mediante `build.rs` del crate del worker (`cargo:rustc-link-arg-bins=/STACK:16777216`). No modifica `RUSTFLAGS`: no obliga a recompilar las dependencias.
- **X-I4** El Coordinador no cambia: `pump_worker` ya registra `WORKER_FAULT` y, tras la caída, `CONTENT_CHANNEL_VIOLATION` con el código de salida.

## 3. CONTRATO DE PERSISTENCIA

`worker_stderr.log` es un archivo de diagnóstico de texto, no una base de datos; mismo directorio que `app_trace.log`.

## 4. CONTRATO DE ERRORES

Sin variantes nuevas.

## 5. ANTI-BLOAT FENCE

- **Entra:** un manejador de excepciones (`windows-sys`, ya dependencia), un archivo de log y un `build.rs` de dos líneas.
- **Descartado:** subir la pila con `RUSTFLAGS` (recompila todo Servo); crear un hilo nuevo para el bucle del worker (`Rc`/objetos de Servo no son `Send`; obliga a mover toda la inicialización); volcados de memoria (requieren registro de sistema con administrador); `catch_unwind` (un desbordamiento de pila no se captura).
- **Prohibido:** declarar resuelto el cierre mientras no se vea el hilo responsable en `worker_stderr.log`.

## 6. Plan de ejecución (un `TARGET_FILE` por paso)

| Paso | TARGET_FILE | Cubre |
|---|---|---|
| 1 | `crates/galapagos-content/src/main.rs` (redirección de stderr, manejador vectorizado, pruebas de lógica pura) | X-1, X-2 |
| 2 | `crates/galapagos-content/build.rs` (nuevo: `/STACK:16777216`) | X-3 |
| 3 | Recompilar el worker (solo relinkea y compila el crate del worker) | — |
| 4 | **Prueba del Director:** repetir la sesión de `google.com` con el mismo guion; enviar `app_trace.log` y `worker_stderr.log` | X-1, X-2 |

Pruebas (D14): lógica pura — formato del texto del fallo, apertura/truncado del archivo de log, construcción de la trama. Ninguna prueba lanza procesos.

## 7. Diferido (Cubo B → `CONTRACT_BACKLOG.md`)

| Código | Descripción |
|---|---|
| `ENG-11` | Verificar la entrada hacia la página de extremo a extremo (foco del cuadro de texto, Enter que envía formularios, doble `KeyDown` por tecla). |
| `PERF-03` | Validar la experiencia con el worker en `--release`: Servo en debug es mucho más lento y consume más pila. |

## 8. Verificación y riesgos

- X-I3 es una **mitigación**, no una causa demostrada: si el hilo que se desborda no es el principal, la pila de 16 MiB no lo arregla. Por eso X-I1 y X-I2 van primero: el nombre del hilo en `worker_stderr.log` decide el arreglo definitivo (p. ej., compilar en release o reducir un marco concreto).
- Mientras no se repita la sesión (paso 4), el estado sigue **NO INTEGRADO**.

## 9. Decisiones para el Director

| # | Decisión | Recomendación |
|---|---|---|
| D20 | `worker_stderr.log` escrito por el propio worker en la carpeta de logs | Sí |
| D21 | Pila de 16 MiB para el hilo principal del worker vía `build.rs` | Sí (mitigación barata) |
| D22 | Manejador vectorizado de desbordamiento de pila que envía `WORKER_FAULT` | Sí |

**Contrato Δ6 en borrador. Responde D20-D22 (o acepta las recomendaciones) para congelarlo y pasar a /znve-execute.**
