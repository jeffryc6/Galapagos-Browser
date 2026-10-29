//! contracts/galapagos_core_v1.rs
//! Especificación ejecutable de fronteras de confianza para Galapagos Browser v0.1
//! Plataforma: Windows 10/11 64-bit, Rust + Servo Embedder API + Win32/WinUI 3 nativo.
//! Gobernado bajo estándar ZNVE v2.4.0 (Axiomas 1 y 2).

#![forbid(unsafe_code)]

use std::fmt;
use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};

/// Identificador inmutable y opaco de perfil de usuario
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileId(pub u64);

/// Identificador inmutable y opaco de pestaña de navegación
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabId(pub u64);

/// Identificador inmutable y no-cero de proceso de contenido en el sistema operativo
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub NonZeroU32);

/// PRIV-01: Niveles de privacidad y compartimentación de perfiles
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrivacyLevel {
    /// Nivel 1: Navegación diaria, bloqueo de rastreadores y cookies 3P
    Daily,
    /// Nivel 2: Sesión efímera, sin persistencia local al cerrar, en RAM cuando sea viable
    Ephemeral,
    /// Nivel 3: Privacidad reforzada, partición estricta, defensa anti-fingerprinting y control WebRTC
    Hardened,
}

/// PRIV-01: Compartimento del perfil de navegación
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProfileCompartment {
    Personal,
    TemporaryPrivate,
    Laboratory,
}

/// SEC-01 / SEC-02: Nivel de aislamiento de proceso en Windows
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowsSandboxLevel {
    /// Proceso coordinador/UI (sin ejecución de contenido web)
    TrustedCoordinator,
    /// Contenido web aislado en AppContainer + Job Object + Restricted Token
    AppContainerStrict,
    /// Servicios de red/almacenamiento sujetos a políticas explícitas
    RestrictedService,
}

/// NET-01: Modos de enrutamiento de red verificables
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

impl NetworkRoutePolicy {
    /// Validación RFC 1123 estricta para host de proxy (máx 253 bytes, etiquetas <= 63 bytes)
    pub fn is_valid_proxy_host(host: &str) -> bool {
        if host.is_empty() || host.len() > 253 {
            return false;
        }
        for label in host.split('.') {
            if label.is_empty() || label.len() > 63 {
                return false;
            }
            if label.starts_with('-') || label.ends_with('-') {
                return false;
            }
            if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return false;
            }
        }
        true
    }
}

/// Constantes estrictas de límites para cuotas de recursos (PERF-01 / SEC-01)
pub const MAX_ALLOWED_IPC_PAYLOAD_BYTES: u32 = 1_048_576; // 1 MiB estricto
pub const MAX_ALLOWED_IPC_TIMEOUT_MS: u32 = 5_000;         // 5 000 ms estricto
pub const MAX_CPU_RATE_LIMIT_BASIS_POINTS: u16 = 10_000;   // 100.00%
pub const MAX_ALLOWED_URL_LEN: usize = 2048;               // 2048 bytes estricto
pub const MAX_ALLOWED_TITLE_LEN: usize = 512;              // DELTA_02: 512 bytes estricto

/// Límites deterministas de recursos por proceso de contenido (PERF-01 / SEC-01)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceQuota {
    pub max_working_set_bytes: NonZeroU64,
    pub cpu_rate_limit_basis_points: NonZeroU16, // 1..=10_000 (100.00%)
    pub max_ipc_payload_bytes: NonZeroU32,       // Límite estricto: <= 1_048_576 (1 MiB)
    pub ipc_timeout_ms: NonZeroU32,              // Límite estricto: <= 5_000 ms
}

impl ResourceQuota {
    /// Constructor validado contra las invariantes de frontera del contrato
    pub fn new(
        max_working_set_bytes: NonZeroU64,
        cpu_rate_limit_basis_points: NonZeroU16,
        max_ipc_payload_bytes: NonZeroU32,
        ipc_timeout_ms: NonZeroU32,
    ) -> Result<Self, GalapagosError> {
        if cpu_rate_limit_basis_points.get() > MAX_CPU_RATE_LIMIT_BASIS_POINTS {
            return Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::MalformedMessage,
            });
        }
        if max_ipc_payload_bytes.get() > MAX_ALLOWED_IPC_PAYLOAD_BYTES {
            return Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::PayloadTooLarge,
            });
        }
        if ipc_timeout_ms.get() > MAX_ALLOWED_IPC_TIMEOUT_MS {
            return Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::ChannelTimeout,
            });
        }
        Ok(Self {
            max_working_set_bytes,
            cpu_rate_limit_basis_points,
            max_ipc_payload_bytes,
            ipc_timeout_ms,
        })
    }
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

