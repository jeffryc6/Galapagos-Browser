# Arnés COMPAT — Golden Master del worker Servo

**Capturado:** 2026-10-27 con los binarios del 2026-10-26: Coordinador `target/debug/galapagos-browser.exe` y worker `crates/galapagos-content/target/debug/galapagos-content.exe` (129 MB, 16:54). **Estado:** 100 % verde en 3 ejecuciones idénticas (captura y dos verificaciones). Sin cambios en el código de producción.

## 1. Configuración de aislamiento

- **Ejecución:**
  - el navegador real, en frío y sin modificar, mediante `packaging/measure_load.ps1`, que escribe la URL en la barra y espera a que el worker quede en reposo;
  - las sondas, servidas desde `probes/` por `System.Net.HttpListener` en `http://localhost:8765/`, en un job de PowerShell que se detiene al terminar. No se añade ninguna dependencia.
- **Canal de observación:** solo lo que el worker ya registra en `%LOCALAPPDATA%\GalapagosBrowser\logs\worker_stderr.log`: las líneas `[console:error]`, `[control:sin-soporte]`, `[aux:rechazado]` y `[content:crash]` de DELTA_10. Las sondas informan por `console.error("[probe:…] k=v;…")` en trozos de 380 B o menos. También se cuentan `WORKER_FAULT` y `CHANNEL_VIOLATION` nuevos en `app_trace.log`.
- **Normalización:**
  - las líneas se ordenan en orden ordinal (el orden asíncrono entre WebView no es estable);
  - se enmascara el puerto (`localhost:PORT`);
  - las sondas no emiten tiempos, PID, idioma, escala ni zona horaria.
- **Completitud:** cada sonda termina con `[probe:done] <nombre>`. Sin ese marcador, el resultado es FALLO y no se captura.
- **Aviso:** el arnés cierra cualquier `galapagos-browser`/`galapagos-content` abierto.

## 2. Batería de inyección

| Sonda | Qué cubre |
|---|---|
| `caps.html` | 60 capacidades: DOM moderno, multimedia (MSE, EME, `canPlayType`), gráficos (WebGL/WebGPU), cripto, almacenamiento, red, JavaScript, `CSS.supports` y el User-Agent |
| `layout.html` | 13 medidas de **render real** en cajas de px fijos: flex, grid (`fr`, `gap`, `minmax`), `aspect-ratio`, `:has()`, container queries, `clamp()`, variables, propiedades lógicas, `transform`, tabla fija e `inset` |
| `errors.html` | Límites de D42: niveles descartados, mensaje vacío, caracteres de control, Unicode, más de 512 B, objeto, error no capturado, promesa rechazada e inundación de 60 mensajes (tope de 50) |
| `popups.html` | `alert`/`confirm`/`prompt` sin soporte y 5 `window.open` en el mismo turno, uno a `data:` (tope de la pila D41) |
| `crash.html` (DELTA_11) | Pánico real de página con `ServoTestUtils.panic()`. Solo con el worker debug y `GALAPAGOS_TEST_SERVO_HELPERS=1`, que el ejecutor pone únicamente para esta sonda (D53). Línea extra `content_crashed=N` contada en `app_trace.log` |

`layout.html` incluye desde DELTA_11 la medida `columns.w` (multicolumna: 140 con soporte, 300 sin él).

## 3. Snapshots Golden Master (comportamiento actual, incluidos los fallos)

`snapshots/*.txt`. Volver a capturar uno existente requiere `-Approve`, con la aprobación del Director.

**Lo que falta hoy, medido en el binario:**

