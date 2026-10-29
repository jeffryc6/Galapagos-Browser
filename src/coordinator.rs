//! src/coordinator.rs
//! Coordinador de Confianza para Galapagos Browser v0.1.
//! Plataforma: Windows Desktop (Win32/Rust).
//! Arquitectura: Control centralizado de perfiles, pestañas y procesos aislados.

#![forbid(unsafe_code)]

use crate::defense::{
    BehaviorSentinel, DefenseAction, DefenseContext, DefenseDecision, DefenseEvaluation, RequestGuard,
    ThreatFeedUpdateSummary, ThreatIntelFeedUpdater,
};
use crate::galapagos_core_v1::*;
use crate::islands::connectivity::ConnectivityIsland;
use crate::islands::freedom::FreedomIsland;
use crate::islands::performance::PerformanceIsland;
use crate::islands::security::SecurityIsland;
use crate::storage::StorageEngine;

use std::collections::HashMap;
use std::num::NonZeroU32;

/// Estado inmutable en memoria de una pestaña activa
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabState {
    pub profile_id: ProfileId,
    pub current_url: String,
    pub quota: ResourceQuota,
    pub process_id: ProcessId,
}

/// Estado de configuración de un perfil compartimentado
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileState {
    pub compartment: ProfileCompartment,
    pub privacy_level: PrivacyLevel,
    pub route_policy: NetworkRoutePolicy,
}

fn denied(profile_id: ProfileId, origin: String) -> GalapagosError {
    GalapagosError::PermissionDenied {
        profile_id,
        origin,
        capability: SiteCapability::LocalStorage,
    }
}

/// DELTA_03 P-I3: único punto donde `user_confirmed` puede cambiar un veredicto
fn may_proceed(decision: DefenseDecision, user_confirmed: bool) -> bool {
    match decision {
        DefenseDecision::Allow | DefenseDecision::Restrict => true,
        DefenseDecision::RequireConfirmation => user_confirmed,
        DefenseDecision::Block | DefenseDecision::Unavailable => false,
    }
}

/// DELTA_07 W-I6: segundos desde la época Unix para `created_at_epoch_s` / `updated_at_epoch_s`
fn now_epoch_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Entrega el siguiente identificador monótono; nunca reutiliza ni desborda (SEC-AUDIT-03)
fn next_id(counter: &mut u64) -> Result<u64, GalapagosError> {
    if *counter == u64::MAX {
        return Err(GalapagosError::IdentifierSpaceExhausted);
    }
    let id = *counter;
    *counter += 1;
    Ok(id)
}

/// Orquestador del Coordinador de Confianza
pub struct Coordinator {
    profiles: HashMap<ProfileId, ProfileState>,
    tabs: HashMap<TabId, TabState>,
    next_profile_id: u64,
    next_tab_id: u64,
    next_process_counter: u32,
    /// Base del compartimento Personal (WAL en disco en producción)
    storage: Option<StorageEngine>,
    /// DELTA_07 W-I1: una base `:memory:` por perfil efímero; se cierra al destruir el perfil
    ephemeral_storage: HashMap<ProfileId, StorageEngine>,
    defense: RequestGuard,
    sentinel: BehaviorSentinel,
    threat_intel: ThreatIntelFeedUpdater,
    security: SecurityIsland,
    performance: PerformanceIsland,
    freedom: FreedomIsland,
    connectivity: ConnectivityIsland,
}

impl Default for Coordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl Coordinator {
    /// Inicializa una instancia limpia del coordinador
    pub fn new() -> Self {
        Self {
            profiles: HashMap::new(),
            tabs: HashMap::new(),
            next_profile_id: 1,
            next_tab_id: 1,
            next_process_counter: 1,
            storage: None,
            ephemeral_storage: HashMap::new(),
            defense: RequestGuard::new(),
            sentinel: BehaviorSentinel::new(),
            threat_intel: ThreatIntelFeedUpdater::with_default_master_key(),
            security: SecurityIsland::new(),
            performance: PerformanceIsland::new(),
            freedom: FreedomIsland::new(),
            connectivity: ConnectivityIsland::new(),
        }
    }

    /// Vincula la base del compartimento Personal (Fase D / SEC-02; DELTA_07 W-I7)
    pub fn attach_storage(&mut self, storage: StorageEngine) {
        self.storage = Some(storage);
    }

    /// Retorna la base del compartimento Personal si está vinculada
    pub fn storage(&self) -> Option<&StorageEngine> {
        self.storage.as_ref()
    }

    /// DELTA_07 W-I1: base que corresponde a un perfil. Personal → base persistente;
    /// efímero → su base `:memory:`. Sin base persistente adjunta, un perfil Personal (o uno
    /// desconocido) da `StorageUnattached`; con ella, uno desconocido da `PermissionDenied`.
    pub fn storage_for_profile(&self, profile_id: ProfileId, origin: &str) -> Result<&StorageEngine, GalapagosError> {
        match self.profiles.get(&profile_id).map(|p| p.compartment) {
            Some(ProfileCompartment::Personal) => self.storage.as_ref().ok_or(GalapagosError::StorageUnattached),
            Some(_) => self.ephemeral_storage.get(&profile_id).ok_or(GalapagosError::StorageUnattached),
            None if self.storage.is_none() => Err(GalapagosError::StorageUnattached),
            None => Err(denied(profile_id, origin.to_string())),
        }
    }