impl UiToCoordinatorCmd {
    /// Validador de esquemas seguros de URL de navegación (ARC-02 / SEC-01)
    pub fn validate_url_scheme(url: &str) -> Result<(), IpcViolationReason> {
        Self::validate_url_strict(url)
    }

    /// Validador estricto de URLs conforme a RFC 3986 sin dependencias externas
    pub fn validate_url_strict(url: &str) -> Result<(), IpcViolationReason> {
        if url.len() > MAX_ALLOWED_URL_LEN {
            return Err(IpcViolationReason::PayloadTooLarge);
        }

        // 1. Prohibir caracteres de control ASCII
        if url.bytes().any(|b| b <= 0x20 || b == 0x7F) {
            return Err(IpcViolationReason::ControlCharacterDetected);
        }

        // 2. Esquema seguro about:blank
        if url == "about:blank" {
            return Ok(());
        }

        // 2b. Esquema reservado de UI Nativa del Coordinador (galapagos://)
        if let Some(target) = url.strip_prefix("galapagos://") {
            match target {
                "dashboard" | "security" | "performance" => return Ok(()),
                _ => return Err(IpcViolationReason::InvalidUrlScheme),
            }
        }

        // 3. Validación de esquemas HTTP/HTTPS
        let rest = if let Some(stripped) = url.strip_prefix("https://") {
            stripped
        } else if let Some(stripped) = url.strip_prefix("http://") {
            stripped
        } else {
            return Err(IpcViolationReason::InvalidUrlScheme);
        };

        // 4. Prohibición de credenciales embebidas (user:pass@host)
        let authority = rest.split('/').next().unwrap_or("");
        if authority.contains('@') {
            return Err(IpcViolationReason::CredentialsInUrl);
        }

        // 5. Host no vacío
        if authority.is_empty() {
            return Err(IpcViolationReason::MalformedHost);
        }

        Ok(())
    }
}

/// DELTA_02: Texto crudo escrito por el usuario en la barra de direcciones (1..=2048 bytes UTF-8)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressBarInput {
    pub raw: String,
}

/// DELTA_02: Normaliza el texto de la barra a una URL absoluta y la valida con `validate_url_scheme`.
/// Regla única, sin heurísticas de búsqueda:
///   1. trim; vacío → `MalformedMessage`; > 2048 bytes → `PayloadTooLarge`
///   2. contiene "://" o es "about:blank" → se usa tal cual
///   3. forma `esquema:resto` sin "//" (resto no numérico, p. ej. `javascript:`, `data:`) → `InvalidUrlScheme`
///   4. en otro caso se antepone "https://" (cubre `servo.org` y `host:puerto`)
pub fn normalize_address_input(input: &AddressBarInput) -> Result<String, IpcViolationReason> {
    let trimmed = input.raw.trim();
    if trimmed.is_empty() {
        return Err(IpcViolationReason::MalformedMessage);
    }
    if trimmed.len() > MAX_ALLOWED_URL_LEN {
        return Err(IpcViolationReason::PayloadTooLarge);
    }

    let candidate = if trimmed.contains("://") || trimmed == "about:blank" {
        trimmed.to_string()
    } else {
        if let Some((scheme, rest)) = trimmed.split_once(':') {
            let mut chars = scheme.chars();
            let scheme_like = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
            if scheme_like && !rest.starts_with(|c: char| c.is_ascii_digit()) {
                return Err(IpcViolationReason::InvalidUrlScheme);
            }
        }
        format!("https://{}", trimmed)
    };

    if candidate.len() > MAX_ALLOWED_URL_LEN {
        return Err(IpcViolationReason::PayloadTooLarge);
    }
    UiToCoordinatorCmd::validate_url_scheme(&candidate)?;
    Ok(candidate)
}

