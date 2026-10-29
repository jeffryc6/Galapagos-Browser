### `CONTRATO DE ENTRADA Y SALIDA`

Especificación de fronteras de confianza para **Galapagos Browser v0.1** (Plataforma: `--platform=desktop`, Windows 10/11 64-bit, Rust + Servo Embedder API + Win32/WinUI 3 nativo, Licencia Apache-2.0 compatible con MPL-2.0). Cubre los requisitos **P0** de arquitectura, seguridad, privacidad y telemetría de rendimiento (`ARC-01`, `ARC-02`, `SEC-01`, `SEC-02`, `SEC-03`, `PRIV-01`, `PERF-01`).

```rust
// Ruta objetivo del contrato: contracts/galapagos_core_v1.rs
#![forbid(unsafe_code)] // Excepción acotada únicamente en el adaptador FFI de Win32/Servo

use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};

/// Identificadores opacos e inmutables de frontera
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProfileId(pub u64);

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessId(pub NonZeroU32);

/// PRIV-01: Niveles de privacidad y compartimentación de perfiles
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyLevel {
    /// Nivel 1: Navegación diaria, bloqueo de rastreadores y cookies 3P
    Daily,
    /// Nivel 2: Sesión efímera, sin persistencia local al cerrar, en RAM cuando sea viable
    Ephemeral,
    /// Nivel 3: Privacidad reforzada, partición estricta, defensa anti-fingerprinting y control WebRTC
    Hardened,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileCompartment {
    Personal,
    TemporaryPrivate,
    Laboratory,
}

/// SEC-01 / SEC-02: Nivel de aislamiento de proceso en Windows
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsSandboxLevel {
    /// Proceso coordinador/UI (sin ejecución de contenido web)
    TrustedCoordinator,
    /// Contenido web aislado en AppContainer + Job Object + Restricted Token
    AppContainerStrict,
    /// Servicios de red/almacenamiento sujetos a políticas explícitas
    RestrictedService,
}

/// NET-01 (Frontera base P0/P1): Modos de enrutamiento de red verificables
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkRoutePolicy {
    DirectSystem,
    CustomProxy {
        host: String, // Validado RFC 1123 / IP, máx 253 bytes
        port: NonZeroU16,
        enforce_remote_dns: bool,
        block_webrtc_leaks: bool,
    },
}

/// Límites deterministas de recursos por proceso de contenido (PERF-01 / SEC-01)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceQuota {
    pub max_working_set_bytes: NonZeroU64,
    pub cpu_rate_limit_basis_points: NonZeroU16, // 1..=10_000 (100.00%)
    pub max_ipc_payload_bytes: NonZeroU32,       // Límite estricto: 1_048_576 (1 MiB)
    pub ipc_timeout_ms: NonZeroU32,              // Límite estricto: <= 5_000 ms
}

/// ARC-02: Comandos unidireccionales desde la Interfaz Nativa hacia el Coordinador de Confianza.
/// El contenido web NUNCA emite estos comandos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiToCoordinatorCmd {
    CreateProfile {
        compartment: ProfileCompartment,
        privacy_level: PrivacyLevel,
        route_policy: NetworkRoutePolicy,
    },
    SpawnTab {
        profile_id: ProfileId,
        initial_url: String, // Esquemas permitidos: https://, http://, about:blank (máx 2048 bytes)
        quota: ResourceQuota,
    },
    Navigate {
        tab_id: TabId,
        target_url: String,
    },
    SetSitePermission {
        profile_id: ProfileId,
        origin: String,
        capability: SiteCapability,
        grant: PermissionGrant,
    },
    TerminateTab {
        tab_id: TabId,
        reason: TerminationReason,
    },
    DestroyEphemeralProfile {
        profile_id: ProfileId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteCapability {
    ClipboardRead,
    Notifications,
    MediaCapture,
    WebRtcPeerConnection,
    LocalStorage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionGrant {
    Deny,
    AllowSessionOnly,
    AllowExplicitRevocable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationReason {
    UserClosed,
    QuotaExceeded,
    SandboxViolation,
    UnresponsiveEngine,
}

/// ARC-01 / ARC-02: Mensajes IPC estrictamente validados desde el proceso Servo (Hostil) al Coordinador
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentToCoordinatorIpc {
    FrameReady {
        tab_id: TabId,
        frame_seq: u64,
    },
    RequestNetworkFetch {
        tab_id: TabId,
        request_id: u64,
        url: String,
        method: HttpMethod,
        is_third_party: bool,
    },
    RequestCapability {
        tab_id: TabId,
        origin: String,
        capability: SiteCapability,
    },
    TelemetrySample(PerformanceSampleDto),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Head,
    Options,
    Put,
    Delete,
}

/// PERF-01: DTO inmutable para el banco de pruebas de CPU, RAM, latencia y costo de seguridad
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerformanceSampleDto {
    pub tab_id: TabId,
    pub process_id: ProcessId,
    pub timestamp_monotonic_ns: u64,
    pub private_working_set_bytes: u64,
    pub cpu_time_delta_us: u64,
    pub navigation_latency_ms: u32,
    pub security_filter_overhead_us: u32,
    pub blocked_trackers_count: u32,
}

/// SEC-03: DTO de verificación criptográfica de actualizaciones y listas de reglas
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedArtifactManifest {
    pub artifact_id: String,
    pub version_semver: (u16, u16, u16),
    pub sha256_digest: [u8; 32],
    pub ed25519_signature: [u8; 64],
}

/// Trait de frontera para el motor de renderizado (ARC-01)
pub trait WebEnginePort {
    fn initialize_sandboxed_worker(
        &mut self,
        tab_id: TabId,
        sandbox: WindowsSandboxLevel,
        quota: ResourceQuota,
    ) -> Result<ProcessId, GalapagosError>;

    fn dispatch_navigation(
        &mut self,
        tab_id: TabId,
        validated_url: &str,
    ) -> Result<(), GalapagosError>;

    fn terminate_worker(&mut self, process_id: ProcessId) -> Result<(), GalapagosError>;
}

```

