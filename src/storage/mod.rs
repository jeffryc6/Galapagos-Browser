//! src/storage/mod.rs
//! Motor de Persistencia Local SQLite para Galapagos Browser v0.1
//! Conforme a contracts/schema_v1.sql y estándar ZNVE v2.4.0.
//! Persistencia en WAL para perfiles persistentes y :memory: para efímeros.

#![forbid(unsafe_code)]

use crate::galapagos_core_v1::{
    GalapagosError, PermissionGrant, PersistedProfileRow, PersistenceViolation, PrivacyLevel,
    ProfileCompartment, ProfileId, SiteCapability, TabId, MAX_PERSISTED_PERF_SAMPLES, SCHEMA_VERSION,
};
use rusqlite::{params, Connection, Error as SqliteError, TransactionBehavior};
use std::path::Path;
use std::sync::Mutex;

/// DDL único: el mismo archivo del contrato (antes duplicado aquí a mano)
const SCHEMA_V1_DDL: &str = include_str!("../../contracts/schema_v1.sql");

/// Cada cuántas inserciones se aplica la retención de `perf_baseline_samples` (D27)
const PERF_RETENTION_EVERY: i64 = 1_000;

fn persistence_violation(reason: PersistenceViolation) -> GalapagosError {
    GalapagosError::PersistenceInvariantViolated { reason }
}

fn code_to_compartment(code: i64) -> Option<ProfileCompartment> {
    match code {
        1 => Some(ProfileCompartment::Personal),
        2 => Some(ProfileCompartment::TemporaryPrivate),
        3 => Some(ProfileCompartment::Laboratory),
        _ => None,
    }
}

fn code_to_privacy_level(code: i64) -> Option<PrivacyLevel> {
    match code {
        1 => Some(PrivacyLevel::Daily),
        2 => Some(PrivacyLevel::Ephemeral),
        3 => Some(PrivacyLevel::Hardened),
        _ => None,
    }
}

/// Mapeo determinista de códigos numéricos conforme a contracts/schema_v1.sql
fn compartment_to_code(c: ProfileCompartment) -> i64 {
    match c {
        ProfileCompartment::Personal => 1,
        ProfileCompartment::TemporaryPrivate => 2,
        ProfileCompartment::Laboratory => 3,
    }
}

fn privacy_level_to_code(p: PrivacyLevel) -> i64 {
    match p {
        PrivacyLevel::Daily => 1,
        PrivacyLevel::Ephemeral => 2,
        PrivacyLevel::Hardened => 3,
    }
}

fn capability_to_code(cap: SiteCapability) -> i64 {
    match cap {
        SiteCapability::ClipboardRead => 1,
        SiteCapability::Notifications => 2,
        SiteCapability::MediaCapture => 3,
        SiteCapability::WebRtcPeerConnection => 4,
        SiteCapability::LocalStorage => 5,
    }
}

fn code_to_grant(code: i64) -> Result<PermissionGrant, GalapagosError> {
    match code {
        0 => Ok(PermissionGrant::Deny),
        1 => Ok(PermissionGrant::AllowSessionOnly),
        2 => Ok(PermissionGrant::AllowExplicitRevocable),
        _ => Err(GalapagosError::StorageFailure {
            sqlite_extended_code: -1,
        }),
    }
}

fn grant_to_code(g: PermissionGrant) -> i64 {
    match g {
        PermissionGrant::Deny => 0,
        PermissionGrant::AllowSessionOnly => 1,
        PermissionGrant::AllowExplicitRevocable => 2,
    }
}

/// Registro de muestra de rendimiento retornado por proyecciones autorizadas
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerfBaselineRow {
    pub timestamp_ns: i64,
    pub working_set_bytes: i64,
    pub cpu_delta_us: i64,
    pub sec_overhead_us: i64,
}

/// Motor de persistencia agnóstico y determinista
pub struct StorageEngine {
    conn: Mutex<Connection>,
}

impl StorageEngine {
    /// Inicializa una base de datos en RAM para perfiles efímeros/laboratorio (PRIV-01 / PRIV-02).
    /// `synchronous=OFF` solo se admite en memoria (DELTA_07 §3).
    pub fn open_in_memory() -> Result<Self, GalapagosError> {
        let conn = Connection::open_in_memory().map_err(Self::map_err)?;
        conn.execute_batch("PRAGMA synchronous=OFF; PRAGMA foreign_keys=ON;")
            .map_err(Self::map_err)?;
        let engine = Self {
            conn: Mutex::new(conn),
        };
        engine.apply_schema()?;
        Ok(engine)
    }