/// Capacidades de sitio web sujetas a gobernanza de permisos
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SiteCapability {
    ClipboardRead,
    Notifications,
    MediaCapture,
    WebRtcPeerConnection,
    LocalStorage,
}

/// Estado de concesión de permisos explícitos y revocables
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionGrant {
    Deny,
    AllowSessionOnly,
    AllowExplicitRevocable,
}

/// Motivos deterministas de terminación de procesos de pestaña
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// DELTA_02: URL real de la pestaña tras redirecciones (MSG_TYPE_URL_CHANGED = 6, url <= 2048 B)
    UrlChanged {
        tab_id: TabId,
        url: String,
    },
    /// DELTA_02: Título de la pestaña (MSG_TYPE_TITLE_CHANGED = 7, title <= 512 B, UTF-8 válido)
    TitleChanged {
        tab_id: TabId,
        title: String,
    },
    /// DELTA_05: motivo de un fallo propio del worker, enviado justo antes de terminar
    /// (MSG_TYPE_WORKER_FAULT = 8, message <= 512 B, UTF-8 válido, sin caracteres de control)
    WorkerFault {
        tab_id: TabId,
        message: String,
    },
    /// DELTA_08 §2.2: estado real de navegación del WebView activo
    /// (MSG_TYPE_NAV_STATE = 9, `tab_id u64 · can_back u8 · can_forward u8 · loading u8`, bytes 0/1)
    NavState {
        tab_id: TabId,
        can_back: bool,
        can_forward: bool,
        loading: bool,
    },
    /// DELTA_11 D49: una página cayó y Servo la sustituyó por su página de fallo; el worker sigue
    /// vivo (MSG_TYPE_CONTENT_CRASHED = 18, reason <= 512 B, UTF-8 válido, sin caracteres de control)
    ContentCrashed {
        tab_id: TabId,
        reason: String,
    },
}

/// Métodos HTTP permitidos en peticiones de contenido
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// Backend del motor. `Stub` solo existe con cfg(test) o feature "engine-stub".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineBackend {
    Real,
    Stub,
}

/// Primer mensaje worker → coordinador (MSG_TYPE_HANDSHAKE = 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerHandshake {
    pub process_id: ProcessId,                   // PID real (GetCurrentProcessId en el worker)
    pub applied_sandbox: WindowsSandboxLevel,    // nivel REALMENTE aplicado
    pub protocol_version: NonZeroU16,            // v1 = 1
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

/// Tipos cerrados deterministas de error en el runtime de Galapagos
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
    /// SEC-AUDIT-02: Fallo al despachar comandos de persistencia sin un backend de almacenamiento activo
    StorageUnattached,
    /// SEC-AUDIT-03: Agotamiento del espacio de identificadores monótonos
    IdentifierSpaceExhausted,
    /// Δ7: la persistencia no puede garantizar su contrato (WAL, versión de esquema o compartimentación)
    PersistenceInvariantViolated {
        reason: PersistenceViolation,
    },
}

/// Δ7: motivos cerrados de `GalapagosError::PersistenceInvariantViolated`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceViolation {
    /// `%LOCALAPPDATA%` ausente o carpeta `profiles` no creable
    DataDirUnavailable,
    /// `journal_mode=WAL` no fue aceptado por el sistema de archivos
    WalUnavailable,
    /// `user_version` mayor que `SCHEMA_VERSION`: base de un binario más nuevo
    SchemaVersionUnsupported { found: i64 },
    /// Fila de un compartimento no persistente dentro de la base en disco
    ForeignCompartmentRow { profile_id: ProfileId },
    /// `CustomProxy` en un perfil persistente: el esquema v1 no guarda la ruta
    RoutePolicyNotPersistable,
}

