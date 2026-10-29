# TRIAGE · INC-20261025-CORE-DEBLOAT

**Comando:** `/znve-triage` (solo lectura) · **Fecha:** 2026-10-25 · **Referencia:** `contract.md` (P0: ARC-01, ARC-02, SEC-01, SEC-02, SEC-03, PRIV-01, PERF-01) y `contracts/CONTRACT_BACKLOG.md`
**Perímetro:** `src/`, `crates/galapagos-content/src/`, `contracts/*.rs`, `tests/`, manifiestos y raíz. Excluido `vendor/servo`.
**Método:** análisis estático. No se compiló ni se ejecutaron tests: ningún resultado de ejecución se declara aquí.

Volumen analizado (sin bloques `#[cfg(test)]`): coordinador 7 726 LOC, worker 1 072 LOC, contratos .rs 706 LOC.

---

## 1. COMPONENTE AFECTADO

| Capa | Archivos |
|---|---|
| Frontera de seguridad (SEC-01/SEC-03) | `src/defense/threat_intel.rs`, `src/ipc/channel.rs`, `src/engine/mod.rs`, `crates/galapagos-content/src/main.rs` |
| Política de navegación (Ocean Defense) | `src/defense/request_guard.rs`, `src/coordinator.rs`, `src/main.rs`, `crates/galapagos-content/src/delegates.rs` |
| Código muerto o diferido | `src/engine/servo_ffi.rs`, `src/engine/input_dispatcher.rs`, `src/islands/*`, `src/defense/{payload_guard,behavior_sentinel}.rs`, `contracts/galapagos_core_v1.rs` |
| Ruta caliente (UI/IPC) | `src/ipc/{channel,framer,dispatcher}.rs`, `src/window/mod.rs`, `src/diagnostics/mod.rs` |

---

## 2. CAUSA RAÍZ DETERMINISTA

**Diagnóstico:** el código se escribió para todas las islas de la arquitectura completa, sin respetar el filtro de diferimiento P0. El resultado es código que compila y pasa tests, pero no está conectado a ninguna ruta de ejecución real. Además, varias garantías P0 se declaran sin aplicarse. Los tests las validan contra stubs o datos forjados, así que quedan en verde.

### 2.1 Seguridad: la garantía se declara pero no se aplica (crítico)

