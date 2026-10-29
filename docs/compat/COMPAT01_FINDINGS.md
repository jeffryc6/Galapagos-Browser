# COMPAT-01 — Hallazgos medidos (DELTA_10)

Fuente: `%LOCALAPPDATA%\GalapagosBrowser\logs\worker_stderr.log` (líneas `[console:error]`, `[control:sin-soporte]`, `[aux:rechazado]`, `[content:crash]`) y `packaging/measure_load.ps1`. Solo datos reales; cada fila dice cuántas cargas cubre.

## 1. Verificación de la cadena (2026-10-26, binarios de ese día)

Página de sonda servida en `http://127.0.0.1:8765/` (no forma parte del producto): llama a `console.log`, `console.warn`, `console.error`, `alert()`, `window.open("/popup.html")` y a una función inexistente.

| Comprobación | Resultado |
|---|---|
| `console.log` / `console.warn` descartados (D42) | Sí: ninguna línea |
| `console.error` registrado con host sin ruta | `[console:error] 127.0.0.1 delta10-probe console.error` |
| Error no capturado | `[console:error] 127.0.0.1 Error at http://127.0.0.1:8765/index.html:12:28 undefinedFunctionDelta10 is not defined` |
| `alert()` sin soporte (D44) | `[control:sin-soporte] 127.0.0.1 SimpleDialog(Alert)` |
| `window.open` crea el WebView auxiliar (D41) | Sí: el script del popup se ejecutó (`delta10-probe popup activo`) |
| Fallos del worker | Ninguno en `app_trace.log` |

**Hallazgo de privacidad:** el host de la línea no lleva ruta, pero el **texto** de un error no capturado lo compone Servo e incluye la URL completa del script (ruta y, si la hubiera, consulta). Pendiente de decisión del Director (§3).

## 2. Sitios reales (1 carga en frío, sin interacción)

| Sitio | Título | Reposo | CPU del worker | Confirmación máx. | `[console:error]` | `[control:sin-soporte]` |
|---|---|---|---|---|---|---|
| www.youtube.com | 0,5 s | 6,6 s | 5,1 s | 434 MB | 0 | 0 |

Con una sola carga sin interacción, YouTube no emitió errores de consola. Para priorizar `UX-08`, `COMPAT-02` y `COMPAT-03` hace falta la prueba con ventana del Director (abrir un vídeo, buscar, menús), que completa esta tabla.

## 2b. Prueba con ventana del Director (2026-10-26)

Sesión: Google (web, imágenes, noticias) → nacion.com, más YouTube con búsqueda. Resultado: 34 `[console:error]` y ningún `[control:sin-soporte]`, `[aux:rechazado]` ni `[content:crash]`. YouTube: 0 errores. Análisis completo en [INFORME_AVANCES_2026-10-26.md](../reports/INFORME_AVANCES_2026-10-26.md) §4.

| Grupo | Líneas |
|---|---|
| Scripts `xjs` de Google (mensaje perdido por el tope de 512 B) | 18 |
| `IntersectionObserver is not defined` | 6 |
| `SyntaxError` en selectores con `:has()` | 3 |
| `SecurityError: The operation is insecure` | 2 |
| `Script error.` | 2 |
| GTM, 404 de `react.js`, `crypto.getRandomValues` | 3 |

## 2c. Prueba con ventana del Director (2026-10-27)

Google, Wikipedia, servo.org y YouTube «cargaron de primer nivel sin errores». Excepciones: los vídeos de YouTube y sus **miniaturas** no cargan. Hipótesis sin verificar: las miniaturas suelen ser WebP o AVIF cargadas en diferido con `IntersectionObserver`, que el arnés confirma desactivado (`IntersectionObserver=0`).

## 3. Decisiones abiertas

| Id | Pregunta |
|---|---|
| F-1 | **HECHO el 2026-10-27 (DELTA_10 D45).** Comprobado en el binario real: `…/search?q=dato+secreto…` → `…/search?…` y `e.js?token=…:1:1` → `e.js?…:1:1`; cero apariciones de «secreto» en el log. El arnés COMPAT sigue sin fallos. **Límite:** las URL de Google sin consulta pero con rutas de más de 450 B (`/xjs/_/js/k=…/am=AAAA…`) siguen agotando el tope de 512 B; recortar rutas largas queda como propuesta aparte. Pregunta original: ¿recortar la consulta y el fragmento de las URL que aparezcan **dentro** del texto de consola (no solo del host)? **Confirmado con datos:** el log guardó una búsqueda del usuario (`q=…`), y las URL largas de Google agotan el tope de 512 B y se pierde el mensaje |
| F-2 | **HECHO el 2026-10-27 (DELTA_10 D46).**<br>• El diagnóstico es por WebView: el host sale del WebView emisor y el contador se vacía con `LoadStatus::Started`.<br>• Se cierra de paso un bypass: antes `pushState` o un `#` reiniciaban el tope de 50 líneas.<br>• Worker 32/32.<br>• Arnés: `caps`, `layout` y `errors` sin cambios; `popups` cambia las 2 líneas previstas (`about` → `localhost`). Recapturado con la aprobación del Director (2026-10-27); arnés completo sin fallos.<br>**Límite:** los mensajes tardíos del documento anterior en el mismo WebView salen con el host nuevo (Servo no identifica al emisor). |