impl fmt::Display for PersistenceViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DataDirUnavailable => write!(f, "Directorio de datos del perfil no disponible (%LOCALAPPDATA%)"),
            Self::WalUnavailable => write!(f, "El sistema de archivos no admite journal_mode=WAL"),
            Self::SchemaVersionUnsupported { found } => {
                write!(f, "Versión de esquema {} no admitida (máxima: {})", found, SCHEMA_VERSION)
            }
            Self::ForeignCompartmentRow { profile_id } => {
                write!(f, "Perfil {:?} de un compartimento no persistente en la base en disco", profile_id)
            }
            Self::RoutePolicyNotPersistable => {
                write!(f, "La ruta CustomProxy no se puede persistir en el esquema v1")
            }
        }
    }
}

/// Δ7: fila proyectada al cargar los perfiles persistidos (solo compartimento Personal)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistedProfileRow {
    pub profile_id: ProfileId,
    pub compartment: ProfileCompartment,
    pub privacy_level: PrivacyLevel,
    pub created_at_epoch_s: i64,
}

/// Δ7: versión del esquema que este binario sabe abrir (`PRAGMA user_version`)
pub const SCHEMA_VERSION: i64 = 1;

/// Δ7 (D27): tope de filas de `perf_baseline_samples` en la base persistente
pub const MAX_PERSISTED_PERF_SAMPLES: i64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IpcViolationReason {
    PayloadTooLarge,
    InvalidUrlScheme,
    MalformedMessage,
    UnauthorizedCommand,
    ChannelTimeout,
    /// SEC-AUDIT-01: Credenciales embebidas en URL (user:pass@host)
    CredentialsInUrl,
    /// SEC-AUDIT-01: Host no especificado o vacío
    MalformedHost,
    /// SEC-AUDIT-01: Carácter de control ASCII detectado
    ControlCharacterDetected,
}

impl fmt::Display for IpcViolationReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayloadTooLarge => write!(f, "Payload IPC excede límite máximo permitido (1 MiB)"),
            Self::InvalidUrlScheme => write!(f, "Esquema de URL no autorizado (sólo https, http, about:blank)"),
            Self::MalformedMessage => write!(f, "Estructura de mensaje IPC corrupta o malformada"),
            Self::UnauthorizedCommand => write!(f, "Comando no autorizado desde el proceso de contenido"),
            Self::ChannelTimeout => write!(f, "Tiempo de espera del canal IPC agotado (> 5 000 ms)"),
            Self::CredentialsInUrl => write!(f, "Credenciales embebidas no autorizadas en URL"),
            Self::MalformedHost => write!(f, "Host malformado o vacío en URL"),
            Self::ControlCharacterDetected => write!(f, "Caracteres de control detectados en URL"),
        }
    }
}

impl fmt::Display for GalapagosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IpcBoundaryViolation { tab_id, reason } => {
                write!(f, "Violación IPC en pestaña {:?}: {}", tab_id, reason)
            }
            Self::SandboxEnforcementFailed { win32_error_code, target_level } => {
                write!(f, "Fallo al aplicar sandbox {:?}. Código Win32: {}", target_level, win32_error_code)
            }
            Self::PermissionDenied { profile_id, origin, capability } => {
                write!(f, "Permiso denegado para origen '{}' con capacidad {:?} en perfil {:?}", origin, capability, profile_id)
            }
            Self::IntegrityVerificationFailed { artifact_id } => {
                write!(f, "Fallo en verificación criptográfica del artefacto '{}'", artifact_id)
            }
            Self::CompartmentIsolationBreach { source_profile, target_profile } => {
                write!(f, "Violación de aislamiento entre perfil origen {:?} y destino {:?}", source_profile, target_profile)
            }
            Self::ResourceQuotaExceeded { tab_id, process_id, observed_bytes, limit_bytes } => {
                write!(f, "Cuota de recursos superada en pestaña {:?} (PID {:?}): {} bytes observados (límite: {} bytes)", tab_id, process_id, observed_bytes, limit_bytes)
            }
            Self::StorageFailure { sqlite_extended_code } => {
                write!(f, "Error SQLite de persistencia (código extendido: {})", sqlite_extended_code)
            }
            Self::StorageUnattached => {
                write!(f, "Subsistema de almacenamiento no vinculado")
            }
            Self::IdentifierSpaceExhausted => {
                write!(f, "Espacio de identificadores monótonos agotado")
            }
            Self::PersistenceInvariantViolated { reason } => {
                write!(f, "Invariante de persistencia violado: {}", reason)
            }
        }
    }
}