---

### `CONTRATO DE PERSISTENCIA`

Motor de persistencia local: **SQLite en modo WAL (`PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;`)** para perfiles persistentes (`Personal`).
Regla estricta para `ProfileCompartment::TemporaryPrivate` y `Laboratory` (`PRIV-01` / `PRIV-02`): base de datos abierta exclusivamente con URI `:memory:` sin archivos `.wal` o `.shm` en disco, y limpieza de buffers al cierre. Prohibido `SELECT *`; todas las consultas usan proyecciones explícitas e índices cubrientes.

```sql
-- Esquema en contracts/schema_v1.sql

-- 1. Metadatos de Perfiles Compartimentados (PRIV-01)
CREATE TABLE IF NOT EXISTS profiles (
    profile_id INTEGER PRIMARY KEY,
    compartment_code INTEGER NOT NULL CHECK (compartment_code IN (1, 2, 3)),
    privacy_level_code INTEGER NOT NULL CHECK (privacy_level_code IN (1, 2, 3)),
    created_at_epoch_s INTEGER NOT NULL
) STRICT;

-- 2. Permisos explícitos y reversibles por sitio (SEC-02 / Principio 6)
CREATE TABLE IF NOT EXISTS site_permissions (
    profile_id INTEGER NOT NULL,
    origin_host TEXT NOT NULL,
    capability_code INTEGER NOT NULL,
    grant_state INTEGER NOT NULL CHECK (grant_state IN (0, 1, 2)),
    updated_at_epoch_s INTEGER NOT NULL,
    PRIMARY KEY (profile_id, origin_host, capability_code),
    FOREIGN KEY (profile_id) REFERENCES profiles(profile_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

CREATE INDEX IF NOT EXISTS idx_site_permissions_lookup
    ON site_permissions (profile_id, origin_host, capability_code, grant_state);

-- 3. Auditoría de paquetes y listas firmadas (SEC-03)
CREATE TABLE IF NOT EXISTS verified_artifacts (
    artifact_id TEXT PRIMARY KEY,
    version_major INTEGER NOT NULL,
    version_minor INTEGER NOT NULL,
    version_patch INTEGER NOT NULL,
    sha256_hex TEXT NOT NULL CHECK (length(sha256_hex) = 64),
    verified_at_epoch_s INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

-- 4. Línea base reproducible de rendimiento (PERF-01)
CREATE TABLE IF NOT EXISTS perf_baseline_samples (
    sample_id INTEGER PRIMARY KEY,
    tab_id INTEGER NOT NULL,
    timestamp_ns INTEGER NOT NULL,
    working_set_bytes INTEGER NOT NULL,
    cpu_delta_us INTEGER NOT NULL,
    nav_latency_ms INTEGER NOT NULL,
    sec_overhead_us INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_perf_tab_time
    ON perf_baseline_samples (tab_id, timestamp_ns, working_set_bytes, cpu_delta_us);

```

**Proyecciones autorizadas (Queries parametrizadas obligatorias):**

* **Verificación de permiso en ruta caliente:**
`SELECT grant_state FROM site_permissions WHERE profile_id = ?1 AND origin_host = ?2 AND capability_code = ?3 LIMIT 1;`
* **Lectura de línea base de memoria (`PERF-01`):**
`SELECT timestamp_ns, working_set_bytes, cpu_delta_us, sec_overhead_us FROM perf_baseline_samples WHERE tab_id = ?1 ORDER BY timestamp_ns DESC LIMIT ?2;`

