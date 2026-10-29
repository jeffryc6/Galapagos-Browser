# RESOURCE BUDGETS — Galapagos Browser v0.1

**Estado:** Presupuestos Deterministas de Recursos y Rendimiento (Fase D · Tarea 12)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Contrato Tipado:** [contracts/galapagos_core_v1.rs](file:///d:/App/Browser-SaveNav/contracts/galapagos_core_v1.rs) (`ResourceQuota`)  

---

## 1. Principio de Presupuesto Rígido (ZNVE-03 / ZNVE-12)

Cada componente de Galapagos cuenta con un presupuesto finito de memoria física de trabajo (`working_set`), tiempo de CPU, actividad de disco y límites de mensajería IPC. Ningún módulo puede asignar memoria de forma ilimitada o retener hilos de CPU sin justificación técnica.

---

## 2. Presupuestos por Componente y Nivel de Privilegio

| Componente | Presupuesto RAM (Working Set Máx) | Presupuesto CPU (Basis Points) | Cuota IPC / I/O | Criterio de Reposo (Idle) |
|---|---|---|---|---|
| **Trusted Coordinator / UI Thread** | **< 64 MiB** | **< 1.00%** (100 bp en reposo) | Máx. 1 MiB por mensaje IPC; timeout 5 000 ms. | Event Loop Win32 bloqueado en `GetMessageW` esperando eventos reales. |
| **Pestaña Estándar (Servo Worker)** | **512 MiB** (`ResourceQuota`) | **50.00%** (5 000 bp máx.) | Sin acceso a disco directo; I/O canalizado por IPC. | Throttling a < 2.00% de CPU si pasa a estado `Background`. |
| **Pestaña Hardened (Alta Privacidad)**| **256 MiB** | **25.00%** (2 500 bp máx.) | Descarte agresivo de cachés de fuentes e imágenes. | Suspensión de timers JavaScript cuando pierde el foco visual. |
| **Ocean Defense (RequestGuard)** | **< 16 MiB** | **0.00%** en reposo; < 100 µs por evaluación | Lectura local en memoria indexada $O(1)$. | Cero hilos de sondeo en segundo plano. |
| **Persistencia SQLite WAL** | **< 32 MiB** (búfer de páginas) | Despreciable (I/O síncrono acotado a transacciones) | `PRAGMA cache_size = -32000` (máx. 32 MiB). | Conexión pasiva; checkpoints WAL fuera del UI Thread. |

> **DECISIÓN PENDIENTE (2026-10-26) — cuota del Servo Worker en prueba**
>
> El valor que aplica `src/main.rs` **no** es el de la tabla: **2 GiB** de memoria y **CPU sin tope** (10 000 bp, sin `JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP`). Es una prueba aprobada por el Director para confirmar la causa del render incompleto.
>
> - **Evidencia:**
>   - `worker_stderr.log`: `memory allocation of 2097218 bytes failed`.
>   - `app_trace.log` (2026-10-26 06:42 UTC): el worker termina con `0xC0000409` tras cargar google.com, con la cuota de 512 MiB.
>   - El tope duro del 50 % congela todos los hilos del worker al alcanzarlo, y así se pierden frames.
> - **Discrepancia a resolver:** la tabla dice *working set*, pero el Job aplica `JOB_OBJECT_LIMIT_PROCESS_MEMORY`, que limita la **memoria comprometida**: el límite real es más estricto de lo que dice el documento.
> - **Pendiente:**
>   1. Medir el «Tamaño de confirmación» del worker con la cuota de prueba (debug y, cuando exista, release; PERF-03).
>   2. Fijar los valores definitivos de las pestañas Estándar y Hardened.
>   3. Decidir si el límite de CPU vuelve como tope duro, como peso (`JOB_OBJECT_CPU_RATE_CONTROL_WEIGHT_BASED`) o desaparece.
>   4. Actualizar esta tabla y `main.rs`.

---

## 3. Escenarios de Prueba y Métricas de Aceptación (Línea Base)

Toda compilación del navegador debe satisfacer la siguiente matriz antes de su aprobación:

| Escenario de Prueba | Métrica Evaluada | Umbral de Aceptación ZNVE | Mecanismo de Medición |
|---|---|---|---|
| **Arranque en Frío (Cold Start)** | Tiempo hasta estado `Ready` | **< 350 ms** | Medición de alta precisión (`QueryPerformanceCounter`). |
| **Navegación Inicial (`about:blank`)** | Latencia de apertura de pestaña | **< 50 ms** | Tiempo desde `UiToCoordinatorCmd::SpawnTab` hasta `FrameReady`. |
| **Costo de Ocean Defense** | Sobrecarga de evaluación síncrona | **< 150 µs** por URL | Métrica `security_filter_overhead_us` en `PerformanceSampleDto`. |
| **Consumo en Reposo (1 Pestaña activa)**| Memoria privada combinada | **< 120 MiB** (Coordinador + Servo) | `GetProcessMemoryInfo` (`WorkingSetSize`). |
| **Purga de Sesión Efímera** | Tiempo de desecho y liberación | **< 15 ms** | Tiempo de ejecución de `destroy_ephemeral_profile`. |

---

## 4. Política de Contención ante Exceso de Cuota

Cuando un proceso de contenido web supera su cuota asignada:
1. **Detección Determinista:** El Windows Job Object notifica la violación (`JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT` o exceso de commit de memoria).
2. **Acción Invariable:** El Coordinador no permite la degradación del sistema anfitrión:
   * Termina el proceso worker de forma controlada (`TerminateProcess`).
   * Registra el evento tipado `GalapagosError::ResourceQuotaExceeded`.
   * Ofrece en la interfaz de la pestaña una pantalla nativa de recarga informando el límite superado.