| ID | Ubicación | Defecto | Contrato |
|---|---|---|---|
| **T-01** | `threat_intel.rs:340` | `acc != 0 \|\| true`: la verificación Ed25519 **siempre devuelve `true`**. Cualquier feed con un SHA-256 correcto se acepta. | SEC-03 |
| T-01b | `threat_intel.rs:21` | `GALAPAGOS_MASTER_PUBKEY` es la clave pública del vector de prueba 1 de RFC 8032, cuya clave privada es pública. | SEC-03 |
| T-01c | `threat_intel.rs:149-156` | `let _ = insert_verified_artifact(…, 1700000000)`: la auditoría se suprime en silencio y la marca de tiempo está fija en el código. | SEC-03 · Guardrail 6 |
| **T-02** | `galapagos-content/src/main.rs:365,442` | El worker anuncia `sandbox=2` (AppContainerStrict) en el handshake sin aplicar nada. `engine/mod.rs:596` lo lanza con `CreateProcessW` sin AppContainer, sin token restringido y sin mitigaciones. `ProcessMitigationEngine::apply_content_worker_mitigations` no se llama nunca. La comprobación H2 (`engine/mod.rs:726`) confía en lo que dice el propio proceso hostil. | SEC-01 · ARC-01 |
| T-02b | `window/mod.rs:604,612` | El dashboard muestra fijos en el código «AppContainerStrict», «Proceso Confinado» y «WebRTC Leak Guard: BlockAllP2P». El usuario ve un estado que no es el real. | SEC-01 · NET-01 |
| **T-03** | `ipc/channel.rs:107` | `SetSecurityDescriptorDacl(…, 1, NULL, 0)` crea una **DACL nula**: cualquier proceso local tiene acceso total. El nombre es predecible (`galapagos-ipc-tab-1`) y falta `FILE_FLAG_FIRST_PIPE_INSTANCE`, lo que permite *pipe squatting*. El worker no verifica el PID del servidor (`GetNamedPipeServerProcessId`). | ARC-02 · SEC-01 |
| **T-04** | `request_guard.rs:109-110` | El host se extrae cortando solo por `/` y `:`. Estas formas evitan la blocklist: `https://phishing.evil.com?x`, `…#x`, `phishing.evil.com.` y los subdominios. | SEC-02 |
| T-04b | `galapagos-content/src/delegates.rs:79-87` | `request_navigation` permite http, https y `data:` en el worker. Las navegaciones que inicia el contenido (enlaces, 30x, `location=`) **nunca pasan por el Coordinador**. Ocean Defense y Behavior Sentinel solo cubren la barra de direcciones. | ARC-02 · SEC-02 |
| T-04c | `coordinator.rs:358-380` | `record_navigation_hop` se ejecuta y, si la navegación tiene éxito, `reset_navigation_hops` la reinicia de inmediato. La detección de bucles de redirección es inalcanzable en el flujo real; solo se dispara en los tests, que inyectan saltos a mano. | SEC-02 |
| **T-05** | `engine/mod.rs:71-93` | El binario del worker se busca en rutas relativas al CWD (`galapagos-content.exe`, `target/…`) y se elige el de *mtime* más reciente. Eso permite plantar un binario desde el directorio de trabajo. | SEC-01 |
| T-06 | `islands/security/mod.rs:40` · `engine/mod.rs:626` | Hay dos Job Objects **con nombre** (`Galapagos_TabJob_<tab>`), uno en SecurityIsland y otro en el motor. Ambos abren el mismo objeto de kernel y aplican los límites dos veces. Otro proceso del mismo usuario puede abrirlo por nombre y relajar los límites. (ENG-08) | SEC-01 |

### 2.2 Redundancia y código muerto

