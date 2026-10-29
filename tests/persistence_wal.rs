//! tests/persistence_wal.rs
//! DELTA_07 paso 5: persistencia WAL del perfil Personal y base por compartimento, sobre un
//! `personal.db` real en una carpeta temporal (la biblioteca se compila sin `cfg(test)`).

use galapagos_browser::coordinator::Coordinator;
use galapagos_browser::galapagos_core_v1::{
    GalapagosError, NetworkRoutePolicy, PermissionGrant, PersistenceViolation, PrivacyLevel,
    ProfileCompartment, ProfileId, SiteCapability, TabId, UiToCoordinatorCmd,
    MAX_PERSISTED_PERF_SAMPLES, SCHEMA_VERSION,
};
use galapagos_browser::storage::StorageEngine;

use std::num::NonZeroU16;
use std::path::{Path, PathBuf};

/// Carpeta temporal propia de cada test; se borra al crearse y al soltarse (después de cerrar la base)
struct TempDb {
    dir: PathBuf,
}

impl TempDb {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("galapagos_it_delta07_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn path(&self) -> PathBuf {
        self.dir.join("personal.db")
    }
}

impl Drop for TempDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Un «arranque»: base WAL en disco adjunta y perfiles guardados cargados (orden W-I3)
fn boot(db: &Path) -> Coordinator {
    let mut coordinator = Coordinator::new();
    coordinator.attach_storage(StorageEngine::open_file(db).expect("abrir personal.db"));
    coordinator.load_persisted_profiles().expect("cargar perfiles");
    coordinator
}

fn create(coordinator: &mut Coordinator, compartment: ProfileCompartment, privacy_level: PrivacyLevel) {
    coordinator
        .dispatch_ui_cmd(UiToCoordinatorCmd::CreateProfile {
            compartment,
            privacy_level,
            route_policy: NetworkRoutePolicy::DirectSystem,
        })
        .expect("crear perfil");
}

fn grant(coordinator: &mut Coordinator, profile_id: ProfileId) {
    coordinator
        .dispatch_ui_cmd(UiToCoordinatorCmd::SetSitePermission {
            profile_id,
            origin: "https://example.com".to_string(),
            capability: SiteCapability::Notifications,
            grant: PermissionGrant::AllowExplicitRevocable,
        })
        .expect("asignar permiso");
}

/// W-I2/W-I4: el segundo arranque carga el perfil y su permiso, no lo reinserta y continúa los ids
#[test]
fn second_boot_reuses_profile_and_permission_delta07() {
    let tmp = TempDb::new("reboot");
    {
        let mut first = boot(&tmp.path());
        assert_eq!(first.lowest_profile_id(ProfileCompartment::Personal), None);
        create(&mut first, ProfileCompartment::Personal, PrivacyLevel::Daily);
        grant(&mut first, ProfileId(1));
    }

    let mut second = boot(&tmp.path());
    assert_eq!(second.profile_count(), 1);
    assert_eq!(second.lowest_profile_id(ProfileCompartment::Personal), Some(ProfileId(1)));
    assert_eq!(
        second
            .storage()
            .unwrap()
            .get_site_permission(ProfileId(1), "https://example.com", SiteCapability::Notifications),
        Ok(Some(PermissionGrant::AllowExplicitRevocable))
    );

    // El siguiente perfil no reutiliza el id 1
    create(&mut second, ProfileCompartment::Personal, PrivacyLevel::Hardened);
    let ids: Vec<u64> = second.storage().unwrap().list_profiles().unwrap().iter().map(|r| r.profile_id.0).collect();
    assert_eq!(ids, vec![1, 2]);
}