impl std::error::Error for GalapagosError {}
impl std::error::Error for IpcViolationReason {}
impl std::error::Error for PersistenceViolation {}

// =========================================================================
// DTOs Y ESTADO INMUTABLE DEL DASHBOARD NATIVO (DELTA UI - ZNVE v2.4.2)
// =========================================================================

/// Paleta inmutable del ADN visual de Galapagos Browser (Opción A: Direct2D nativo)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GalapagosTheme {
    pub volcanic_black: [u8; 3],   // #0B121A
    pub deep_ocean_panel: [u8; 3], // #0F1C27
    pub lava_stone_card: [u8; 3],  // #222B34
    pub iguana_fire: [u8; 3],      // #E66A1F
    pub pacific_teal: [u8; 3],     // #1D9BB8
    pub evolution_green: [u8; 3],  // #43A047
}

pub const GALAPAGOS_DARK: GalapagosTheme = GalapagosTheme {
    volcanic_black:   [11, 18, 26],
    deep_ocean_panel: [15, 28, 39],
    lava_stone_card:  [34, 43, 52],
    iguana_fire:      [230, 106, 31],
    pacific_teal:     [29, 155, 184],
    evolution_green:  [67, 160, 71],
};

/// Ventana fija de historial de rendimiento en memoria: 60 muestras (1/s). Cero allocations en caliente.
pub const DASHBOARD_HISTORY_LEN: usize = 60;

/// Estado de telemetría en memoria para alimentar la vista nativa `galapagos://dashboard`
#[derive(Debug, Clone, PartialEq)]
pub struct LiveDashboardState {
    pub active_compartment: ProfileCompartment,
    pub active_privacy: PrivacyLevel,
    pub active_sandbox: WindowsSandboxLevel,
    pub total_blocked_trackers: u64,
    pub active_workers_count: u32,
    pub cpu_history_pct: [f32; DASHBOARD_HISTORY_LEN],
    pub ram_history_pct: [f32; DASHBOARD_HISTORY_LEN],
    pub cursor: usize,
}

impl LiveDashboardState {
    pub fn new(
        compartment: ProfileCompartment,
        privacy: PrivacyLevel,
        sandbox: WindowsSandboxLevel,
    ) -> Self {
        Self {
            active_compartment: compartment,
            active_privacy: privacy,
            active_sandbox: sandbox,
            total_blocked_trackers: 0,
            active_workers_count: 0,
            cpu_history_pct: [0.0; DASHBOARD_HISTORY_LEN],
            ram_history_pct: [0.0; DASHBOARD_HISTORY_LEN],
            cursor: 0,
        }
    }

    /// Ingesta determinista O(1) de una muestra de telemetría sin I/O ni asignaciones en memoria dinámica
    pub fn ingest_sample(&mut self, cpu_delta_us: u64, private_ram_bytes: u64, quota_max_ram: u64, blocked_trackers: u32) {
        let cpu_pct = ((cpu_delta_us as f32 / 1_000_000.0) * 100.0).clamp(0.0, 100.0);
        let ram_pct = if quota_max_ram > 0 {
            ((private_ram_bytes as f32 / quota_max_ram as f32) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        self.cpu_history_pct[self.cursor] = cpu_pct;
        self.ram_history_pct[self.cursor] = ram_pct;
        self.cursor = (self.cursor + 1) % DASHBOARD_HISTORY_LEN;
        self.total_blocked_trackers = self.total_blocked_trackers.saturating_add(blocked_trackers as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_quota_invariants() {
        let valid_quota = ResourceQuota::new(
            NonZeroU64::new(512 * 1024 * 1024).unwrap(),
            NonZeroU16::new(5000).unwrap(),
            NonZeroU32::new(1024 * 1024).unwrap(),
            NonZeroU32::new(5000).unwrap(),
        );
        assert!(valid_quota.is_ok());

        // Exceso de payload IPC (> 1 MiB)
        let invalid_payload = ResourceQuota::new(
            NonZeroU64::new(512 * 1024 * 1024).unwrap(),
            NonZeroU16::new(5000).unwrap(),
            NonZeroU32::new(MAX_ALLOWED_IPC_PAYLOAD_BYTES + 1).unwrap(),
            NonZeroU32::new(5000).unwrap(),
        );
        assert_eq!(
            invalid_payload,
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::PayloadTooLarge,
            })
        );

        // Exceso de timeout IPC (> 5 000 ms)
        let invalid_timeout = ResourceQuota::new(
            NonZeroU64::new(512 * 1024 * 1024).unwrap(),
            NonZeroU16::new(5000).unwrap(),
            NonZeroU32::new(1024).unwrap(),
            NonZeroU32::new(MAX_ALLOWED_IPC_TIMEOUT_MS + 1).unwrap(),
        );
        assert_eq!(
            invalid_timeout,
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::ChannelTimeout,
            })
        );
    }