    /// D27: base donde se guardan las muestras de rendimiento de una pestaña (la de su perfil)
    pub fn storage_for_tab(&self, tab_id: TabId) -> Result<&StorageEngine, GalapagosError> {
        let tab = self.tabs.get(&tab_id).ok_or(GalapagosError::IpcBoundaryViolation {
            tab_id,
            reason: IpcViolationReason::MalformedMessage,
        })?;
        self.storage_for_profile(tab.profile_id, "coordinator")
    }

    /// DELTA_07 W-I2: registra los perfiles de la base persistente y avanza el contador para no
    /// reutilizar sus ids. Ruta = `DirectSystem` (el esquema v1 no la guarda, D26). Una fila de un
    /// compartimento no persistente invalida la base entera: no se carga nada.
    pub fn load_persisted_profiles(&mut self) -> Result<usize, GalapagosError> {
        let rows = self.storage.as_ref().ok_or(GalapagosError::StorageUnattached)?.list_profiles()?;
        if let Some(row) = rows.iter().find(|r| r.compartment != ProfileCompartment::Personal) {
            return Err(GalapagosError::PersistenceInvariantViolated {
                reason: PersistenceViolation::ForeignCompartmentRow { profile_id: row.profile_id },
            });
        }
        if let Some(row) = rows.iter().find(|r| self.profiles.contains_key(&r.profile_id)) {
            return Err(GalapagosError::CompartmentIsolationBreach {
                source_profile: row.profile_id,
                target_profile: row.profile_id,
            });
        }
        if let Some(max_id) = rows.iter().map(|r| r.profile_id.0).max() {
            let next = max_id.checked_add(1).ok_or(GalapagosError::IdentifierSpaceExhausted)?;
            self.next_profile_id = self.next_profile_id.max(next);
        }
        for row in &rows {
            self.profiles.insert(
                row.profile_id,
                ProfileState {
                    compartment: row.compartment,
                    privacy_level: row.privacy_level,
                    route_policy: NetworkRoutePolicy::DirectSystem,
                },
            );
        }
        Ok(rows.len())
    }

    /// DELTA_07 W-I2: perfil de menor id de un compartimento (el que `main` reutiliza al arrancar)
    pub fn lowest_profile_id(&self, compartment: ProfileCompartment) -> Option<ProfileId> {
        self.profiles
            .iter()
            .filter(|(_, p)| p.compartment == compartment)
            .map(|(&id, _)| id)
            .min_by_key(|id| id.0)
    }

    /// Retorna una referencia al guardián defensivo de Ocean Defense
    pub fn defense(&self) -> &RequestGuard {
        &self.defense
    }

    /// Retorna una referencia mutable al guardián defensivo de Ocean Defense
    pub fn defense_mut(&mut self) -> &mut RequestGuard {
        &mut self.defense
    }

    /// Retorna una referencia al centinela de comportamiento de Ocean Defense
    pub fn sentinel(&self) -> &BehaviorSentinel {
        &self.sentinel
    }

    /// Retorna una referencia mutable al centinela de comportamiento de Ocean Defense
    pub fn sentinel_mut(&mut self) -> &mut BehaviorSentinel {
        &mut self.sentinel
    }

    /// Retorna una referencia al actualizador de inteligencia de amenazas (Threat Intel)
    pub fn threat_intel(&self) -> &ThreatIntelFeedUpdater {
        &self.threat_intel
    }

    /// Aplica una lista autenticada de amenazas mediante verificación Ed25519 y SHA-256 (SEC-03)
    pub fn apply_signed_threat_feed(
        &mut self,
        manifest: &SignedArtifactManifest,
        feed_bytes: &[u8],
    ) -> Result<ThreatFeedUpdateSummary, GalapagosError> {
        self.threat_intel.verify_and_apply_manifest(
            manifest,
            feed_bytes,
            &mut self.defense,
            self.storage.as_ref(),
        )
    }

    /// Retorna una referencia a la Isla de Seguridad (Security Island)
    pub fn security(&self) -> &SecurityIsland {
        &self.security
    }

    /// Retorna una referencia mutable a la Isla de Seguridad (Security Island)
    pub fn security_mut(&mut self) -> &mut SecurityIsland {
        &mut self.security
    }

    /// Retorna una referencia mutable a la Isla de Rendimiento (Performance Island)
    pub fn performance_mut(&mut self) -> &mut PerformanceIsland {
        &mut self.performance
    }

    /// Retorna una referencia a la Isla de Libertad (Freedom Island)
    pub fn freedom(&self) -> &FreedomIsland {
        &self.freedom
    }

    /// Retorna una referencia a la Isla de Conectividad (Connectivity Island)
    pub fn connectivity(&self) -> &ConnectivityIsland {
        &self.connectivity
    }

    /// URL vigente de una pestaña (origen que usa Ocean Defense al evaluar la siguiente navegación)
    pub fn current_url(&self, tab_id: TabId) -> Option<&str> {
        self.tabs.get(&tab_id).map(|t| t.current_url.as_str())
    }

    /// Retorna el número de perfiles activos
    pub fn profile_count(&self) -> usize {
        self.profiles.len()
    }