| ID | Ubicación | Hallazgo | LOC (estimado) |
|---|---|---|---|
| R-01 | `src/main.rs:10-20` | Redeclara todos los módulos que ya están en `lib.rs`. El crate entero se compila **dos veces** (lib y bin), los tests corren dos veces y `#[allow(dead_code)]` oculta los avisos. | — (½ del tiempo de compilación) |
| R-02 | `engine/servo_ffi.rs` (todo) | Servo **simulado** con `#[no_mangle]` exportado. Entra en producción por `default = ["engine-stub"]` (`Cargo.toml:22`). Viola el Guardrail 9. | 352 |
| R-03 | `engine/input_dispatcher.rs:133-233, 235-417` | `to_servo_native_event`, `flush_to_embedder`, `enqueue_win32_message`, `drain_events` y una cola con `Mutex` que solo se llena desde `resize_viewport` y nunca se vacía. Solo se usan `translate_win32_message` y `KeyModifiers`. | ≈300 |
| R-04 | `islands/performance` (`ThrottlingManager`, `CpuExceeded`) | Implementa PERF-02, que está **diferido** en el backlog. | ≈90 |
| R-05 | `islands/research/mod.rs` | Implementa DEV-01, **diferido**. Lo único conectado es una llamada que formatea un `String` en cada navegación (`main.rs:324-329`). | 164 |
| R-06 | `islands/connectivity` (`WebRtcPolicy`, router) | Implementa NET-01, **diferido**. No se aplica a Servo. `validate_destination(url, 443)` recibe una URL completa y un puerto constante, así que repite `validate_url_scheme`. | ≈110 |
| R-07 | `islands/privacy/mod.rs` | Almacén paralelo de cookies y localStorage que Servo nunca alimenta. El «zeroize» es `String::clear()`, que no borra memoria (PRIV-02 está diferido). Abre un SQLite por perfil efímero que nunca se usa. `validate_storage_access`, rama 2, no hace nada. | ≈280 |
| R-08 | `islands/freedom` (`SitePermissionStore`) | Caché que duplica `site_permissions` de SQLite. `check_permission` no tiene consumidores. | ≈80 |
| R-09 | `defense/payload_guard.rs` | MIME, magic bytes y MOTW sin ruta de descarga en el producto. | 236 |
| R-10 | `defense/behavior_sentinel.rs` (scripts/CPU) | Detectores que nada alimenta. `record_script_injections` mete `injected_count` Instants (`u32`) en la cola: es una bomba de memoria latente. | ≈150 |
| R-11 | `defense/threat_intel.rs:178-461` | SHA-256 y SHA-512 escritos a mano, y un verificador falso (T-01). | ≈280 |
| R-12 | `coordinator.rs:120-205` | 15 *accessors* con `#[allow(dead_code)]`; `navigate()` solo envuelve otra llamada. | ≈80 |
| R-13 | `main.rs:310-321` + `coordinator.rs:365-373` | **Doble evaluación** de RequestGuard por navegación. La de `main` fija `ProfileId(1)/Daily` en el código e ignora el perfil real. | ≈25 |
| R-14 | `galapagos-content/src/main.rs:273-323` | Segundo codec IPC (framing, handshake, texto y constantes 2048/512/1 MiB) duplicado de `src/ipc`. Hay riesgo de que diverjan. | ≈60 |
| R-15 | `ipc/channel.rs:174-225 vs 301-352` | `send_frame` y `receive_frame` copiados entre el servidor y el cliente. `RestrictedNamedPipeClient` solo lo usan los tests. | ≈60 |
| R-16 | `engine/mod.rs:629-735` | 8 copias del bloque de limpieza (`terminate_and_cleanup` + `DestroyWindow`). La altura de 46 px está fija en el código en 3 sitios. | ≈50 |
| R-17 | `diagnostics/mod.rs:253-350` | `log_info`, `log_warn`, `log_error` y `log_critical`: el mismo cuerpo 4 veces. | ≈70 |
| R-18 | `contracts/galapagos_core_v1.rs:512-584` | Lógica de UI dentro del contrato (`GalapagosTheme`, `GALAPAGOS_DARK`, `LiveDashboardState`) que nadie usa; el dashboard pinta con `rgb()` fijo. `validate_url_scheme` solo reenvía a `validate_url_strict`. | ≈75 |

**Estimado:** entre ≈2 000 y 2 400 LOC eliminables de 7 726 (≈27-31 % del coordinador), sin pérdida funcional en runtime.

### 2.3 Rendimiento (ruta caliente)

| ID | Ubicación | Hallazgo |
|---|---|---|
| P-01 | `channel.rs:198` · `framer.rs:70` · `galapagos-content/src/main.rs:524-531` · `dispatcher.rs:401` | El worker envía `FrameReady` **en cada frame** y el Coordinador lo descarta. Cada lectura reserva y pone a cero `vec![0; 64 KiB]` y después hace `to_vec()` del payload. A 60 fps son ≈3,8 MiB/s de asignaciones y ≈120 syscalls por segundo en el hilo de UI, sin ningún efecto útil. |
| P-02 | `window/mod.rs:436-449` | Se registra en el log **cada `WM_COMMAND` de la barra**, es decir cada pulsación (EN_CHANGE/EN_UPDATE): formato, fecha ISO y escritura síncrona en disco **en el hilo de UI**. Viola los Guardrails 2 y 4. |
| P-03 | `main.rs:54-112,227,255` · `window/mod.rs:250,350,415,453` · `diagnostics/mod.rs:221,236-245` | Logs de estado sano (BOOTSTRAP, PROFILE_INIT, TAB_INIT, STORAGE_BOUND, WINDOW_INITIALIZED, MESSAGE_LOOP_*, SHUTDOWN_CLEAN) y `println!` en debug. Viola el Guardrail 2. |
| P-04 | `dispatcher.rs:452-464` | El contenido hostil puede enviar `TelemetrySample` sin límite; se inserta con `let _ =` → crecimiento de SQLite (DoS) y fallos silenciosos. |
| P-05 | `diagnostics/mod.rs:97` | La rotación de 5 MiB solo se evalúa al abrir el archivo, así que `app_trace.log` crece sin límite durante la sesión. |
| P-06 | `storage/mod.rs:209,326` | Se usa `prepare` en lugar de `prepare_cached`. Hay un `Mutex<Connection>` en un coordinador de un solo hilo. |
| P-07 | `main.rs:107` | El perfil `Personal` usa `open_in_memory()`. El contrato exige WAL en disco y `open_file` no se usa: el contrato de persistencia no se cumple. |
| P-08 | `research/mod.rs:66` | `Vec::remove(0)` es O(n) en el «buffer circular». Hoy está latente. |
| P-09 | `crates/galapagos-content` | Queda fuera del workspace: dos `target/` (27,6 GB + 2,4 GB) y dos `Cargo.lock`. `codegen-units=1` + `incremental=false` en debug sobre todo Servo alargan las compilaciones. |