    #[test]
    fn test_url_scheme_validation() {
        assert!(UiToCoordinatorCmd::validate_url_scheme("about:blank").is_ok());
        assert!(UiToCoordinatorCmd::validate_url_scheme("https://example.com/test").is_ok());
        assert!(UiToCoordinatorCmd::validate_url_scheme("http://localhost:8080").is_ok());

        // Esquemas inseguros o prohibidos
        assert_eq!(
            UiToCoordinatorCmd::validate_url_scheme("file:///C:/Windows/System32/calc.exe"),
            Err(IpcViolationReason::InvalidUrlScheme)
        );
        assert_eq!(
            UiToCoordinatorCmd::validate_url_scheme("javascript:alert(1)"),
            Err(IpcViolationReason::InvalidUrlScheme)
        );
        assert_eq!(
            UiToCoordinatorCmd::validate_url_scheme("chrome://settings"),
            Err(IpcViolationReason::InvalidUrlScheme)
        );

        // Longitud mayor a 2048 bytes
        let long_url = format!("https://example.com/{}", "a".repeat(2040));
        assert_eq!(
            UiToCoordinatorCmd::validate_url_scheme(&long_url),
            Err(IpcViolationReason::PayloadTooLarge)
        );
    }

    #[test]
    fn test_proxy_host_rfc1123_validation() {
        assert!(NetworkRoutePolicy::is_valid_proxy_host("proxy.internal.net"));
        assert!(NetworkRoutePolicy::is_valid_proxy_host("127.0.0.1"));
        assert!(NetworkRoutePolicy::is_valid_proxy_host("localhost"));

        // Host inválidos
        assert!(!NetworkRoutePolicy::is_valid_proxy_host(""));
        assert!(!NetworkRoutePolicy::is_valid_proxy_host("-invalid.host"));
        assert!(!NetworkRoutePolicy::is_valid_proxy_host("invalid-.host"));
        assert!(!NetworkRoutePolicy::is_valid_proxy_host("proxy..internal"));
        assert!(!NetworkRoutePolicy::is_valid_proxy_host(&"a".repeat(254)));
    }

    #[test]
    fn test_error_display_conformance() {
        let err = GalapagosError::IpcBoundaryViolation {
            tab_id: TabId(42),
            reason: IpcViolationReason::PayloadTooLarge,
        };
        let formatted = format!("{}", err);
        assert!(formatted.contains("Violación IPC en pestaña TabId(42)"));
        assert!(formatted.contains("Payload IPC excede"));
    }

    /// Δ7: cada motivo de persistencia tiene un texto propio y la variante lo incluye
    #[test]
    fn test_persistence_violation_display_delta07() {
        let reasons = [
            PersistenceViolation::DataDirUnavailable,
            PersistenceViolation::WalUnavailable,
            PersistenceViolation::SchemaVersionUnsupported { found: 2 },
            PersistenceViolation::ForeignCompartmentRow { profile_id: ProfileId(9) },
            PersistenceViolation::RoutePolicyNotPersistable,
        ];
        let texts: Vec<String> = reasons.iter().map(|r| r.to_string()).collect();
        for (i, a) in texts.iter().enumerate() {
            assert!(!a.is_empty());
            for b in &texts[i + 1..] {
                assert_ne!(a, b, "textos repetidos");
            }
        }
        assert!(texts[2].contains('2') && texts[2].contains(&SCHEMA_VERSION.to_string()));
        assert!(texts[3].contains("ProfileId(9)"));

        let err = GalapagosError::PersistenceInvariantViolated { reason: PersistenceViolation::WalUnavailable };
        assert!(err.to_string().contains(&PersistenceViolation::WalUnavailable.to_string()));
        assert_eq!(SCHEMA_VERSION, 1);
        assert_eq!(MAX_PERSISTED_PERF_SAMPLES, 10_000);
    }