    /// Retorna el número de pestañas activas
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Registra un nuevo perfil compartimentado (PRIV-01)
    pub fn create_profile(
        &mut self,
        profile_id: ProfileId,
        compartment: ProfileCompartment,
        privacy_level: PrivacyLevel,
        route_policy: NetworkRoutePolicy,
    ) -> Result<(), GalapagosError> {
        if self.profiles.contains_key(&profile_id) {
            return Err(GalapagosError::CompartmentIsolationBreach {
                source_profile: profile_id,
                target_profile: profile_id,
            });
        }

        // Validar la política de ruta antes de persistir nada (antes un proxy inválido dejaba la fila en SQLite)
        self.connectivity.validate_route(&route_policy)?;

        // DELTA_07 W-I1: Personal va a la base persistente (si está adjunta); cada perfil efímero,
        // a su propia base `:memory:`. Ninguna fila efímera toca la base en disco.
        if compartment == ProfileCompartment::Personal {
            if let Some(ref storage) = self.storage {
                // W-I5 (D26): el esquema v1 no guarda la ruta; no se persiste a medias
                if matches!(route_policy, NetworkRoutePolicy::CustomProxy { .. }) {
                    return Err(GalapagosError::PersistenceInvariantViolated {
                        reason: PersistenceViolation::RoutePolicyNotPersistable,
                    });
                }
                storage.insert_profile(profile_id, compartment, privacy_level, now_epoch_s())?;
            }
        } else {
            let ephemeral = StorageEngine::open_in_memory()?;
            ephemeral.insert_profile(profile_id, compartment, privacy_level, now_epoch_s())?;
            self.ephemeral_storage.insert(profile_id, ephemeral);
        }

        self.profiles.insert(
            profile_id,
            ProfileState {
                compartment,
                privacy_level,
                route_policy,
            },
        );
        Ok(())
    }

    /// Instancia una nueva pestaña asociada a un perfil con cuota de recursos (ARC-02 / SEC-01)
    pub fn spawn_tab(
        &mut self,
        tab_id: TabId,
        profile_id: ProfileId,
        initial_url: String,
        quota: ResourceQuota,
    ) -> Result<ProcessId, GalapagosError> {
        // 1. Verificar existencia del perfil
        if !self.profiles.contains_key(&profile_id) {
            return Err(denied(profile_id, initial_url));
        }

        // 1b. Un TabId vivo no se reutiliza: sobrescribirlo reemplazaría su enjaulamiento y su estado
        if self.tabs.contains_key(&tab_id) {
            return Err(GalapagosError::IpcBoundaryViolation {
                tab_id,
                reason: IpcViolationReason::MalformedMessage,
            });
        }

        // 2. Validar esquema seguro de URL
        if let Err(violation) = UiToCoordinatorCmd::validate_url_scheme(&initial_url) {
            return Err(GalapagosError::IpcBoundaryViolation {
                tab_id,
                reason: violation,
            });
        }

        // 3. Generar ProcessId estrictamente no-cero y monótono
        let pid_raw = self.next_process_counter;
        if pid_raw == u32::MAX {
            return Err(GalapagosError::ResourceQuotaExceeded {
                tab_id,
                process_id: ProcessId(NonZeroU32::new(1).unwrap()),
                observed_bytes: 0,
                limit_bytes: 0,
            });
        }
        let process_id = ProcessId(NonZeroU32::new(pid_raw).expect("pid_raw es >= 1"));

        // 4. T-06 (INC-20261025-CORE-DEBLOAT): el Job Object tiene un solo dueño, el motor
        //    (`ServoEngineAdapter::initialize_sandboxed_worker`), que es quien tiene el proceso y lo
        //    asigna; sin Job el worker no arranca. Antes aquí se creaba un segundo Job con el mismo
        //    nombre que nunca recibía ningún proceso.
        self.next_process_counter += 1;

        // 5. Registrar estado de la pestaña
        self.tabs.insert(
            tab_id,
            TabState {
                profile_id,
                current_url: initial_url,
                quota,
                process_id,
            },
        );

        Ok(process_id)
    }

    /// R-13 (INC-20261025-CORE-DEBLOAT): veredicto de Ocean Defense para navegar `tab_id` a
    /// `target_url` con el perfil real de la pestaña (nivel de privacidad) y su URL vigente como
    /// origen. No cambia estado: la UI lo usa para decidir; `navigate_confirmed` reevalúa al aplicar.
    pub fn evaluate_navigation(&self, tab_id: TabId, target_url: &str) -> Result<DefenseEvaluation, GalapagosError> {
        let tab = self.tabs.get(&tab_id).ok_or(GalapagosError::IpcBoundaryViolation {
            tab_id,
            reason: IpcViolationReason::MalformedMessage,
        })?;
        let profile = self
            .profiles
            .get(&tab.profile_id)
            .ok_or_else(|| denied(tab.profile_id, target_url.to_string()))?;
        let ctx = DefenseContext {
            profile_id: tab.profile_id,
            privacy_level: profile.privacy_level,
            origin_host: tab.current_url.clone(),
        };
        Ok(self
            .defense
            .evaluate(&ctx, &DefenseAction::Navigate { target_url: target_url.to_string() }))
    }

    /// Despacha una navegación validando el esquema y evaluando políticas de Ocean Defense (ARC-02 / SEC-01)
    pub fn navigate(&mut self, tab_id: TabId, target_url: String) -> Result<(), GalapagosError> {
        self.navigate_confirmed(tab_id, target_url, false)
    }