---

### `CONTRATO DE ERRORES`

Tipos cerrados deterministas. Cero `Box<dyn Error>`, cero `anyhow` y cero silenciamiento de fallos.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GalapagosError {
    /// ARC-01 / ARC-02: Violación de formato o límite en canal IPC desde contenido web
    IpcBoundaryViolation {
        tab_id: TabId,
        reason: IpcViolationReason,
    },
    /// SEC-01: Fallo al aplicar AppContainer, Job Object o mitigación de proceso en Windows
    SandboxEnforcementFailed {
        win32_error_code: u32,
        target_level: WindowsSandboxLevel,
    },
    /// SEC-02: Intento de acceso a capacidad o archivo no autorizado
    PermissionDenied {
        profile_id: ProfileId,
        origin: String,
        capability: SiteCapability,
    },
    /// SEC-03: Firma Ed25519 o hash SHA-256 inválido en actualización o lista de bloqueo
    IntegrityVerificationFailed {
        artifact_id: String,
    },
    /// PRIV-01: Intento de cruzar datos entre perfiles o persistir en sesión efímera
    CompartmentIsolationBreach {
        source_profile: ProfileId,
        target_profile: ProfileId,
    },
    /// PERF-01: Proceso de contenido superó el presupuesto asignado o dejó de responder
    ResourceQuotaExceeded {
        tab_id: TabId,
        process_id: ProcessId,
        observed_bytes: u64,
        limit_bytes: u64,
    },
    /// Fallo acotado de persistencia SQLite
    StorageFailure {
        sqlite_extended_code: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcViolationReason {
    PayloadTooLarge,
    InvalidUrlScheme,
    MalformedMessage,
    UnauthorizedCommand,
    ChannelTimeout,
}

```

---

### `ANTI-BLOAT FENCE`

1. **Paquetes y Runtimes Prohibidos:**
* Prohibido Chromium/CEF/WebView2 para el renderizado web principal (viola la tesis de arquitectura `Servo + Rust`).


* Prohibido Electron o interfaces construidas con tecnologías web dentro del proceso privilegiado (`ARC-02` exige separar la interfaz nativa de escritorio del motor web para no compartir superficie de ataque ni bloquear la UI si el motor falla).


* Prohibidos ORMs pesados (`diesel`, `sea-orm`) y gestores de errores dinámicos (`anyhow`, `eyre`). Se utiliza SQL parametrizado directo sobre `rusqlite` y enums cerrados.
* Prohibida cualquier SDK de telemetría externa, analítica de terceros o sincronización en la nube obligatoria (Principio 1: Privacidad por defecto y Pilar 5: Libertad de navegación).


* Prohibidos módulos de evasión activa de sistemas anti-bot de terceros (CAPTCHA solvers/bypassers); el módulo anti-bot se limita estrictamente a defensa contra automatización abusiva local y reducción de huella (fingerprinting).




2. **Excepciones Justificadas en el Stack:**
* `libservo` (MPL-2.0): Motor web embebido para DOM, CSS, layout y ejecución JS. Alternativa nativa descartada: programar un motor de renderizado web desde cero es inviable.


* `windows-sys` (MIT/Apache-2.0): Bindings FFI de costo cero hacia las APIs nativas de Windows (`CreateAppContainerProfile`, `CreateJobObjectW`, `AssignProcessToJobObject`, `UpdateProcThreadAttribute` y Win32 UI) para aislamiento real de procesos (`SEC-01`) y ventana nativa (`ARC-02`).




3. **Filtro de Diferimiento (`contracts/CONTRACT_BACKLOG.md`):**
* Se difieren fuera del contrato v1 (Fase P0 / Etapa 0-2) todas las capacidades clasificadas como **P1** y **P2** en la especificación:


* `PRIV-02` (P1): Eliminación verificable avanzada de artefactos de memoria virtual/volcados en sesiones efímeras.


* `PERF-02` (P1): Suspensión y reanudación automática de pestañas inactivas.


* `DEV-01` y `DEV-02` (P1/P2): Centro de diagnóstico propio (`Network Lab`, `Performance Lab`, `Security Lab`, `Storage Lab`) e integración progresiva con Firefox DevTools.


* `NET-01` (P1): Enrutador de proxies activos e integración opcional futura con Tor (`Connectivity Island`).


* `EXT-01` y `EXT-02` (P1/P2): Definición de permisos de extensiones y sistema de extensiones instalables.


* Expansión multiplataforma a Linux.







Contrato v1 sólido y cerrado. Listo para /znve-execute.