/// W-I1 (PRIV-01): un perfil efímero con permisos no deja ninguna fila en el archivo
#[test]
fn ephemeral_profile_leaves_no_rows_on_disk_delta07() {
    let tmp = TempDb::new("ephemeral");
    {
        let mut coordinator = boot(&tmp.path());
        create(&mut coordinator, ProfileCompartment::TemporaryPrivate, PrivacyLevel::Ephemeral);
        create(&mut coordinator, ProfileCompartment::Laboratory, PrivacyLevel::Hardened);
        grant(&mut coordinator, ProfileId(1));
        grant(&mut coordinator, ProfileId(2));
        coordinator.destroy_ephemeral_profile(ProfileId(1)).unwrap();
    }

    // Conexión aparte sobre el archivo: cero filas de perfiles y de permisos
    let raw = rusqlite::Connection::open(tmp.path()).unwrap();
    let count = |table: &str| -> i64 {
        raw.query_row(&format!("SELECT count(*) FROM {};", table), [], |r| r.get(0)).unwrap()
    };
    assert_eq!(count("profiles"), 0);
    assert_eq!(count("site_permissions"), 0);
}

/// W-6/W-7: el archivo queda en modo WAL con `user_version` = SCHEMA_VERSION
#[test]
fn file_is_wal_with_schema_version_delta07() {
    let tmp = TempDb::new("wal");
    drop(boot(&tmp.path()));
    let raw = rusqlite::Connection::open(tmp.path()).unwrap();
    let mode: String = raw.query_row("PRAGMA journal_mode;", [], |r| r.get(0)).unwrap();
    let version: i64 = raw.query_row("PRAGMA user_version;", [], |r| r.get(0)).unwrap();
    assert_eq!(mode.to_ascii_lowercase(), "wal");
    assert_eq!(version, SCHEMA_VERSION);
}

/// W-7: una base de un binario más nuevo no se abre
#[test]
fn newer_schema_version_is_refused_delta07() {
    let tmp = TempDb::new("newer");
    rusqlite::Connection::open(tmp.path())
        .unwrap()
        .pragma_update(None, "user_version", 2i64)
        .unwrap();
    assert_eq!(
        StorageEngine::open_file(&tmp.path()).err(),
        Some(GalapagosError::PersistenceInvariantViolated {
            reason: PersistenceViolation::SchemaVersionUnsupported { found: 2 },
        })
    );
}

/// W-I5 (D26): CustomProxy en Personal se rechaza y no deja fila en el archivo
#[test]
fn custom_proxy_on_personal_is_not_persisted_delta07() {
    let tmp = TempDb::new("proxy");
    let mut coordinator = boot(&tmp.path());
    let res = coordinator.dispatch_ui_cmd(UiToCoordinatorCmd::CreateProfile {
        compartment: ProfileCompartment::Personal,
        privacy_level: PrivacyLevel::Daily,
        route_policy: NetworkRoutePolicy::CustomProxy {
            host: "proxy.internal.corp".to_string(),
            port: NonZeroU16::new(8080).unwrap(),
            enforce_remote_dns: true,
            block_webrtc_leaks: true,
        },
    });
    assert_eq!(
        res,
        Err(GalapagosError::PersistenceInvariantViolated {
            reason: PersistenceViolation::RoutePolicyNotPersistable,
        })
    );
    assert!(coordinator.storage().unwrap().list_profiles().unwrap().is_empty());
}

/// D27: tras 10 500 muestras en el archivo quedan como máximo 10 000, incluida la última
#[test]
fn perf_samples_are_capped_on_disk_delta07() {
    let tmp = TempDb::new("perf");
    let storage = StorageEngine::open_file(&tmp.path()).unwrap();
    let tab = TabId(1);
    for i in 0..(MAX_PERSISTED_PERF_SAMPLES + 500) {
        storage.insert_perf_sample(tab, i, 1, 1, 1, 1).unwrap();
    }
    let rows = storage.query_perf_baseline(tab, (MAX_PERSISTED_PERF_SAMPLES * 2) as u32).unwrap();
    assert!(rows.len() as i64 <= MAX_PERSISTED_PERF_SAMPLES, "{} filas", rows.len());
    assert_eq!(rows[0].timestamp_ns, MAX_PERSISTED_PERF_SAMPLES + 499);
}