### 2.4 Higiene

| ID | Hallazgo |
|---|---|
| H-01 | Hay `let _ =` silenciosos en `threat_intel.rs:149`, `dispatcher.rs:455`, `engine/mod.rs:476`, `main.rs:495`, `diagnostics/mod.rs:100-101`, `galapagos-content/src/main.rs:467` y `delegates.rs:66,72,77` (Guardrail 6). |
| H-02 | Códigos Win32 falsos fijos en el código (1400, 1450, 5) en lugar de `GetLastError`: `security/mod.rs:46,77,100,113`, `performance/mod.rs:52,72`, `engine/mod.rs:522,732`. |
| H-03 | Rastreadores de relleno en producción (`telemetry.badtracker.com`…) en `request_guard.rs:68-70`. Hay una inyección de host de prueba `--smoke-test` en el binario de release (`main.rs:116-118`). |
| H-04 | `timestamp_monotonic_ns` usa `SystemTime` (`performance/mod.rs:285`), que no es monotónico, pese a lo que dice el nombre del campo en el contrato. |
| H-05 | Imágenes de 3,7 MB en la raíz del source. No hay `.git` ni `.gitignore`, y ~30 GB de `target/` quedan sin excluir. |

---

## 3. RADIO DE IMPACTO (BLAST RADIUS)

- **SEC-01, SEC-03 y ARC-02 no se cumplen en runtime.** El worker con Servo y JIT corre a nivel de integridad medio con todos los derechos del usuario. Lo único real es el Job Object (memoria y CPU).
- **Tests en verde falsos (Guardrail 9).** Estos tests validan comportamiento falso o forjado y **no se tocan sin aprobación del Director**:
  - `threat_intel.rs` `test_threat_feed_admission_and_hot_update` y `test_anti_downgrade_barrier` (firma `[0x5a;64]` forjada).
  - `coordinator.rs` `test_threat_intel_feed_hot_update_and_audit`.
  - `servo_ffi.rs` (2 tests de un stub).
  - `tests/worker_isolation_tests.rs::test_rejection_when_applied_sandbox_lower_than_requested` (certifica la autodeclaración del worker).
- **Interfaz engañosa:** el dashboard comunica protecciones que no existen.
- **Superficie local:** cualquier proceso del usuario puede inyectar o leer tramas IPC y alterar los límites del Job.
- **Rendimiento:** churn constante en el hilo de UI (P-01, P-02) y crecimiento de logs y de la DB sin tope.
- **Fuera de impacto:** el esquema SQLite (`schema_v1.sql`), los DTOs del contrato y `vendor/servo`.

---

## 4. PLAN DE CONTENCIÓN INMEDIATA

Sin alterar los contratos de datos. Cada fila es un `/znve-hotfix --incident=INC-20261025-CORE-DEBLOAT` con un único `TARGET_FILE`.

