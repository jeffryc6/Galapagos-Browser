# CONTRACT BACKLOG - Galapagos Browser

Filtro de diferimiento de capacidades P1 y P2 fuera del contrato v1 (Fase P0 / Etapas 0-2), conforme a la Anti-Bloat Fence de `contract.md`.

## Capacidades Diferidas

| Código | Prioridad | Descripción | Justificación de Diferimiento |
|---|---|---|---|
| `PRIV-02` | P1 | Eliminación verificable avanzada de artefactos de memoria virtual/volcados en sesiones efímeras. | Requiere APIs de Windows de gestión de heap seguro (`VirtualLock`, `SecureZeroMemory`) tras validar el ciclo de vida base. |
| `PERF-02` | P1 | Suspensión y reanudación automática de pestañas inactivas. | Requiere telemetría de uso y señalización IPC estable con el motor web antes de aplicar descarte de páginas. |
| `DEV-01` | P1 | Centro de diagnóstico propio (`Network Lab`, `Performance Lab`, `Security Lab`, `Storage Lab`). | Herramientas de inspección interna a desarrollar sobre las interfaces de telemetría P0. |
| `DEV-02` | P2 | Integración progresiva con Firefox DevTools via CDP o WebSocket. | Dependiente del soporte de servidor devtools en la capa FFI de `libservo`. |
| `NET-01` | P1 | Enrutador de proxies activos e integración opcional futura con Tor (`Connectivity Island`). | Fase P0 implementa política `DirectSystem` y `CustomProxy` básico; enrutamiento dinámico multicircuito diferido. |
| `EXT-01` | P1 | Definición de permisos y manifiesto de extensiones. | Superficie de seguridad secundaria sujeta a la estabilidad del runtime principal. |
| `EXT-02` | P2 | Sistema de extensiones instalables. | Requiere aislamiento IPC específico para procesos de extensión. |
| `PLAT-01` | P2 | Expansión multiplataforma a Linux (Wayland/X11 + Landlock/seccomp). | El objetivo P0 está enfocado en Windows 10/11 64-bit nativo. |
| `ENG-02` | P1 | Transporte de frames entre procesos (textura compartida DXGI o SetParent cross-process). | Hito 1 de Δ1 solo cubre arranque confinado + handshake (DELTA_01). |
| `ENG-03` | P2 | Site isolation: un worker por origen. | Requiere ciclo de vida de worker estable (DELTA_01). |
| `ENG-04` | P1 | GPU en AppContainer, con fallback a Low Integrity Level. | Pruebas de capacidades mínimas tras tener worker real. |
| `ENG-05` | P1 | Nivel de seguridad "Strict": JIT desactivado + ACG (`ProhibitDynamicCode`). | ACG es incompatible con el JIT de SpiderMonkey (decisión D1 de DELTA_01). |
| `UX-01` | P1 | Atrás/Adelante/Recargar/Detener y estado de carga. | Contratado en DELTA_08 §2.2 (paso 5). |
| `UX-02` | P1 | Barra de pestañas y multi-pestaña real. | Requiere ciclo de navegación estable (DELTA_02). |
| `UX-03` | P2 | IME y composición (`WM_IME_*`). | DELTA_02 cubre solo `WM_CHAR`. |
| `UX-04` | P2 | Cambio de cursor según el elemento bajo el puntero. | Reenvío de MouseMove/MouseUp/rueda promovido a DELTA_02 (D8). |
| `ENG-06` | P2 | Búsqueda desde la barra y autocompletado. | Implica red y política de privacidad. |
| `ENG-07` | P2 | I/O solapada en el worker en lugar de sondeo de 16 ms. | DELTA_02 mitiga con vaciado del pipe y tope de payload. |
| `UX-05` | P1 | Redimensionar la superficie del worker al cambiar el tamaño de la ventana (`WM_SIZE` no llama a `resize_viewport`). | Detectado en DELTA_02 paso 7; fuera de su perímetro. |
| `ENG-08` | P2 | Consolidar el Job Object en un único dueño (Coordinator y ServoEngineAdapter crean `Galapagos_TabJob_<tab>`). | DELTA_04; toca la Security Island y R8. |
| `ENG-09` | P1 | Arrancar el worker sin bloquear el hilo de UI (~6 s en `ConnectNamedPipe` + handshake). | DELTA_04; requiere hilo o espera asíncrona. |
| `ENG-10` | P2 | Navegar con el worker terminado devuelve `MalformedMessage`; debería indicar canal caído. | DELTA_05 (E-1); fuera del perímetro de la corrección de GL. |
| `ENG-11` | P1 | Verificar la entrada hacia la página de extremo a extremo (foco del cuadro de texto, Enter en formularios, doble KeyDown por tecla). | DELTA_06; la sonda no vio navegación tras Enter en la caja de búsqueda de google.com. |
| `PERF-03` | P1 | Optimizar Servo en el perfil `dev` del worker y volver a 3 hilos de estilo. | **DELTA_09 (CONGELADO; pasos 1–3 hechos)**; línea base en `docs/performance/BASELINE_PERF03_DEBUG.md`. |
| `PERF-04` | P2 | Perfil de distribución (`release` o `production` con LTO) para el MSIX, con su medición. | DELTA_09 §8. |
| `BUILD-01` | P2 | Meter `galapagos-content` en el workspace raíz (un `target/` y un `Cargo.lock`). | DELTA_09 §8; P-09. |
| `BUILD-02` | P3 | Limpiar `target/release` del worker (3,5 GB de un intento fallido). | DELTA_09 §8; con autorización del Director. |
| `RENDER-01` | P2 | Iconos de Wikipedia como cuadrados negros (máscaras o SVG en Servo). | DELTA_09 §8; visto en INC-20261026-WORKER-DPI. |
| `CLI-01` | P2 | `galapagos-browser.exe <url>` no carga la URL. | DELTA_09 §8; comprobado tras INC-20261026-WORKER-CONSOLE. |
| `SEC-04` | — | **CERRADO.** `data:` ya se deniega en `is_navigation_allowed` (`delegates.rs:34`), con test. | Comprobado en la auditoría de DELTA_10 §0. |
| `UX-08` | P1 | Presentar los controles del embebedor desde el Coordinador (`<select>`, selector de archivos, `alert`/`confirm`/`prompt`). | DELTA_10 §7; priorizado con los datos de D44; IPC en los dos sentidos. |
| `PERS-05` | P2 | Cookies, almacenamiento web y caché HTTP en disco solo para `Personal`. | DELTA_10 §7; decisión de privacidad por compartimento (DELTA_07). |
| `COMPAT-02` | P2 | Experimento de User-Agent, medido. | DELTA_10 §7; toca el test `worker_preferences_*` (Guardrail 9). |
| `COMPAT-03` | P2 | Experimento con Service Workers e IndexedDB (prefs de Servo desactivadas por omisión). | DELTA_10 §7; con la sonda y la puerta de `WORKER_FAULT`. |
| `DEV-03` | P2 | Sonda de capacidades (`evaluate_javascript` sobre `navigator.*`). | DELTA_10 §7; parte de `DEV-01`. |
| `COMPAT-04` | P2 | `:has()` y subgrid: preferencias de Stylo fuera de `servo::Preferences`. | DELTA_11 §8; necesita `stylo_static_prefs` directo o un parche. |
| `COMPAT-05` | P2 | Resto de preferencias experimentales de Servo, una a una con el arnés. | DELTA_11 §8. |
| `GFX-01` | P2 | WebGL/WebGL2 con análisis de superficie de ataque. | DELTA_11 D52. |
| `ENG-12` | P2 | Registrar `ServoDelegate` (errores internos y consola de los workers). | FORENSIC_SERVO_CONFIG Z-3. |
| `NET-04` | P1 | Proxy fijado por la política del Coordinador, no por variables de entorno. | FORENSIC_SERVO_CONFIG Z-2. |
| `LOG-01` | P2 | Quitar `RUST_LOG` del entorno del worker. | DELTA_11 §8. |
| `NET-03` | P2 | Descargas interceptadas con `load_web_resource` y guardadas por el Coordinador. | DELTA_10 §7; Servo 0.7 no tiene delegado de descargas. |
| `PERS-01` | P1 | Esquema v2: persistir `NetworkRoutePolicy` de los perfiles `Personal` y protocolo de migraciones (`user_version` 1→2 en `BEGIN EXCLUSIVE`). | DELTA_07 (D26): hasta entonces `CustomProxy` en `Personal` se rechaza. |
| `PERS-02` | P1 | Instancia única del Coordinador (mutex con nombre) para que dos procesos no compartan `personal.db`. | DELTA_07 (W-11). |
| `PERS-03` | P2 | `verified_artifacts` con upsert al reaplicar un artefacto. | DELTA_07 (W-10); requiere el verificador Ed25519 real (SEC-03 en fail-closed). |
| `UX-06` | P2 | Favoritos. | DELTA_08; requiere esquema v2 (PERS-01). |
| `UX-07` | P2 | Barra de título personalizada con el logo integrado (`DwmExtendFrameIntoClientArea`). | DELTA_08 (D30): v0.1 usa la barra del sistema en modo oscuro. |
| `NET-02` | P1 | Filtro de rastreadores real (p. ej. `adblock-rust`) sobre el tráfico del worker. | DELTA_08; requiere que la red pase por el Coordinador (T-04b). |
| `SEC-05` | P1 | Verificador Ed25519 real para SEC-03 (p. ej. `ed25519-dalek` + `sha2`). | DELTA_08 / T-01: hoy fail-closed; delta propio con su Anti-Bloat Fence. |
| `CI-01` | P2 | `cargo-audit`, `cargo-deny` y `cargo-geiger` en la compilación. | DELTA_08; herramientas de CI, no de runtime. |
| `PERS-04` | P2 | `StorageEngine` sin `lock().unwrap()`: un `Mutex` envenenado debe dar `StorageFailure`, no un pánico. | DELTA_07; fuera del perímetro de la persistencia WAL. |