    /// Inicializa una base de datos persistente en disco con modo WAL (PRIV-01 / SEC-02).
    /// DELTA_07 W-6: el modo WAL se comprueba en la respuesta del PRAGMA; si el sistema de
    /// archivos no lo acepta, la apertura falla en lugar de seguir en otro modo sin avisar.
    pub fn open_file(path: &Path) -> Result<Self, GalapagosError> {
        let conn = Connection::open(path).map_err(Self::map_err)?;
        let mode: String = conn
            .query_row("PRAGMA journal_mode=WAL;", [], |row| row.get(0))
            .map_err(Self::map_err)?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(persistence_violation(PersistenceViolation::WalUnavailable));
        }
        conn.execute_batch("PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;")
            .map_err(Self::map_err)?;
        let engine = Self {
            conn: Mutex::new(conn),
        };
        engine.apply_schema()?;
        Ok(engine)
    }

    fn map_err(err: SqliteError) -> GalapagosError {
        let code = match err {
            SqliteError::SqliteFailure(extended, _) => extended.extended_code,
            _ => -1,
        };
        GalapagosError::StorageFailure {
            sqlite_extended_code: code,
        }
    }

    /// Aplica el DDL de `contracts/schema_v1.sql` y fija `user_version` (DELTA_07 W-7).
    /// Versión igual a `SCHEMA_VERSION`: sin cambios. Mayor: la base es de un binario más nuevo
    /// y se rechaza sin escribir. Menor (0): DDL idempotente y versión en una sola transacción.
    pub fn apply_schema(&self) -> Result<(), GalapagosError> {
        let mut conn = self.conn.lock().unwrap();
        let found: i64 = conn
            .query_row("PRAGMA user_version;", [], |row| row.get(0))
            .map_err(Self::map_err)?;
        if found > SCHEMA_VERSION {
            return Err(persistence_violation(PersistenceViolation::SchemaVersionUnsupported { found }));
        }
        if found == SCHEMA_VERSION {
            return Ok(());
        }
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Self::map_err)?;
        tx.execute_batch(SCHEMA_V1_DDL).map_err(Self::map_err)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(Self::map_err)?;
        tx.commit().map_err(Self::map_err)
    }

    /// Versión de esquema registrada en la base (`PRAGMA user_version`)
    pub fn schema_version(&self) -> Result<i64, GalapagosError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("PRAGMA user_version;", [], |row| row.get(0))
            .map_err(Self::map_err)
    }

    /// Registra un nuevo perfil compartimentado (PRIV-01)
    pub fn insert_profile(
        &self,
        profile_id: ProfileId,
        compartment: ProfileCompartment,
        privacy_level: PrivacyLevel,
        created_at_epoch_s: i64,
    ) -> Result<(), GalapagosError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO profiles (profile_id, compartment_code, privacy_level_code, created_at_epoch_s)
             VALUES (?1, ?2, ?3, ?4);",
            params![
                profile_id.0 as i64,
                compartment_to_code(compartment),
                privacy_level_to_code(privacy_level),
                created_at_epoch_s,
            ],
        )
        .map_err(Self::map_err)?;
        Ok(())
    }

    /// DELTA_07 W-I2: perfiles guardados, ordenados por id (proyección autorizada; lectura completa
    /// intencionada de una tabla de pocas filas por su clave primaria). Un código fuera del
    /// esquema es una base corrupta: `StorageFailure { -1 }`.
    pub fn list_profiles(&self) -> Result<Vec<PersistedProfileRow>, GalapagosError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare_cached(
                "SELECT profile_id, compartment_code, privacy_level_code, created_at_epoch_s
                 FROM profiles ORDER BY profile_id;",
            )
            .map_err(Self::map_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
            })
            .map_err(Self::map_err)?;

        let mut profiles = Vec::new();
        for row in rows {
            let (id, compartment, privacy, created_at_epoch_s) = row.map_err(Self::map_err)?;
            let (Some(compartment), Some(privacy_level), Ok(id)) =
                (code_to_compartment(compartment), code_to_privacy_level(privacy), u64::try_from(id))
            else {
                return Err(GalapagosError::StorageFailure { sqlite_extended_code: -1 });
            };
            profiles.push(PersistedProfileRow {
                profile_id: ProfileId(id),
                compartment,
                privacy_level,
                created_at_epoch_s,
            });
        }
        Ok(profiles)
    }

    /// Consulta de permisos en ruta caliente (Query parametrizada obligatoria de contract.md)
    pub fn get_site_permission(
        &self,
        profile_id: ProfileId,
        origin_host: &str,
        capability: SiteCapability,
    ) -> Result<Option<PermissionGrant>, GalapagosError> {
        let conn = self.conn.lock().unwrap();
        // P-06: sentencias de ruta caliente preparadas una vez por conexión (caché de rusqlite)
        let mut stmt = conn
            .prepare_cached(
                "SELECT grant_state FROM site_permissions
                 WHERE profile_id = ?1 AND origin_host = ?2 AND capability_code = ?3
                 LIMIT 1;",
            )
            .map_err(Self::map_err)?;

        let mut rows = stmt
            .query(params![
                profile_id.0 as i64,
                origin_host,
                capability_to_code(capability)
            ])
            .map_err(Self::map_err)?;

        if let Some(row) = rows.next().map_err(Self::map_err)? {
            let code: i64 = row.get(0).map_err(Self::map_err)?;
            Ok(Some(code_to_grant(code)?))
        } else {
            Ok(None)
        }
    }

    /// Asigna o revoca un permiso explícito por sitio (SEC-02)
    pub fn set_site_permission(
        &self,
        profile_id: ProfileId,
        origin_host: &str,
        capability: SiteCapability,
        grant: PermissionGrant,
        updated_at_epoch_s: i64,
    ) -> Result<(), GalapagosError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare_cached(
            "INSERT INTO site_permissions (profile_id, origin_host, capability_code, grant_state, updated_at_epoch_s)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(profile_id, origin_host, capability_code)
             DO UPDATE SET grant_state = excluded.grant_state, updated_at_epoch_s = excluded.updated_at_epoch_s;",
        )
        .and_then(|mut stmt| stmt.execute(
            params![
                profile_id.0 as i64,
                origin_host,
                capability_to_code(capability),
                grant_to_code(grant),
                updated_at_epoch_s,
            ],
        ))
        .map_err(Self::map_err)?;
        Ok(())
    }

    /// Registra auditoría de artefactos verificados y listas de bloqueo (SEC-03)
    pub fn insert_verified_artifact(
        &self,
        artifact_id: &str,
        major: u16,
        minor: u16,
        patch: u16,
        sha256_hex: &str,
        verified_at_epoch_s: i64,
    ) -> Result<(), GalapagosError> {
        if sha256_hex.len() != 64 {
            return Err(GalapagosError::IntegrityVerificationFailed {
                artifact_id: artifact_id.to_string(),
            });
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO verified_artifacts (artifact_id, version_major, version_minor, version_patch, sha256_hex, verified_at_epoch_s)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
            params![
                artifact_id,
                major as i64,
                minor as i64,
                patch as i64,
                sha256_hex,
                verified_at_epoch_s,
            ],
        )
        .map_err(Self::map_err)?;
        Ok(())
    }

    /// Inserta una muestra de telemetría de rendimiento determinista (PERF-01)
    pub fn insert_perf_sample(
        &self,
        tab_id: TabId,
        timestamp_ns: i64,
        working_set_bytes: i64,
        cpu_delta_us: i64,
        nav_latency_ms: i64,
        sec_overhead_us: i64,
    ) -> Result<(), GalapagosError> {
        let conn = self.conn.lock().unwrap();
        conn.prepare_cached(
            "INSERT INTO perf_baseline_samples (tab_id, timestamp_ns, working_set_bytes, cpu_delta_us, nav_latency_ms, sec_overhead_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
        )
        .and_then(|mut stmt| stmt.execute(
            params![
                tab_id.0 as i64,
                timestamp_ns,
                working_set_bytes,
                cpu_delta_us,
                nav_latency_ms,
                sec_overhead_us,
            ],
        ))
        .map_err(Self::map_err)?;

        // D27: cada PERF_RETENTION_EVERY inserciones se conservan las últimas
        // MAX_PERSISTED_PERF_SAMPLES - PERF_RETENTION_EVERY filas, de modo que entre dos podas la
        // tabla nunca supera MAX_PERSISTED_PERF_SAMPLES (borrado por `sample_id`, la clave primaria)
        if conn.last_insert_rowid() % PERF_RETENTION_EVERY == 0 {
            conn.prepare_cached(
                "DELETE FROM perf_baseline_samples
                 WHERE sample_id <= (SELECT MAX(sample_id) FROM perf_baseline_samples) - ?1;",
            )
            .and_then(|mut stmt| stmt.execute(params![MAX_PERSISTED_PERF_SAMPLES - PERF_RETENTION_EVERY]))
            .map_err(Self::map_err)?;
        }
        Ok(())
    }

    /// Lectura de línea base de memoria y CPU (Query parametrizada obligatoria de contract.md)
    pub fn query_perf_baseline(
        &self,
        tab_id: TabId,
        limit: u32,
    ) -> Result<Vec<PerfBaselineRow>, GalapagosError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare_cached(
                "SELECT timestamp_ns, working_set_bytes, cpu_delta_us, sec_overhead_us
                 FROM perf_baseline_samples
                 WHERE tab_id = ?1
                 ORDER BY timestamp_ns DESC
                 LIMIT ?2;",
            )
            .map_err(Self::map_err)?;

        let rows = stmt
            .query_map(params![tab_id.0 as i64, limit as i64], |row| {
                Ok(PerfBaselineRow {
                    timestamp_ns: row.get(0)?,
                    working_set_bytes: row.get(1)?,
                    cpu_delta_us: row.get(2)?,
                    sec_overhead_us: row.get(3)?,
                })
            })
            .map_err(Self::map_err)?;

        let mut result = Vec::new();
        for r in rows {
            result.push(r.map_err(Self::map_err)?);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Carpeta temporal propia de cada test (se borra al empezar y al terminar)
    fn temp_db(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("galapagos_delta07_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("personal.db");
        (dir, db)
    }

    /// DELTA_07 W-6/W-7: la base en disco queda en WAL, con user_version 1, y conserva los datos al reabrir
    #[test]
    fn test_open_file_wal_schema_version_and_reopen_delta07() {
        let (dir, db) = temp_db("reopen");
        {
            let storage = StorageEngine::open_file(&db).expect("abrir WAL");
            assert_eq!(storage.schema_version(), Ok(SCHEMA_VERSION));
            storage
                .insert_profile(ProfileId(3), ProfileCompartment::Personal, PrivacyLevel::Hardened, 1_700_000_000)
                .unwrap();
        }
        let raw = Connection::open(&db).unwrap();
        let mode: String = raw.query_row("PRAGMA journal_mode;", [], |r| r.get(0)).unwrap();
        assert_eq!(mode.to_ascii_lowercase(), "wal");
        drop(raw);

        let storage = StorageEngine::open_file(&db).expect("reabrir");
        assert_eq!(
            storage.list_profiles(),
            Ok(vec![PersistedProfileRow {
                profile_id: ProfileId(3),
                compartment: ProfileCompartment::Personal,
                privacy_level: PrivacyLevel::Hardened,
                created_at_epoch_s: 1_700_000_000,
            }])
        );
        drop(storage);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// W-6: si SQLite no acepta WAL (aquí, una base `:memory:` abierta como archivo), la apertura falla
    #[test]
    fn test_open_file_rejects_non_wal_journal_delta07() {
        assert!(matches!(
            StorageEngine::open_file(Path::new(":memory:")),
            Err(GalapagosError::PersistenceInvariantViolated { reason: PersistenceViolation::WalUnavailable })
        ));
    }

    /// W-7: una base de un binario más nuevo se rechaza sin crear tablas
    #[test]
    fn test_newer_schema_version_rejected_without_writes_delta07() {
        let (dir, db) = temp_db("newer");
        Connection::open(&db).unwrap().pragma_update(None, "user_version", 2i64).unwrap();
        assert_eq!(
            StorageEngine::open_file(&db).err(),
            Some(GalapagosError::PersistenceInvariantViolated {
                reason: PersistenceViolation::SchemaVersionUnsupported { found: 2 },
            })
        );
        let raw = Connection::open(&db).unwrap();
        let tables: i64 = raw
            .query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table';", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tables, 0);
        drop(raw);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// W-7: una base con tablas y user_version 0 (versiones anteriores de este binario) se valida y pasa a 1
    #[test]
    fn test_legacy_version_zero_database_upgraded_delta07() {
        let (dir, db) = temp_db("legacy");
        {
            let raw = Connection::open(&db).unwrap();
            raw.execute_batch(SCHEMA_V1_DDL).unwrap();
            raw.execute(
                "INSERT INTO profiles (profile_id, compartment_code, privacy_level_code, created_at_epoch_s) VALUES (1, 1, 1, 0);",
                [],
            )
            .unwrap();
        }
        let storage = StorageEngine::open_file(&db).expect("abrir base heredada");
        assert_eq!(storage.schema_version(), Ok(1));
        assert_eq!(storage.list_profiles().unwrap().len(), 1);
        drop(storage);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// D27: la retención deja como máximo MAX_PERSISTED_PERF_SAMPLES filas y conserva las más recientes
    #[test]
    fn test_perf_sample_retention_cap_delta07() {
        let storage = StorageEngine::open_in_memory().unwrap();
        let tid = TabId(1);
        for i in 0..(MAX_PERSISTED_PERF_SAMPLES + 500) {
            storage.insert_perf_sample(tid, i, 1, 1, 1, 1).unwrap();
        }
        let rows = storage.query_perf_baseline(tid, (MAX_PERSISTED_PERF_SAMPLES * 2) as u32).unwrap();
        assert!(rows.len() as i64 <= MAX_PERSISTED_PERF_SAMPLES, "{} filas", rows.len());
        assert_eq!(rows[0].timestamp_ns, MAX_PERSISTED_PERF_SAMPLES + 499);
    }

    /// W-I2: códigos fuera del esquema no se aceptan al cargar perfiles
    #[test]
    fn test_list_profiles_rejects_unknown_codes_delta07() {
        let storage = StorageEngine::open_in_memory().unwrap();
        storage
            .conn
            .lock()
            .unwrap()
            .execute_batch(
                "PRAGMA ignore_check_constraints = ON;
                 INSERT INTO profiles (profile_id, compartment_code, privacy_level_code, created_at_epoch_s) VALUES (5, 9, 1, 0);",
            )
            .unwrap();
        assert_eq!(storage.list_profiles(), Err(GalapagosError::StorageFailure { sqlite_extended_code: -1 }));
    }

    #[test]
    fn test_storage_lifecycle_and_projections() {
        let storage = StorageEngine::open_in_memory().expect("open memory db");
        let pid = ProfileId(1);

        // 1. Insertar perfil
        assert!(storage
            .insert_profile(pid, ProfileCompartment::Personal, PrivacyLevel::Daily, 1700000000)
            .is_ok());

        // 2. Comprobar que permiso no existe inicialmente
        assert_eq!(
            storage
                .get_site_permission(pid, "example.com", SiteCapability::Notifications)
                .unwrap(),
            None
        );

        // 3. Asignar permiso explícito
        assert!(storage
            .set_site_permission(
                pid,
                "example.com",
                SiteCapability::Notifications,
                PermissionGrant::AllowExplicitRevocable,
                1700000010
            )
            .is_ok());

        // 4. Verificar permiso vía proyección estricta
        assert_eq!(
            storage
                .get_site_permission(pid, "example.com", SiteCapability::Notifications)
                .unwrap(),
            Some(PermissionGrant::AllowExplicitRevocable)
        );

        // 5. Revocar permiso (actualización atómica)
        assert!(storage
            .set_site_permission(
                pid,
                "example.com",
                SiteCapability::Notifications,
                PermissionGrant::Deny,
                1700000020
            )
            .is_ok());

        assert_eq!(
            storage
                .get_site_permission(pid, "example.com", SiteCapability::Notifications)
                .unwrap(),
            Some(PermissionGrant::Deny)
        );
    }

    #[test]
    fn test_perf_baseline_projection() {
        let storage = StorageEngine::open_in_memory().expect("open memory db");
        let tid = TabId(42);

        storage
            .insert_perf_sample(tid, 1000, 50_000_000, 1500, 25, 300)
            .unwrap();
        storage
            .insert_perf_sample(tid, 2000, 55_000_000, 1800, 20, 280)
            .unwrap();

        let samples = storage.query_perf_baseline(tid, 10).unwrap();
        assert_eq!(samples.len(), 2);
        // Debe venir ordenado DESC por timestamp_ns
        assert_eq!(samples[0].timestamp_ns, 2000);
        assert_eq!(samples[0].working_set_bytes, 55_000_000);
        assert_eq!(samples[1].timestamp_ns, 1000);
    }

    #[test]
    fn test_foreign_key_and_integrity_constraints() {
        let storage = StorageEngine::open_in_memory().expect("open memory db");
        // Intentar registrar un permiso para un perfil que NO existe debe fallar por FOREIGN KEY
        let err = storage.set_site_permission(
            ProfileId(999),
            "attacker.com",
            SiteCapability::LocalStorage,
            PermissionGrant::AllowSessionOnly,
            1700000000,
        );
        assert!(err.is_err());
    }
}