| Orden | Sev. | Hallazgos | TARGET_FILE | Acción mínima | ¿Delta? |
|---|---|---|---|---|---|
| 1 | Crítica | T-01 | `src/defense/threat_intel.rs` | *Fail-closed*: `verify_ed25519_signature` → `false` hasta tener un verificador real; propagar el error de la auditoría. | El verificador real (CNG no ofrece Ed25519; `aws-lc-rs` ya está en el árbol del worker) necesita `--delta` |
| 2 | Crítica | T-03 | `src/ipc/channel.rs` | DACL SDDL solo para el propietario, `FILE_FLAG_FIRST_PIPE_INSTANCE`, nombre con sufijo aleatorio, búfer de lectura reutilizable (P-01). | No |
| 3 | Crítica | T-02b | `src/window/mod.rs` | Quitar del dashboard las afirmaciones falsas y el log por pulsación (P-02). | No |
| 4 | Alta | T-02 | `src/engine/mod.rs` | El nivel de sandbox lo decide el Coordinador (`TrustedCoordinator` mientras no exista AppContainer), no el handshake. Buscar el worker solo en `current_exe()\` (T-05). Usar un único Job sin nombre (T-06). Limpieza con RAII (R-16). | El AppContainer real necesita `--delta` (ENG-04) |
| 5 | Alta | T-04 | `src/defense/request_guard.rs` | Cortar el host en `/ ? # @ :`, quitar el punto final y comparar sufijos de dominio. | No |
| 6 | Alta | T-04b | `crates/galapagos-content/src/delegates.rs` | Denegar `data:` (SEC-04). | Consultar al Coordinador por cada navegación iniciada por el contenido necesita un **`--delta`** (nuevo mensaje IPC) |
| 7 | Media | R-01 | `src/main.rs` | El binario consume `galapagos_browser::*` y deja de redeclarar módulos. Un solo camino de evaluación (R-13). Quitar los logs de estado sano (P-03). | No |
| 8 | Media | R-02, R-03 | `Cargo.toml` + `src/engine/*` | Eliminar `engine-stub` y `servo_ffi.rs`, y reducir `input_dispatcher.rs` a la traducción Win32. | Requiere aprobación para retirar tests |
| 9 | Media | R-04…R-12, R-18 | `src/islands/*`, `src/defense/*`, `contracts/galapagos_core_v1.rs` | Retirar lo diferido según el backlog (PERF-02, DEV-01, NET-01, PRIV-02) y los *accessors* muertos. | `--delta` para el contrato (R-18) |
| 10 | Baja | P-04…P-09, H-* | `dispatcher.rs`, `diagnostics`, `storage`, workspace | Telemetría con tope, rotación por tamaño, `prepare_cached`, WAL para `Personal`, workspace único. | P-07 / P-09: `--delta` |

### Estado de remediación

