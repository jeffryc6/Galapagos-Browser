# Informe de avances — 2026-10-26

**Alcance:** lo hecho en la jornada del 2026-10-26, los datos medidos en la prueba con ventana del Director y la recomendación para la siguiente fase («llevar Servo a otro nivel»). Solo datos reales: cada cifra indica su fuente.

## 1. Resumen

| Área | Estado | Fuente |
|---|---|---|
| Interfaz nativa (DELTA_08, pasos 1–5) | Hecha. Atrás, Adelante y Recargar funcionan con el historial real | Prueba con ventana del Director: «la navegación ya es fluida y estable» |
| Worker en debug (DELTA_09, pasos 1–3) | Dependencias optimizadas: worker de 210 → 129 MB | Compilación limpia de 58,7 min sin el error de 4 GB |
| Compatibilidad (DELTA_10, pasos 1–2) | `window.open`/`target="_blank"`, consola por excepción, caídas y controles sin soporte | Sonda en localhost y prueba con ventana |
| Incidentes cerrados | Consola del worker (0x800700E8) y DPI (páginas borrosas) | Hotfixes con test de regresión |
| Suites | Coordinador en verde (120 en la biblioteca); worker 29/29; IPC 4/4 | `cargo test`, 2026-10-26 |

## 2. Evolución de la carga de páginas

| Momento | Google (reposo) | Wikipedia (reposo) | YouTube (reposo) | Fuente |
|---|---|---|---|---|
| Antes del hotfix de consola | no cargaba | no cargaba | — | Error 0x800700E8 que aportó el Director |
| Tras el hotfix de DPI (worker sin optimizar) | 2,9 s | 16,5 s | — | `BASELINE_PERF03_DEBUG.md`, mediana de 3 |
| Tras DELTA_09 D36–D39 (worker optimizado) | sin medir con el script | sin medir con el script | 6,6 s (1 carga) | `measure_load.ps1`; percepción del Director: «mejoró mucho» |

La medición formal antes/después del DELTA_09 (paso 4: 3 pasadas por página) sigue pendiente.

## 3. Prueba con ventana del Director (2026-10-26)

**Funciona:**
- Ir y volver entre búsquedas web, imágenes y noticias de Google.
- Atrás y Adelante habilitados según el historial (captura: Atrás activo, Adelante gris).
- Navegación estable, sin caídas del worker: ningún `WORKER_FAULT` ni violación de canal nuevos en `app_trace.log`.
- YouTube abre su portada y la búsqueda.

**No funciona o está incompleto:**
- Los vídeos de YouTube no se reproducen (§4.1).
- Google no muestra todos los resultados ni todo el contenido de las páginas (§4.2).
- La insignia dice «Ocean Defense: permitido» en navegaciones que inicia la página (pendiente T-04b, ya conocido).

**Nota:** el aviso de YouTube «Try searching to get started» es su comportamiento normal sin cuenta y sin historial (el worker no guarda cookies, PERS-05). No es un fallo de dibujo.

## 4. Diagnóstico con datos (`worker_stderr.log` + código de Servo)

### 4.1 Vídeos de YouTube: causa determinista, verificada en el código

| Causa | Evidencia | ¿Lo resolvemos nosotros? |
|---|---|---|
| **Servo 0.7 no implementa Media Source Extensions** | No existe `MediaSource.webidl` en `vendor/servo/components/script_bindings/webidls/` | No, sin una implementación propia grande. El reproductor de YouTube depende de MSE |
| **Nuestro worker se compila sin backend multimedia** | `crates/galapagos-content/Cargo.toml`: `features = ["bundled", "multiprocess", "js_jit", "no-wgl"]`; Servo necesita `media-gstreamer` (`components/servo/Cargo.toml:60`) | Sí, pero ni un `<video>` MP4 simple se reproduce hoy. Requiere GStreamer en Windows: es una dependencia nueva con su propio delta |
| **Tampoco se activan las «web features» por defecto de Servo** | `default_web_features = ["brotli-compression-stream", "clipboard", "webcrypto", "webgpu", "webgl", "webxr"]` (`Cargo.toml:26`) y no figuran en nuestras features | Sí: es una decisión de configuración que hay que medir |

YouTube no registró ningún error de consola en dos cargas: falla en silencio porque detecta que no hay MSE.

### 4.2 Contenido incompleto: 34 errores reales en una sesión (noticias de Google → nacion.com)

| Error (agrupado) | Líneas | Causa en Servo | Verificado |
|---|---|---|---|
| `IntersectionObserver is not defined` | 6 | Preferencia desactivada por omisión: `dom_intersection_observer_enabled: false` (`prefs.rs:449`). Muchos sitios cargan con ella las imágenes y los bloques diferidos | Sí, en el código |
| Errores de los scripts `xjs` de Google | 18 | **Mensaje perdido**: la URL del script (más de 450 B) agota el tope de 512 B de D42 | Sí: hallazgo sobre nuestra propia telemetría |
| `SecurityError: The operation is insecure` (Google) | 2 | Sin determinar (probablemente almacenamiento o cookies de un iframe) | No |
| `SyntaxError` en selectores con `:has()` | 3 | Sin determinar: una de las tres cadenas trae un `]` desequilibrado del propio sitio | No |
| `crypto.getRandomValues() not supported` | 1 | `getRandomValues` existe en `Crypto.webidl`; posible relación con la falta de la feature `webcrypto` | No |
| `Script error.`, GTM, 404 de `react.js` | 4 | Del sitio o de origen cruzado | — |

