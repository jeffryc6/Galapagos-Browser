# Línea base PERF-03: worker en debug (2026-10-26)

Medida antes de cambiar los perfiles de compilación, para compararla después con el worker optimizado. Todos los valores son medidas reales sobre el binario de producción en debug; no hay datos simulados.

## Condiciones
- **Coordinador:** `target/debug/galapagos-browser.exe`, con el hotfix INC-20261026-WORKER-CONSOLE (`CREATE_NO_WINDOW`).
- **Worker:** `crates/galapagos-content/target/debug/galapagos-content.exe`, de 210 MB:
  - `opt-level = 0`;
  - `-C codegen-units=1` en `.cargo/config.toml`;
  - `LAYOUT_THREADS = 1`.
- **Cuota del Job:** 2 GiB de memoria y CPU sin tope.
- **Equipo:** 8 procesadores lógicos, 32 GB de RAM, Windows 11 23H2 (22631), monitor a 144 DPI y red doméstica.

## Método
Script externo (sin cambios en el código), 3 pasadas por página, cada una con la app arrancada en frío:
1. Arranca la app, que muestra `galapagos://dashboard`, y espera a que el worker quede en reposo.
2. Escribe la URL en la barra (`WM_SETTEXT`) y pulsa Enter (`WM_KEYDOWN VK_RETURN`), igual que el usuario.

Datos que toma:
- **Arranque:** desde que se lanza la app hasta que el worker queda en reposo.
- **Título:** desde Enter hasta que la ventana muestra el título de la página (llega `TitleChanged`).
- **Reposo:** desde Enter hasta el inicio de una ventana de 2 s en la que el worker gasta menos de 0,10 s de CPU. Es la mejor aproximación externa a «carga terminada»; el Coordinador no recibe ninguna señal de fin de carga.
- **CPU:** tiempo de CPU del worker entre Enter y el reposo.
- **Confirmación y working set:** picos del worker durante la carga.

## Resultados (mediana de 3 y rango)

| Página | Título | Reposo | CPU del worker | Confirmación máx. | Working set máx. |
|---|---|---|---|---|---|
| example.com | 0,2 s | 0,7 s (0,6–0,7) | 0,2 s | 141 MB | 162 MB |
| www.google.com | 0,6 s (0,5–0,9) | 3,0 s (2,8–3,2) | 1,9 s | 224 MB | 245 MB |
| servo.org | 0,3 s (0,3–0,6) | 1,9 s (1,6–2,2) | 2,5 s | 242 MB | 275 MB |
| es.wikipedia.org (Islas Galápagos) | 0,5 s | 18,5 s (18,2–18,5) | 18,9 s | 314 MB | 321 MB |

- **Arranque:** 1,0–1,1 s en las 12 pasadas.
- **Worker en reposo antes de navegar:** 52 MB de confirmación y 76 MB de working set.
- **Datos brutos:** `BASELINE_PERF03_DEBUG.csv`, junto a este archivo.

### Tras el hotfix INC-20261026-WORKER-DPI (misma tarde)
La tabla anterior se midió con el worker sin declarar DPI: Servo dibujaba a ⅔ de la resolución a 144 DPI y la imagen se estiraba. Con el worker en PerMonitorV2 y `hidpi_scale_factor = 1,5`, dibuja a resolución real:

| Página | Título | Reposo | CPU del worker | Confirmación máx. |
|---|---|---|---|---|
| www.google.com | 0,6 s | 2,9 s (2,8–2,9) | 1,7 s | 236 MB |
| es.wikipedia.org (Islas Galápagos) | 0,6 s | 16,5 s (16,4–16,5) | 19,8 s | 353 MB |

Datos brutos en `BASELINE_PERF03_DEBUG_DPI.csv`. **Esta es la línea base de referencia para PERF-03.**

## Lectura
- **El cuello de botella es la CPU:**
  - en Wikipedia, la CPU (18,9 s) iguala al tiempo hasta el reposo (18,5 s), es decir, un núcleo saturado;
  - es lo esperable con Servo sin optimizar y un solo hilo de estilo (`LAYOUT_THREADS = 1`);
  - la red no es el límite: el título llega en ≤ 0,9 s en todas las páginas.
- **En páginas ligeras** (example.com) la carga ya es casi inmediata; la diferencia se nota en páginas con mucho CSS y DOM.
- **Para la cuota del worker:** el pico medido es de 320 MB en una página pesada, muy por debajo de los 2 GiB de prueba. Es un primer dato para decidir los valores de Estándar y Hardened (`RESOURCE_BUDGETS.md`).

## Cómo repetir la medición tras PERF-03
Con `packaging/measure_load.ps1` (parámetros `-Exe`, `-Urls`, `-Runs`, `-TimeoutS` y `-OutCsv`): mismas páginas, mismo método y 3 pasadas. Se compara la mediana de «Reposo» y «CPU del worker», y se confirma que no reaparece el desbordamiento de pila de `StyleThread` al volver a `LAYOUT_THREADS = 3`.