    #[test]
    fn test_strict_url_validation_audit() {
        // Credenciales embebidas prohibidas
        assert_eq!(
            UiToCoordinatorCmd::validate_url_strict("https://user:pass@banking.com"),
            Err(IpcViolationReason::CredentialsInUrl)
        );

        // Host vacío prohibido
        assert_eq!(
            UiToCoordinatorCmd::validate_url_strict("https://"),
            Err(IpcViolationReason::MalformedHost)
        );
        assert_eq!(
            UiToCoordinatorCmd::validate_url_strict("https:///path/only"),
            Err(IpcViolationReason::MalformedHost)
        );

        // Caracteres de control ASCII prohibidos
        assert_eq!(
            UiToCoordinatorCmd::validate_url_strict("https://example.com\r\n"),
            Err(IpcViolationReason::ControlCharacterDetected)
        );
        assert_eq!(
            UiToCoordinatorCmd::validate_url_strict("https://example.com\0"),
            Err(IpcViolationReason::ControlCharacterDetected)
        );
        assert_eq!(
            UiToCoordinatorCmd::validate_url_strict("https://example .com"),
            Err(IpcViolationReason::ControlCharacterDetected)
        );
    }

    #[test]
    fn test_worker_handshake_and_engine_backend_contract() {
        let pid = ProcessId(std::num::NonZeroU32::new(1234).unwrap());
        let handshake = WorkerHandshake {
            process_id: pid,
            applied_sandbox: WindowsSandboxLevel::AppContainerStrict,
            protocol_version: std::num::NonZeroU16::new(1).unwrap(),
        };
        assert_eq!(handshake.process_id, pid);
        assert_eq!(handshake.applied_sandbox, WindowsSandboxLevel::AppContainerStrict);
        assert_eq!(handshake.protocol_version.get(), 1);

        assert_eq!(EngineBackend::Real, EngineBackend::Real);
        assert_ne!(EngineBackend::Real, EngineBackend::Stub);
    }

    fn norm(raw: &str) -> Result<String, IpcViolationReason> {
        normalize_address_input(&AddressBarInput { raw: raw.to_string() })
    }

    #[test]
    fn test_normalize_address_input_delta02() {
        assert_eq!(norm("servo.org"), Ok("https://servo.org".to_string()));
        assert_eq!(norm("  servo.org/a?b=1  "), Ok("https://servo.org/a?b=1".to_string()));
        assert_eq!(norm("localhost:8080"), Ok("https://localhost:8080".to_string()));
        assert_eq!(norm("http://example.com"), Ok("http://example.com".to_string()));
        assert_eq!(norm("about:blank"), Ok("about:blank".to_string()));

        assert_eq!(norm(""), Err(IpcViolationReason::MalformedMessage));
        assert_eq!(norm("   "), Err(IpcViolationReason::MalformedMessage));
        assert_eq!(norm("javascript:alert(1)"), Err(IpcViolationReason::InvalidUrlScheme));
        assert_eq!(norm("data:text/html,x"), Err(IpcViolationReason::InvalidUrlScheme));
        assert_eq!(norm("file:///C:/x"), Err(IpcViolationReason::InvalidUrlScheme));
        assert_eq!(norm("user@host.com"), Err(IpcViolationReason::CredentialsInUrl));
        assert_eq!(norm("exa mple.com"), Err(IpcViolationReason::ControlCharacterDetected));
        assert_eq!(
            norm(&"a".repeat(MAX_ALLOWED_URL_LEN + 1)),
            Err(IpcViolationReason::PayloadTooLarge)
        );
        // Con prefijo https:// supera el límite aunque el texto crudo no
        assert_eq!(
            norm(&"a".repeat(MAX_ALLOWED_URL_LEN - 2)),
            Err(IpcViolationReason::PayloadTooLarge)
        );
    }
}