| Área | Resultado | Causa ya identificada |
|---|---|---|
| `crypto` | **No existe** (`ReferenceError` en `getRandomValues`, `subtle` y `randomUUID`) | Worker sin la feature `webcrypto` (INFORME §4.1). Explica el error de vidoomy en nacion.com |
| Multimedia | `MediaSource=0`, `VideoDecoder=0`, `EME=0`; `canPlayType` vacío para MP4, WebM y MP3 | Servo sin MSE y worker sin `media-gstreamer` |
| Gráficos | `webgl=0`, `webgl2=0`, `webgpu=0`, `OffscreenCanvas=0` | Worker sin las features `webgl`/`webgpu` |
| Observadores | `IntersectionObserver=0`, `requestIdleCallback=0` | Preferencia de Servo desactivada (`prefs.rs:449`) |
| Almacenamiento | `indexedDB=0`, `serviceWorker=0`, `Notification=0`, `clipboard=0` | Preferencias de Servo desactivadas / feature `clipboard` |
| CSS | `:has()` sin soporte (`has.w=10`, `css.has=0`); container queries sin soporte (`container.w=10`) | Stylo en Servo 0.7 |
| **Render incoherente** | `CSS.supports("aspect-ratio")=1`, pero la caja mide **0** de alto (esperado 60) | Por investigar: anuncia el soporte pero no lo aplica |

**Lo que funciona:** flex, grid (`fr`, `gap`, `minmax`), `clamp()`, variables, propiedades lógicas, `transform`, tabla fija, `inset`, `localStorage`, cookies, `fetch`, WebSocket, Workers, WebAssembly y JavaScript moderno. User-Agent: `Mozilla/5.0 (Windows NT 10.0; Win64; x86_64; rv:153.0) Servo/0.7.0 Firefox/153.0`.

**Comportamientos accidentales tolerados (no se corrigen en el arnés):**
- `alert` devuelve `undefined`, `confirm` devuelve `false` y `prompt` devuelve `null` sin mostrarse (D44).
- `window.open`: las 3 primeras devuelven un objeto y la 4.ª y 5.ª devuelven `null`, con 2 líneas `[aux:rechazado] pila llena (4)` (D41 correcto). Ninguna `aux.html` llegó a registrar su línea antes del reposo: **por investigar** si los auxiliares en segundo plano cargan.
- ~~Las líneas emitidas tras abrir un auxiliar llevan el host `about`~~. Corregido por F-2 (D46): `popups.txt` se recapturó el 2026-10-27 con la aprobación del Director y ahora lleva `localhost`.
- Una línea larga se recorta a 512 B de mensaje (538 caracteres con el prefijo); los caracteres de control pasan a espacio; el mensaje vacío se registra como línea vacía.
- La inundación se corta en 50 líneas por documento; sin cambio de documento, el resumen de omitidos no se escribe.

## 4. Comando de ejecución

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tests\characterization\compat\Run-CompatHarness.ps1
```

- **Salida:** solo fallos, como `FALLO snapshots\<sonda>.txt:<línea> - esperado <x> - recibido <y>`, y luego `fallos=N`. El código de salida es N.
- **Duración:** unos 150 s.
- **`-Probes caps`:** ejecuta una sola sonda.
- **`-Mode Sites`:** métricas de sitios reales en `runs/sites_*.csv`; no se comparan porque el contenido externo cambia.

**Sitios reales, 2026-10-27 (1 carga cada uno; nivel de métricas):**

| Sitio | Reposo | CPU | Confirmación máx. | Errores de consola | Fallos del worker |
|---|---|---|---|---|---|
| google.com | 1,9 s | 0,8 s | 236 MB | 0 | 0 |
| es.wikipedia.org (Islas Galápagos) | 3,9 s | 3,1 s | 361 MB | 0 | 0 |
| servo.org | 1,5 s | 0,8 s | 255 MB | 1 | 0 |
| youtube.com | 6,9 s | 5,0 s | 425 MB | 0 | 0 |

Respecto a la línea base de PERF-03 (Wikipedia: 16,5 s y 19,8 s de CPU; Google: 2,9 s), la mejora de DELTA_09 es clara. La cifra formal (mediana de 3) sigue siendo el paso 4 de DELTA_09.
