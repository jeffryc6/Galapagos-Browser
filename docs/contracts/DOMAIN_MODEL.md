# DOMAIN MODEL — Galapagos Browser v0.1

**Estado:** Contrato Formal de Entidades de Dominio (Fase B · Tarea 4)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Implementación Tipada en Rust:** [contracts/galapagos_core_v1.rs](file:///d:/App/Browser-SaveNav/contracts/galapagos_core_v1.rs)  

---

## 1. Principios de Modelado de Dominio (ZNVE-02 / ZNVE-05)

1. **Inmutabilidad y Opacidad:** Los identificadores (`InstanceId`, `ProfileId`, `WindowId`, `TabId`, `ProcessId`) son transparentes, opacos y fuertemente tipados (`#[repr(transparent)]`). No contienen datos personales, marcas de tiempo ni URLs codificadas.
2. **Cero campos parásitos:** Ninguna entidad contiene datos especulativos o punteros circulares hacia el árbol del DOM o el motor web.
3. **Separación Estricta:** El estado operativo de navegación jamás se mezcla con los búferes de diagnóstico o telemetría.

---

## 2. Catálogo de Entidades Formales

### 2.1. `BrowserInstance`
Representa una ejecución única y acotada del proceso Trusted Coordinator.
* **Campos:**
  * `instance_id: u64` — Identificador único generado al arranque.
  * `version_semver: (u16, u16, u16)` — Versión exacta del binario del navegador.
  * `lifecycle_state: BrowserState` — Estado actual en la máquina de estados maestro.
  * `started_at_epoch_s: u64` — Marca temporal de inicialización del proceso coordinador.
* **Garantías ZNVE:** Propietario raíz de los handles del sistema operativo y coordinador de la desconexión limpia.

### 2.2. `Profile`
Representa un compartimento aislado de almacenamiento, credenciales, cookies y permisos.
* **Campos:**
  * `profile_id: ProfileId` — Identificador inmutable (monótono incremental `u64`).
  * `compartment: ProfileCompartment` — `Personal`, `TemporaryPrivate` o `Laboratory`.
  * `privacy_level: PrivacyLevel` — `Daily`, `Ephemeral` o `Hardened`.
  * `route_policy: NetworkRoutePolicy` — `DirectSystem` o `CustomProxy` (RFC 1123).
  * `created_at_epoch_s: i64` — Marca temporal de registro.
* **Garantías ZNVE:** Aislamiento estricto de base de datos (SQLite WAL en disco para `Personal`; `:memory:` sin archivos residuales para `TemporaryPrivate`).

### 2.3. `BrowserWindow`
Representa una superficie de ventana nativa Win32 (HWND).
* **Campos:**
  * `window_id: u64` — Identificador de ventana.
  * `hwnd_raw: usize` — Descriptor nativo Win32 HWND.
  * `active_tab_id: Option<TabId>` — Pestaña actualmente visible en primer plano.
  * `associated_profile_id: ProfileId` — Perfil predeterminado para nuevas pestañas abiertas en esta ventana.
* **Garantías ZNVE:** La ventana nativa pertenece al UI Thread del Coordinador. El motor Servo no tiene propiedad ni control sobre el bucle de mensajes de la ventana anfitriona.

### 2.4. `Tab`
Representa una unidad lógica de navegación asociada a un proceso de contenido web.
* **Campos:**
  * `tab_id: TabId` — Identificador inmutable de pestaña (`u64`).
  * `profile_id: ProfileId` — Perfil al que pertenece la pestaña.
  * `current_url: String` — URL validada (máx. 2048 bytes; `https`, `http`, `about:blank`).
  * `process_id: ProcessId` — PID no-cero del proceso de renderizado worker.
  * `quota: ResourceQuota` — Presupuesto estricto de memoria (`working_set`), CPU y límites IPC.
  * `state: TabLifecycleState` — `Created`, `Loading`, `Active`, `Background`, `Terminating`, `Destroyed`.
* **Garantías ZNVE:** La terminación de una pestaña purga inmediatamente su `TabState` y señaliza la liberación de los recursos asociados.

### 2.5. `SiteContext`
Define el origen web y los permisos aplicables evaluados por Ocean Defense.
* **Campos:**
  * `origin_host: String` — Dominio normalizado en minúsculas (sin credenciales ni rutas).
  * `scheme: String` — Protocolo (`https` o `http`).
  * `port: u16` — Puerto explícito o implícito (443 / 80).
  * `effective_permissions: HashMap<SiteCapability, PermissionGrant>` — Permisos vigentes para el sitio en el perfil activo.
* **Garantías ZNVE:** Consultas parametrizadas sobre `site_permissions` sin escaneos completos.

### 2.6. `NetworkContext`
Configuración de transporte de red para un perfil o pestaña.
* **Campos:**
  * `policy: NetworkRoutePolicy` — Directa o Proxy.
  * `enforce_remote_dns: bool` — Forzar resolución DNS a través del proxy para evitar fugas.
  * `block_webrtc_leaks: bool` — Restringir interfaces ICE locales de WebRTC.
* **Garantías ZNVE:** Verificación RFC 1123 en host y puerto no-cero (`NonZeroU16`).

### 2.7. `DiagnosticSession`
Sesión de inspección acotada para investigación técnica (Research Island).
* **Campos:**
  * `session_id: u64` — Identificador de sesión de diagnóstico.
  * `target_tab_id: TabId` — Pestaña bajo inspección.
  * `allowed_probes: HashSet<ProbeType>` — Sondas autorizadas (CPU, Red, Memoria, Render).
  * `started_at_epoch_s: u64` — Inicio de la captura.
  * `max_duration_seconds: u32` — Límite estricto de recolección para evitar saturación de memoria.
* **Garantías ZNVE:** Requiere activación explícita. No se activa en navegación diaria estándar.