| Orden | Estado (2026-10-25) |
|---|---|
| 1 | HECHO: T-01 fail-closed, T-01c. Los 3 tests de firma forjada se reescribieron con aprobación del Director. |
| 2 | HECHO: T-03 (DACL SY+OW, primera instancia, sin clientes remotos, nombre de 128 bits), P-01 (lectura exacta) y R-15. |
| 3 | HECHO: T-02b (dashboard), P-02 y P-03 en `window/mod.rs`. |
| 4 | PARCIAL: T-05, R-16 y verificación del PID del cliente del pipe con el kernel (`client_process_id`, autorizada en `channel.rs`). T-02 se difiere al delta de AppContainer por decisión del Director. T-06 pasa a `islands/security`. |
| 5 | HECHO: T-04 (autoridad hasta `/?#\`, sin userinfo ni puerto, sin punto final, rechazo de `%`, coincidencia por dominio padre en la blocklist y en los rastreadores). Fuera de alcance: homógrafos IDN y formas numéricas alternativas de IPv4. |
| 6 | PARCIAL: SEC-04 en el worker (solo http, https y `about:blank`; se deniegan `data:` y el resto de `about:`). Consultar al Coordinador por cada navegación iniciada por el contenido sigue pendiente de un `--delta`. |
| 7 | HECHO: R-01 (el bin consume la lib), R-13 (`Coordinator::evaluate_navigation` con el perfil real; sin pestaña → rechazo) y P-03 en `main.rs`. El fixture `coordinator_with_tab` crea perfil y pestaña, con aprobación del Director. `--smoke-test` se conserva por DELTA_04 L-I6. |
| 8 | HECHO: R-02 (eliminados `servo_ffi.rs` y el feature `engine-stub`; fuera de `cfg(test)`, `Stub` falla con `SandboxEnforcementFailed{120}`) y R-03 (`input_dispatcher.rs` se reduce a la traducción Win32 sin estado; fuera la cola muerta, `handle_win32_input` y `servo_events_processed`). Se retiraron 4 tests del código eliminado (2 de `servo_ffi` y 2 de la cola). Pendiente de `--delta`: el doc-comment de `EngineBackend` en `contracts/galapagos_core_v1.rs:367` aún menciona el feature. Copia de respaldo en el scratchpad de la sesión. |
| 9 | HECHO (salvo R-09, R-11 y R-18):<br>• R-05 y R-07: islas Research y Privacy eliminadas.<br>• R-06: Connectivity reducida a `validate_destination` + `validate_route`; la ruta se valida antes de escribir en SQLite.<br>• R-04: Performance sin `ThrottlingManager`, `MemoryExceeded` ni `CpuExceeded`.<br>• R-08: Freedom sin caché de permisos ni autoconfirmación.<br>• R-10: Sentinel solo con detección de bucles de redirección.<br>• R-12: Coordinator sin `#[allow(dead_code)]` ni `research_mut`.<br>Se conservan:<br>• R-09 `PayloadGuard`, porque `production_certification` (golden master) certifica las decisiones de descarga.<br>• R-11 `compute_sha256`, que sigue usándose para el digest.<br>• R-18, porque necesita un `--delta` del contrato.<br>Se retiraron 12 tests del código eliminado. LOC fuera de tests en `src/`: 7 726 → 5 675 (−26,5 % acumulado del incidente). |
| 10 | PARCIAL:<br>• P-04: `TelemetrySample` del contenido → `UnauthorizedCommand`, que cierra el canal. Decisión del Director: las muestras PERF-01 las toma el Coordinador.<br>• P-05: rotación por tamaño en cada escritura, sin `let _` en la rotación.<br>• P-03 y R-17: fuera `BOOTSTRAP_START`, el `println!` de debug, `log_info`, `trace_*` y `log_critical`; un solo cuerpo de log (`emit`).<br>• P-06: `prepare_cached` en las 2 consultas autorizadas y en los 2 inserts frecuentes.<br>Diferidos:<br>• P-07 (WAL para `Personal`): pasa a `/znve-contract --delta` por decisión del Director. Requiere cargar los perfiles existentes y una base por compartimento; con una sola base, los perfiles efímeros acabarían en disco (PRIV-01).<br>• P-09 (workspace único): `--delta`.<br>Los dos `let _` restantes ya están cerrados: el envío del resize y de la entrada informan una vez por ráfaga (`report_send_failure`) y `terminate_worker` registra `WORKER_TERMINATE_FAILED`. LOC fuera de tests en `src/`: 5 571. |
| P-07 | HECHO vía `DELTA_07` (congelado y ejecutado el 2026-10-26): WAL para `Personal` y una base `:memory:` por perfil efímero. INTEGRADO en el smoke test headless; falta la prueba del Director con ventana. |
| Obs. | `test_certify_ocean_defense_hot_overhead_sub_5us` mide 2-4 µs en debug frente a un límite de 5 µs: es inestable bajo carga (falló 1 vez en paralelo). En release mide 0,2-0,3 µs. Con `cargo test --release`, `test_golden_master_production_navigation_matrix` falla porque no existe un worker release junto al ejecutable (efecto de T-05 en el hotfix 4). |

**Cubo B** (va a `CONTRACT_BACKLOG.md` tras aprobación): verificador Ed25519 con dependencia, AppContainer y token restringido real, IPC `NavigationRequest` del worker hacia el Coordinador y workspace unificado.