**Hipótesis por medir:** activar `IntersectionObserver` puede explicar buena parte de «no muestra todos los resultados». Es un cambio de preferencia que se mide con el arnés (§6), sin tocar el código de Servo.

### 4.3 Hallazgos sobre nuestra propia telemetría (DELTA_10)

| Id | Hallazgo | Propuesta (pendiente de aprobar) |
|---|---|---|
| F-1 | **Privacidad:** la línea 8 del log contiene la búsqueda del usuario (`q=lista+loter%C3%ADa+nacional`) dentro del texto de error que compone Servo | Recortar la consulta y el fragmento de toda URL dentro del texto (resuelve también la pérdida de mensajes de §4.2) |
| F-2 | **Atribución:** errores de scripts de google.com quedaron etiquetados como `www.nacion.com`; el host se toma del documento activo al registrar el mensaje, no del que lo emitió | Tomar el host del WebView que emite (`show_console_message` lo recibe) y vaciar el contador al empezar la carga, no al cambiar la URL |

## 5. Pendientes abiertos

| Id | Pendiente | Prioridad |
|---|---|---|
| F-1, F-2 | Correcciones de la telemetría de consola | Alta (F-1 es de privacidad) |
| DELTA_09 pasos 4–7 | Medición con 1 hilo, D40 (3 hilos con puerta) e informe antes/después | Media |
| T-04b | Navegaciones iniciadas por la página sin pasar por Ocean Defense; la insignia miente | Alta |
| UX-08 | `<select>` y `alert`/`confirm`/`prompt` no se muestran | Media |
| COMPAT-03 | Preferencias de Servo desactivadas (`IntersectionObserver`, Service Workers, IndexedDB, WebGL2) | Alta: es lo más barato y medible |
| PERS-05 | Cookies persistentes (sesiones de YouTube y Google) | Decisión de privacidad |
| RENDER-01 | Iconos como cuadrados negros | Media |
| CLI-01 | La URL por línea de comandos no carga | Baja |

## 6. Recomendación: cómo «llevar Servo a otro nivel»

**Recomiendo arnés primero, luego forense, después delta. El upgrade, al final.** No conviene empezar modificando el código de Servo.

| Opción | Para qué sirve aquí | Cuándo |
|---|---|---|
| **1. `/znve-harness` — Golden Master de compatibilidad** | Una batería fija de páginas de sonda locales (APIs presentes, `IntersectionObserver`, `:has()`, `<video>`, `crypto`, formularios, `window.open`) más una lista fija de sitios reales. Captura la salida actual del binario intacto: errores de consola, tiempos y capacidades detectadas. Sin esta línea base, cualquier cambio en Servo es a ciegas y no se puede demostrar que mejora algo | **Primero** |
| **2. `/znve-forensic` de la superficie de configuración de Servo** | Solo lectura sobre `components/config/prefs.rs`, las features de `components/servo/Cargo.toml` y `components/servo` (API del embebedor). Inventario de palancas: preferencias desactivadas, features que no compilamos y métodos del delegado sin usar. Los datos de §4 indican que el mayor margen está en la **configuración**, no en el código | Segundo |
| **3. `/znve-contract --delta` (COMPAT-03/04)** | Activar preferencias y features una a una, cada una medida contra el arnés y con la puerta de cero `WORKER_FAULT` | Tercero |
| 4. Parches propios en `vendor/servo` | Solo donde el forense y el arnés demuestren un hueco que la configuración no cubre. Cada parche, registrado en `SERVO_VENDOR.md` con su `.patch` | Después |
| 5. `/znve-upgrade` (Servo 0.7 → más reciente) | Servo publica mejoras cada mes, y una versión nueva puede traer gratis lo que nos falta. Con el arnés, el upgrade se valida con datos, y el adaptador del motor (`WebEnginePort`) ya aísla el resto del navegador | Cuando el arnés esté en verde |

**Por qué no parches en Servo ya:**
- MSE (lo que necesitan los vídeos) es un subsistema completo de Servo y no está empezado. Implementarlo nosotros es un proyecto en sí.
- Los fallos medidos de Google y de las noticias apuntan a una preferencia desactivada y a features sin compilar, que se arreglan sin parchear Servo.
- Sin arnés, no hay forma de comprobar que un cambio en Servo no rompe otra cosa (Guardrail 9).

## 7. Estado del proyecto para la copia manual

- No queda ningún proceso de compilación, del navegador ni del servidor de sonda en marcha.
- Binarios actuales:
  - `target/debug/galapagos-browser.exe` (Coordinador);
  - `crates/galapagos-content/target/debug/galapagos-content.exe` (worker, 129 MB, 2026-10-26 16:54).
- Hay un worker antiguo, de 210 MB y del 2026-10-23, en `target/debug/galapagos-content.exe`. El Coordinador lanza el más reciente (L-I7).
- Carpetas `target/`: unos 25,5 GB en el worker más los del Coordinador. No hacen falta en la copia si se puede recompilar, aunque la compilación limpia del worker tarda unos 59 min.
- Contratos al día: `contracts/RETOMAR.md`, `DELTA_08`, `DELTA_09`, `DELTA_10`, `CONTRACT_BACKLOG.md`. Datos: `docs/compat/COMPAT01_FINDINGS.md`, `docs/performance/`.