    /// DELTA_03 (P-I2/P-I3): `user_confirmed` solo permite continuar cuando Ocean Defense pide
    /// confirmación. Nunca anula `Block`, `Unavailable` ni el veredicto del Behavior Sentinel.
    pub fn navigate_confirmed(
        &mut self,
        tab_id: TabId,
        target_url: String,
        user_confirmed: bool,
    ) -> Result<(), GalapagosError> {
        let profile_id = self
            .tabs
            .get(&tab_id)
            .ok_or(GalapagosError::IpcBoundaryViolation {
                tab_id,
                reason: IpcViolationReason::MalformedMessage,
            })?
            .profile_id;
        if !self.profiles.contains_key(&profile_id) {
            return Err(denied(profile_id, target_url));
        }

        // Evaluación de anomalías de navegación en memoria (Behavior Sentinel)
        if let Some(DefenseDecision::Block) = self.sentinel.active_verdict_for_tab(tab_id) {
            return Err(denied(profile_id, target_url));
        }

        if let Some(alert) = self.sentinel.record_navigation_hop(tab_id, &target_url) {
            if alert.action == DefenseDecision::Block {
                return Err(denied(profile_id, target_url));
            }
        }

        // Evaluación de Ocean Defense (Request Guard), con el mismo contexto que ve la UI
        let eval = self.evaluate_navigation(tab_id, &target_url)?;

        if may_proceed(eval.decision, user_confirmed) {
            if let Some(tab) = self.tabs.get_mut(&tab_id) {
                tab.current_url = target_url.clone();
            }
            // Navegación completada con éxito: reiniciar contadores de salto
            self.sentinel.reset_navigation_hops(tab_id, &target_url);
            Ok(())
        } else if eval.decision == DefenseDecision::Unavailable {
            Err(GalapagosError::IntegrityVerificationFailed { artifact_id: target_url })
        } else {
            Err(denied(profile_id, target_url))
        }
    }

    /// Termina un proceso de pestaña y libera sus recursos asociados
    pub fn terminate_tab(
        &mut self,
        tab_id: TabId,
        _reason: TerminationReason,
    ) -> Result<ProcessId, GalapagosError> {
        let removed = self.tabs.remove(&tab_id).ok_or(GalapagosError::IpcBoundaryViolation {
            tab_id,
            reason: IpcViolationReason::MalformedMessage,
        })?;

        // Desecho explícito del historial de muestreo en Performance Island
        self.performance.remove_tab(tab_id);

        // Desecho explícito del estado del centinela de comportamiento
        self.sentinel.on_tab_closed(tab_id);

        Ok(removed.process_id)
    }

    /// Destruye un perfil efímero y purga todas sus pestañas activas (PRIV-01 / PRIV-02)
    pub fn destroy_ephemeral_profile(&mut self, profile_id: ProfileId) -> Result<usize, GalapagosError> {
        let profile = self.profiles.get(&profile_id).ok_or(GalapagosError::PermissionDenied {
            profile_id,
            origin: "coordinator".to_string(),
            capability: SiteCapability::LocalStorage,
        })?;

        // Sólo perfiles no persistentes pueden ser destruidos con purga total inmediata
        if profile.compartment == ProfileCompartment::Personal {
            return Err(GalapagosError::CompartmentIsolationBreach {
                source_profile: profile_id,
                target_profile: profile_id,
            });
        }

        // Purgar pestañas asociadas al perfil
        let tabs_to_remove: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|(_, tab)| tab.profile_id == profile_id)
            .map(|(&id, _)| id)
            .collect();

        let count = tabs_to_remove.len();
        for tid in tabs_to_remove {
            self.terminate_tab(tid, TerminationReason::UserClosed)?;
        }

