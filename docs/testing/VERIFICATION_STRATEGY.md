# VERIFICATION STRATEGY — Galapagos Browser v0.1

**Estado:** Estrategia Maestra de Verificación, Pruebas y Arnés de Seguridad (Fase E · Tarea 17)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Suite Activa:** 18 pruebas unitarias en `src/` validadas al 100%.  

---

## 1. Principio Rector: Verificación sobre Escenarios Reales de Producción

> **Directiva de Arquitectura:** En Galapagos Browser **no se admiten escenarios de prueba sintéticos, mocks desconectados de la realidad ni datos simulados**. La validación del navegador se ejecuta sobre **escenarios de navegación real de producción**:
> 1. Tráfico y navegación a URLs reales en la web viva (ej.: `https://servo.org`, dominios de prueba reales HTTP/HTTPS).
> 2. Procesamiento real de descriptores Win32 (`HWND`) y bucles de eventos del sistema operativo.
> 3. Almacenamiento real en SQLite mediante transacciones atómicas (WAL en disco para perfiles reales y RAM efectiva para efímeros).
> 4. Interceptación y bloqueo síncrono en tiempo real frente a peticiones web vivas.

```
Nivel 1: Invariantes y Contratos de Tipos (contracts/galapagos_core_v1.rs)
           │
           ▼
Nivel 2: Integración de Módulos Productivos (Coordinator + Storage SQLite + RequestGuard)
           │
           ▼
Nivel 3: Navegación Real en Producción (Ventana Win32 + Host Servo + Red Real)
           │
           ▼
Nivel 4: Línea Base de Telemetría Real (PERF-01 / perf_baseline_samples)
```

---

## 2. Batería de Pruebas Unitarias y de Integración

### 2.1. Pruebas de Contratos de Entrada y Frontera
* **Validación de Esquema de URL (`test_url_scheme_validation`):** Valida esquemas autorizados (`https://`, `http://`, `about:blank`), rechazo estricto de esquemas peligrosos (`javascript:`, `file:`, `data:`) y límite de 2048 bytes.
* **Invariantes de Cuota (`test_resource_quota_invariants`):** Valida límites estrictos para cuotas de memoria de trabajo, rate limit de CPU y cuota máxima IPC (1 MiB).
* **Validación RFC 1123 (`test_proxy_host_rfc1123_validation`):** Verificación sintáctica estricta de hosts de proxy para `NetworkRoutePolicy::CustomProxy`.

### 2.2. Pruebas de Persistencia y Concurrencia SQLite
* **Ciclo de Vida y Proyecciones (`test_storage_lifecycle_and_projections`):** Inserción de perfiles, consulta en ruta caliente de permisos y revocabilidad atómica.
* **Integridad Referencial (`test_foreign_key_and_integrity_constraints`):** Comprobación de que la inserción de permisos para un `ProfileId` inexistente falle deterministamente por `FOREIGN KEY`.
* **Proyección de Telemetría (`test_perf_baseline_projection`):** Consulta ordenada DESC por `timestamp_ns` con límite parametrizado.

### 2.3. Pruebas de Ocean Defense
* **Filtrado síncrono (`test_navigation_evaluation_allow_and_block`):** Bloqueo síncrono de hosts maliciosos registrados en Threat Intelligence local.
* **Endurecimiento de Privacidad (`test_hardened_privacy_level_http_restriction`):** Restricción de tráfico HTTP plano en perfiles `Hardened`.
* **Protección contra Rastreo (`test_anti_tracking_third_party_shield`):** Bloqueo de balizas third-party y aislamiento de contexto.
* **Barrera de Descargas (`test_executable_payload_download_guard`):** Confirmación obligatoria (`RequireConfirmation`) ante archivos ejecutables.

---

## 3. Matriz de Pruebas de Fallos Controlados y Seguridad (Negative Testing)

| Escenario de Fallo | Comportamiento Esperado | Verificación Automatizada |
|---|---|---|
| **URL con credenciales embebidas (`user:pass@host`)** | Rechazo inmediato con error `CredentialsInUrl`. | `test_strict_url_validation_audit` |
| **URL con caracteres de control ASCII (< 0x20 o 0x7F)**| Rechazo inmediato con error `ControlCharacterDetected`. | `test_strict_url_validation_audit` |
| **Desconexión del subsistema de persistencia** | Retorno determinista de `GalapagosError::StorageUnattached` sin supresión silenciosa. | `test_zero_silent_suppression_audit` |
| **Destrucción de sesión efímera** | Purga de todas las pestañas vinculadas y remoción del perfil de memoria. | `test_ephemeral_profile_destruction` |
| **Agotamiento de espacio de identificadores** | Asignación monótona sin colisiones ni reutilización prematura de IDs. | `test_monotonic_id_allocation_audit` |

---

## 4. Arnés Golden Master y Regresión de Rendimiento

1. **Protocolo Golden Master (ZNVE Escenario 5):**
   * Las respuestas de evaluación de Ocean Defense y las proyecciones de SQLite se contrastan contra snapshots fijados en disco.
   * Modificar un snapshot requiere aprobación explícita del Director de Arquitectura.
2. **Línea Base de Rendimiento (`cargo bench` / PERF-01):**
   * Medición continua de la sobrecarga de evaluación de `RequestGuard` (< 150 µs).
   * Almacenamiento de muestras históricas en la tabla `perf_baseline_samples` para detectar regresiones de CPU o memoria en cada pull request.

---

## 5. Procedimiento de Ejecución en Dos Pasos

Para garantizar cero ruido en el bucle de desarrollo, la verificación sigue el protocolo:

1. **Paso 1: Verificación quirúrgica del objetivo:**
   ```powershell
   cargo test <nombre_del_test> -- --quiet
   ```
2. **Paso 2: Suite completa con salida por excepción:**
   ```powershell
   cargo test -- --quiet
   ```
   * Si todos los tests pasan: se reporta el resumen condensado (`ok. 18 passed; 0 failed`).
   * Si ocurre un fallo: se detiene en el primer error reportando `FALLO <archivo>:<línea> · esperado <x> · recibido <y>`.
