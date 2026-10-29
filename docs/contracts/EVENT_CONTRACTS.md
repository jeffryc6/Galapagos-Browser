# EVENT CONTRACTS — Galapagos Browser v0.1

**Estado:** Contrato Formal del Sistema de Eventos (Fase B · Tarea 6)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  

---

## 1. Principios del Sistema de Eventos (ZNVE-01 / ZNVE-11)

1. **Cero Ruido:** No existen eventos de "latido" periódico (`heartbeat`) ni telemetría rutinaria de éxito ("OK"). Los eventos se emiten exclusivamente ante transiciones de estado reales o anomalías verificadas.
2. **Contexto Mínimo Tipado:** Ningún evento transporta objetos completos del motor (como árboles DOM, cookies o payloads de respuesta HTTP). Se transmiten exclusivamente identificadores inmutables (`TabId`, `ProfileId`) y códigos de motivo.
3. **Privacidad por Defecto:** Las URLs completas con cadenas de consulta (`query params`), credenciales o tokens de autenticación están estrictamente prohibidas en el cuerpo de los eventos.

---

## 2. Taxonomía Cerrada de Eventos

```
GalapagosEvent
   ├── LifecycleEvent     (Arranque, parada, suspensión, recuperación)
   ├── NavigationEvent    (Inicio, compromiso, bloqueo defensivo, finalización)
   ├── ResourceEvent      (Alerta de cuota, pico de memoria, throttling de CPU)
   ├── SecurityEvent      (Violación de sandbox, intento de escape, firma inválida)
   ├── PrivacyEvent       (Purga de perfil efímero, aislamiento de cookie third-party)
   ├── PerformanceEvent   (Muestra de latencia, overhead de filtrado de seguridad)
   └── DiagnosticEvent    (Trazas bajo demanda exclusivas de Research Island)
```

### 2.1. `LifecycleEvent`
* **Productor:** Galapagos Core (`Coordinator`).
* **Consumidores Autorizados:** Window Manager, Security Island.
* **Variantes:**
  * `BrowserStateChanged { previous: BrowserLifecycleState, current: BrowserLifecycleState }`
  * `TabSpawned { tab_id: TabId, profile_id: ProfileId, process_id: ProcessId }`
  * `TabTerminated { tab_id: TabId, reason: TerminationReason }`
  * `ProfilePurged { profile_id: ProfileId, tabs_closed: usize }`

### 2.2. `NavigationEvent`
* **Productor:** Coordinador / Ocean Defense.
* **Consumidores Autorizados:** UI Thread (actualización de barra de direcciones), History Manager.
* **Variantes:**
  * `NavigationStarted { tab_id: TabId, origin_host: String }`
  * `NavigationBlocked { tab_id: TabId, origin_host: String, reason: &'static str }`
  * `NavigationCommitted { tab_id: TabId, origin_host: String, is_secure: bool }`
  * `NavigationFailed { tab_id: TabId, error_code: i32 }`

### 2.3. `ResourceEvent`
* **Productor:** Performance Island (Job Object monitor).
* **Consumidores Autorizados:** Resource Budget Manager, Tab Manager.
* **Variantes:**
  * `QuotaWarning { tab_id: TabId, observed_bytes: u64, limit_bytes: u64 }`
  * `QuotaExceededTerminated { tab_id: TabId, process_id: ProcessId }`
  * `CpuThrottlingApplied { tab_id: TabId, rate_basis_points: u16 }`

### 2.4. `SecurityEvent`
* **Productor:** Ocean Defense / Security Island.
* **Consumidores Autorizados:** Trusted Coordinator, Audit Log local.
* **Variantes:**
  * `SandboxViolationBlocked { process_id: ProcessId, detail: &'static str }`
  * `IpcMessageRejected { tab_id: TabId, violation: IpcViolationReason }`
  * `ArtifactSignatureInvalid { artifact_id: String }`

### 2.5. `PrivacyEvent`
* **Productor:** Privacy Island / Anti-Tracking Shield.
* **Consumidores Autorizados:** Trusted Coordinator, UI Shield Badge.
* **Variantes:**
  * `TrackerBlocked { tab_id: TabId, tracker_domain: String }`
  * `EphemeralStorageWiped { profile_id: ProfileId }`
  * `CookiePartitionEnforced { tab_id: TabId, origin_host: String }`

### 2.6. `PerformanceEvent`
* **Productor:** Performance Island.
* **Consumidores Autorizados:** SQLite Storage (`perf_baseline_samples`).
* **Variantes:**
  * `SampleRecorded(PerformanceSampleDto)`

### 2.7. `DiagnosticEvent`
* **Productor:** Research Island.
* **Consumidores Autorizados:** Diagnostic Lab UI.
* **Restricción:** Solo se emite si existe una `DiagnosticSession` activa autorizada.
* **Variantes:**
  * `ProbeCapture { session_id: u64, probe_type: u8, payload_len: u32 }`

---

## 3. Ciclo de Vida de Suscripciones y Despacho

1. **Aislamiento de Fallos:** Si un consumidor lanza un pánico o falla al procesar un evento, el hilo emisor del Coordinador no se interrumpe ni bloquea (`fail-safe`).
2. **Propiedad Determinista:** Las suscripciones a eventos están vinculadas al ciclo de vida del componente suscriptor (`SubscriptionHandle`). Al ser descartado (`Drop`), la suscripción se desregistra de inmediato.
3. **Búferes Acotados:** Los canales de eventos son de capacidad fija (máx. 128 mensajes). Si un consumidor no drena sus eventos, los eventos no críticos se descartan deterministamente registrando una advertencia en el log de excepciones.