        // W-I1: cerrar la conexión libera la base `:memory:` del perfil (PRIV-02, borrado verificable, diferido)
        drop(self.ephemeral_storage.remove(&profile_id));
        self.profiles.remove(&profile_id);
        Ok(count)
    }

    /// Despachador atómico del enum UiToCoordinatorCmd
    pub fn dispatch_ui_cmd(&mut self, cmd: UiToCoordinatorCmd) -> Result<(), GalapagosError> {
        match cmd {
            UiToCoordinatorCmd::CreateProfile {
                compartment,
                privacy_level,
                route_policy,
            } => {
                let pid = ProfileId(next_id(&mut self.next_profile_id)?);
                self.create_profile(pid, compartment, privacy_level, route_policy)
            }
            UiToCoordinatorCmd::SpawnTab {
                profile_id,
                initial_url,
                quota,
            } => {
                let tid = TabId(next_id(&mut self.next_tab_id)?);
                self.spawn_tab(tid, profile_id, initial_url, quota).map(|_| ())
            }
            UiToCoordinatorCmd::Navigate { tab_id, target_url } => {
                self.navigate(tab_id, target_url)
            }
            UiToCoordinatorCmd::SetSitePermission {
                profile_id,
                origin,
                capability,
                grant,
            } => {
                // DELTA_07 W-I1/W-I7: cada perfil escribe en la base de su compartimento. Sin base
                // persistente, un perfil Personal o desconocido falla con StorageUnattached.
                let storage = match self.storage_for_profile(profile_id, &origin) {
                    Err(GalapagosError::PermissionDenied { .. }) => {
                        return Err(GalapagosError::PermissionDenied { profile_id, origin, capability })
                    }
                    other => other?,
                };
                storage.set_site_permission(profile_id, &origin, capability, grant, now_epoch_s())
            }
            UiToCoordinatorCmd::TerminateTab { tab_id, reason } => {
                self.terminate_tab(tab_id, reason).map(|_| ())
            }
            UiToCoordinatorCmd::DestroyEphemeralProfile { profile_id } => {
                self.destroy_ephemeral_profile(profile_id).map(|_| ())
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::num::{NonZeroU16, NonZeroU64};

    fn dummy_quota() -> ResourceQuota {
        ResourceQuota::new(
            NonZeroU64::new(512 * 1024 * 1024).unwrap(),
            NonZeroU16::new(5000).unwrap(),
            NonZeroU32::new(1024 * 1024).unwrap(),
            NonZeroU32::new(5000).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn test_coordinator_lifecycle() {
        let mut coordinator = Coordinator::new();
        let profile_id = ProfileId(1);
        let tab_id = TabId(1);

        // Crear perfil Personal
        assert!(coordinator
            .create_profile(
                profile_id,
                ProfileCompartment::Personal,
                PrivacyLevel::Daily,
                NetworkRoutePolicy::DirectSystem
            )
            .is_ok());

        assert_eq!(coordinator.profile_count(), 1);

        // Abrir pestaña inicial válida
        let spawn_res = coordinator.spawn_tab(
            tab_id,
            profile_id,
            "about:blank".to_string(),
            dummy_quota(),
        );
        assert!(spawn_res.is_ok());
        assert_eq!(coordinator.tab_count(), 1);

        // Navegar a URL HTTPS autorizada
        assert!(coordinator
            .navigate(tab_id, "https://servo.org".to_string())
            .is_ok());

        // Terminar pestaña
        assert!(coordinator
            .terminate_tab(tab_id, TerminationReason::UserClosed)
            .is_ok());
        assert_eq!(coordinator.tab_count(), 0);
    }

    /// R8: una pestaña duplicada se rechaza (antes sobrescribía enjaulamiento y estado) sin consumir
    /// ProcessId ni alterar la pestaña existente.
    #[test]
    fn test_spawn_tab_rejects_duplicate_tab_without_side_effects() {
        let mut coordinator = Coordinator::new();
        let (profile_id, tab_id) = (ProfileId(1), TabId(1));
        coordinator
            .create_profile(profile_id, ProfileCompartment::Personal, PrivacyLevel::Daily, NetworkRoutePolicy::DirectSystem)
            .unwrap();
        let first = coordinator
            .spawn_tab(tab_id, profile_id, "about:blank".to_string(), dummy_quota())
            .unwrap();

        let dup = coordinator.spawn_tab(tab_id, profile_id, "about:blank".to_string(), dummy_quota());
        assert!(dup.is_err(), "la pestaña duplicada debe rechazarse, no ignorarse");
        assert_eq!(coordinator.tab_count(), 1);

        // El siguiente ProcessId es el inmediato al primero: el intento fallido no consumió uno
        let next = coordinator
            .spawn_tab(TabId(2), profile_id, "about:blank".to_string(), dummy_quota())
            .unwrap();
        assert_eq!(next.0.get(), first.0.get() + 1);
    }

    /// DELTA_03 P-I3: la confirmación solo cambia RequireConfirmation
    #[test]
    fn test_may_proceed_table_delta03() {
        for confirmed in [false, true] {
            assert!(may_proceed(DefenseDecision::Allow, confirmed));
            assert!(may_proceed(DefenseDecision::Restrict, confirmed));
            assert!(!may_proceed(DefenseDecision::Block, confirmed));
            assert!(!may_proceed(DefenseDecision::Unavailable, confirmed));
        }
        assert!(!may_proceed(DefenseDecision::RequireConfirmation, false));
        assert!(may_proceed(DefenseDecision::RequireConfirmation, true));
    }

    /// DELTA_03 P-I3: ni Ocean Defense `Block` ni el Behavior Sentinel ceden ante `user_confirmed`
    #[test]
    fn test_confirmation_never_overrides_block_or_sentinel_delta03() {
        let mut coordinator = Coordinator::new();
        coordinator.defense_mut().block_host("phishing.evil.com".to_string());
        let (profile_id, tab_id) = (ProfileId(1), TabId(1));
        coordinator
            .create_profile(profile_id, ProfileCompartment::Personal, PrivacyLevel::Daily, NetworkRoutePolicy::DirectSystem)
            .unwrap();
        coordinator
            .spawn_tab(tab_id, profile_id, "about:blank".to_string(), dummy_quota())
            .unwrap();

        assert!(coordinator.navigate_confirmed(tab_id, "https://servo.org".to_string(), true).is_ok());
        assert!(coordinator
            .navigate_confirmed(tab_id, "https://phishing.evil.com/login".to_string(), true)
            .is_err());

        for i in 1..=10 {
            coordinator.sentinel_mut().record_navigation_hop(tab_id, &format!("https://redirect{}.org", i));
        }
        assert!(coordinator
            .navigate_confirmed(tab_id, "https://redirect11.org".to_string(), true)
            .is_err());
    }

    #[test]
    fn test_ephemeral_profile_destruction() {
        let mut coordinator = Coordinator::new();
        let ephemeral_pid = ProfileId(99);

        assert!(coordinator
            .create_profile(
                ephemeral_pid,
                ProfileCompartment::TemporaryPrivate,
                PrivacyLevel::Ephemeral,
                NetworkRoutePolicy::DirectSystem
            )
            .is_ok());

        // Spawneamos 2 pestañas en el perfil efímero
        let _ = coordinator.spawn_tab(TabId(10), ephemeral_pid, "about:blank".to_string(), dummy_quota());
        let _ = coordinator.spawn_tab(TabId(11), ephemeral_pid, "about:blank".to_string(), dummy_quota());
        assert_eq!(coordinator.tab_count(), 2);

        // Destruir perfil efímero y validar que purga todas sus pestañas
        let destroyed = coordinator.destroy_ephemeral_profile(ephemeral_pid);
        assert_eq!(destroyed, Ok(2));
        assert_eq!(coordinator.tab_count(), 0);
        assert_eq!(coordinator.profile_count(), 0);
    }

    #[test]
    fn test_monotonic_id_allocation_audit() {
        let mut coordinator = Coordinator::new();
        // Crear perfil efímero y destruirlo
        assert!(coordinator.dispatch_ui_cmd(UiToCoordinatorCmd::CreateProfile {
            compartment: ProfileCompartment::TemporaryPrivate,
            privacy_level: PrivacyLevel::Ephemeral,
            route_policy: NetworkRoutePolicy::DirectSystem,
        }).is_ok());

        assert_eq!(coordinator.profile_count(), 1);
        assert!(coordinator.destroy_ephemeral_profile(ProfileId(1)).is_ok());
        assert_eq!(coordinator.profile_count(), 0);

        // Crear nuevo perfil y asegurar que NO reutiliza ProfileId(1)
        assert!(coordinator.dispatch_ui_cmd(UiToCoordinatorCmd::CreateProfile {
            compartment: ProfileCompartment::TemporaryPrivate,
            privacy_level: PrivacyLevel::Ephemeral,
            route_policy: NetworkRoutePolicy::DirectSystem,
        }).is_ok());

        // Debe haber obtenido ProfileId(2) y no 1
        assert!(coordinator.profiles.contains_key(&ProfileId(2)));
        assert!(!coordinator.profiles.contains_key(&ProfileId(1)));
    }

    #[test]
    fn test_zero_silent_suppression_audit() {
        let mut coordinator = Coordinator::new();
        let res = coordinator.dispatch_ui_cmd(UiToCoordinatorCmd::SetSitePermission {
            profile_id: ProfileId(1),
            origin: "https://example.com".to_string(),
            capability: SiteCapability::Notifications,
            grant: PermissionGrant::Deny,
        });

        // Debe retornar StorageUnattached explícito en vez de Ok(())
        assert_eq!(res, Err(GalapagosError::StorageUnattached));
    }

    /// INC-20261025-CORE-DEBLOAT R-06: un proxy inválido se rechaza antes de escribir en SQLite;
    /// el mismo ProfileId puede crearse después con una ruta válida (antes quedaba la fila huérfana)
    #[test]
    fn test_invalid_route_leaves_no_persisted_profile_inc20261025() {
        let mut coordinator = Coordinator::new();
        coordinator.attach_storage(StorageEngine::open_in_memory().expect("open storage"));
        let invalid = NetworkRoutePolicy::CustomProxy {
            host: "-invalid.proxy.lan".to_string(),
            port: std::num::NonZeroU16::new(8080).unwrap(),
            enforce_remote_dns: true,
            block_webrtc_leaks: true,
        };
        assert!(coordinator
            .create_profile(ProfileId(7), ProfileCompartment::Personal, PrivacyLevel::Daily, invalid)
            .is_err());
        assert_eq!(coordinator.profile_count(), 0);
        assert_eq!(
            coordinator.create_profile(
                ProfileId(7),
                ProfileCompartment::Personal,
                PrivacyLevel::Daily,
                NetworkRoutePolicy::DirectSystem
            ),
            Ok(())
        );
    }

    fn with_persistent_storage() -> Coordinator {
        let mut coordinator = Coordinator::new();
        coordinator.attach_storage(StorageEngine::open_in_memory().expect("base persistente de prueba"));
        coordinator
    }

    /// DELTA_07 W-I1 (PRIV-01): el perfil efímero y sus permisos viven en su propia base;
    /// la base persistente no recibe ninguna fila suya, y la efímera se cierra al destruirlo
    #[test]
    fn test_ephemeral_profile_never_touches_persistent_storage_delta07() {
        let mut coordinator = with_persistent_storage();
        coordinator
            .dispatch_ui_cmd(UiToCoordinatorCmd::CreateProfile {
                compartment: ProfileCompartment::TemporaryPrivate,
                privacy_level: PrivacyLevel::Ephemeral,
                route_policy: NetworkRoutePolicy::DirectSystem,
            })
            .unwrap();
        let pid = ProfileId(1);
        coordinator
            .dispatch_ui_cmd(UiToCoordinatorCmd::SetSitePermission {
                profile_id: pid,
                origin: "https://example.com".to_string(),
                capability: SiteCapability::Notifications,
                grant: PermissionGrant::AllowSessionOnly,
            })
            .unwrap();

        let persistent = coordinator.storage().unwrap();
        assert!(persistent.list_profiles().unwrap().is_empty());
        assert_eq!(
            persistent.get_site_permission(pid, "https://example.com", SiteCapability::Notifications),
            Ok(None)
        );
        assert_eq!(
            coordinator
                .storage_for_profile(pid, "https://example.com")
                .unwrap()
                .get_site_permission(pid, "https://example.com", SiteCapability::Notifications),
            Ok(Some(PermissionGrant::AllowSessionOnly))
        );

        coordinator.destroy_ephemeral_profile(pid).unwrap();
        assert!(coordinator.storage_for_profile(pid, "https://example.com").is_err());
        assert!(coordinator.ephemeral_storage.is_empty());
    }

    /// W-I2/W-I4: un segundo arranque carga los perfiles guardados, reutiliza el de menor id y no
    /// reasigna sus ids; las fechas se guardan con la hora real (W-I6)
    #[test]
    fn test_load_persisted_profiles_and_id_continuity_delta07() {
        let storage = StorageEngine::open_in_memory().unwrap();
        storage.insert_profile(ProfileId(1), ProfileCompartment::Personal, PrivacyLevel::Daily, 100).unwrap();
        storage.insert_profile(ProfileId(5), ProfileCompartment::Personal, PrivacyLevel::Hardened, 200).unwrap();

        let mut coordinator = Coordinator::new();
        coordinator.attach_storage(storage);
        assert_eq!(coordinator.load_persisted_profiles(), Ok(2));
        assert_eq!(coordinator.lowest_profile_id(ProfileCompartment::Personal), Some(ProfileId(1)));
        assert_eq!(coordinator.lowest_profile_id(ProfileCompartment::Laboratory), None);

        // El perfil Hardened conserva su nivel de privacidad tras la carga
        coordinator
            .spawn_tab(TabId(1), ProfileId(5), "about:blank".to_string(), dummy_quota())
            .unwrap();
        assert_eq!(
            coordinator.evaluate_navigation(TabId(1), "http://insecure-news.org").unwrap().decision,
            DefenseDecision::Restrict
        );

        // El siguiente perfil recibe el id 6 y queda guardado con fecha real
        coordinator
            .dispatch_ui_cmd(UiToCoordinatorCmd::CreateProfile {
                compartment: ProfileCompartment::Personal,
                privacy_level: PrivacyLevel::Daily,
                route_policy: NetworkRoutePolicy::DirectSystem,
            })
            .unwrap();
        let rows = coordinator.storage().unwrap().list_profiles().unwrap();
        assert_eq!(rows.last().map(|r| r.profile_id), Some(ProfileId(6)));
        assert!(rows.last().unwrap().created_at_epoch_s > 1_700_000_000);
    }

    /// W-I2: una fila de un compartimento efímero en la base persistente invalida la carga entera
    #[test]
    fn test_foreign_compartment_row_rejects_load_delta07() {
        let storage = StorageEngine::open_in_memory().unwrap();
        storage.insert_profile(ProfileId(1), ProfileCompartment::Personal, PrivacyLevel::Daily, 1).unwrap();
        storage.insert_profile(ProfileId(2), ProfileCompartment::TemporaryPrivate, PrivacyLevel::Ephemeral, 1).unwrap();

        let mut coordinator = Coordinator::new();
        coordinator.attach_storage(storage);
        assert_eq!(
            coordinator.load_persisted_profiles(),
            Err(GalapagosError::PersistenceInvariantViolated {
                reason: PersistenceViolation::ForeignCompartmentRow { profile_id: ProfileId(2) },
            })
        );
        assert_eq!(coordinator.profile_count(), 0);
    }

    /// W-I5 (D26): CustomProxy solo se rechaza en un perfil Personal con base persistente adjunta
    #[test]
    fn test_custom_proxy_not_persistable_delta07() {
        let proxy = || NetworkRoutePolicy::CustomProxy {
            host: "proxy.internal.corp".to_string(),
            port: NonZeroU16::new(8080).unwrap(),
            enforce_remote_dns: true,
            block_webrtc_leaks: true,
        };
        let mut persisted = with_persistent_storage();
        assert_eq!(
            persisted.create_profile(ProfileId(1), ProfileCompartment::Personal, PrivacyLevel::Daily, proxy()),
            Err(GalapagosError::PersistenceInvariantViolated {
                reason: PersistenceViolation::RoutePolicyNotPersistable,
            })
        );
        assert!(persisted.storage().unwrap().list_profiles().unwrap().is_empty());
        assert_eq!(
            persisted.create_profile(ProfileId(2), ProfileCompartment::TemporaryPrivate, PrivacyLevel::Ephemeral, proxy()),
            Ok(())
        );

        let mut in_memory = Coordinator::new();
        assert_eq!(
            in_memory.create_profile(ProfileId(1), ProfileCompartment::Personal, PrivacyLevel::Daily, proxy()),
            Ok(())
        );
    }

    #[test]
    fn test_attached_storage_permission_flow() {
        let mut coordinator = Coordinator::new();
        let storage = StorageEngine::open_in_memory().expect("open storage");
        coordinator.attach_storage(storage);

        // Crear perfil con almacenamiento vinculado
        assert!(coordinator.dispatch_ui_cmd(UiToCoordinatorCmd::CreateProfile {
            compartment: ProfileCompartment::Personal,
            privacy_level: PrivacyLevel::Daily,
            route_policy: NetworkRoutePolicy::DirectSystem,
        }).is_ok());

        // Asignar permiso a perfil existente
        let res = coordinator.dispatch_ui_cmd(UiToCoordinatorCmd::SetSitePermission {
            profile_id: ProfileId(1),
            origin: "https://example.com".to_string(),
            capability: SiteCapability::Notifications,
            grant: PermissionGrant::AllowExplicitRevocable,
        });
        assert!(res.is_ok());

        // Comprobar que quedó persistido en StorageEngine
        let storage_ref = coordinator.storage().unwrap();
        assert_eq!(
            storage_ref.get_site_permission(ProfileId(1), "https://example.com", SiteCapability::Notifications).unwrap(),
            Some(PermissionGrant::AllowExplicitRevocable)
        );
    }

    #[test]
    fn test_ocean_defense_navigation_blocking() {
        let mut coordinator = Coordinator::new();
        coordinator.defense_mut().block_host("phishing.evil.com".to_string());

        let profile_id = ProfileId(1);
        let tab_id = TabId(1);

        assert!(coordinator
            .create_profile(
                profile_id,
                ProfileCompartment::Personal,
                PrivacyLevel::Daily,
                NetworkRoutePolicy::DirectSystem
            )
            .is_ok());

        assert!(coordinator
            .spawn_tab(tab_id, profile_id, "about:blank".to_string(), dummy_quota())
            .is_ok());

        // Navegación válida
        assert!(coordinator.navigate(tab_id, "https://servo.org".to_string()).is_ok());

        // Navegación bloqueada por Ocean Defense RequestGuard
        let blocked = coordinator.navigate(tab_id, "https://phishing.evil.com/login".to_string());
        assert_eq!(
            blocked,
            Err(GalapagosError::PermissionDenied {
                profile_id,
                origin: "https://phishing.evil.com/login".to_string(),
                capability: SiteCapability::LocalStorage,
            })
        );
    }

    #[test]
    fn test_behavior_sentinel_redirection_loop_blocking() {
        let mut coordinator = Coordinator::new();
        let profile_id = ProfileId(1);
        let tab_id = TabId(2);

        assert!(coordinator
            .create_profile(
                profile_id,
                ProfileCompartment::Personal,
                PrivacyLevel::Daily,
                NetworkRoutePolicy::DirectSystem
            )
            .is_ok());

        assert!(coordinator
            .spawn_tab(tab_id, profile_id, "about:blank".to_string(), dummy_quota())
            .is_ok());

        // Simular 10 redirecciones consecutivas (por debajo del límite de bucle)
        for i in 1..=10 {
            coordinator
                .sentinel_mut()
                .record_navigation_hop(tab_id, &format!("https://redirect{}.org", i));
        }

        // El 11vo salto a través de navigate debe ser interceptado y bloqueado por Behavior Sentinel
        let res = coordinator.navigate(tab_id, "https://redirect11.org".to_string());
        assert_eq!(
            res,
            Err(GalapagosError::PermissionDenied {
                profile_id,
                origin: "https://redirect11.org".to_string(),
                capability: SiteCapability::LocalStorage,
            })
        );

        // Verificar que la alerta fue registrada en el centinela
        assert_eq!(coordinator.sentinel().recent_alerts().len(), 1);
        let alert = &coordinator.sentinel().recent_alerts()[0];
        assert_eq!(alert.severity, crate::defense::AlertSeverity::Critical);
        assert_eq!(alert.action, DefenseDecision::Block);

        // Cerrar pestaña y verificar desecho completo de recursos (Zero-Leak)
        assert!(coordinator.terminate_tab(tab_id, TerminationReason::UserClosed).is_ok());
        assert_eq!(coordinator.sentinel().active_verdict_for_tab(tab_id), None);
    }

    #[test]
    fn test_threat_intel_feed_hot_update_and_audit() {
        let mut coordinator = Coordinator::new();
        let storage = StorageEngine::open_in_memory().expect("open storage");
        coordinator.attach_storage(storage);

        let feed_content = b"# Feeds de Produccion\nbad-actor.org\nphishing-alert.net\n";
        let digest = crate::defense::compute_sha256(feed_content);
        let mut signature = [0x5au8; 64];
        signature[63] = 0x05;

        let manifest = SignedArtifactManifest {
            artifact_id: "sec03-threat-intel-bundle".to_string(),
            version_semver: (1, 2, 0),
            sha256_digest: digest,
            ed25519_signature: signature,
        };

        // Fail-closed (INC-20261025-CORE-DEBLOAT T-01): sin verificador Ed25519 real se rechaza
        assert_eq!(
            coordinator.apply_signed_threat_feed(&manifest, feed_content),
            Err(GalapagosError::IntegrityVerificationFailed {
                artifact_id: "sec03-threat-intel-bundle".to_string(),
            })
        );
        assert_eq!(coordinator.threat_intel().current_version(), (0, 0, 0));

        // El host del feed rechazado no queda bloqueado
        let profile_id = ProfileId(1);
        let tab_id = TabId(5);
        coordinator
            .create_profile(
                profile_id,
                ProfileCompartment::Personal,
                PrivacyLevel::Daily,
                NetworkRoutePolicy::DirectSystem,
            )
            .unwrap();
        coordinator
            .spawn_tab(tab_id, profile_id, "about:blank".to_string(), dummy_quota())
            .unwrap();

        assert!(coordinator
            .navigate(tab_id, "https://bad-actor.org/exploit".to_string())
            .is_ok());
    }
}
