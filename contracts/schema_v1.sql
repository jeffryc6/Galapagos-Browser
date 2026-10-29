-- contracts/schema_v1.sql
-- Motor de persistencia local para Galapagos Browser v0.1
-- Modo de ejecución obligatorio para perfiles persistentes:
-- PRAGMA journal_mode=WAL;
-- PRAGMA synchronous=NORMAL;
-- PRAGMA foreign_keys=ON;
-- Para perfiles efímeros/laboratorio: URI ':memory:' sin archivos .wal/.shm en disco.
--
-- DELTA_07: versión de esquema = PRAGMA user_version = 1 (SCHEMA_VERSION en galapagos_core_v1.rs).
-- StorageEngine aplica este archivo (include_str!) y fija user_version en una sola transacción
-- BEGIN IMMEDIATE; una base con user_version > 1 se rechaza sin escribir.
-- Ubicación de la base persistente: %LOCALAPPDATA%\GalapagosBrowser\profiles\personal.db
-- (solo perfiles Personal; cada perfil efímero usa su propia base ':memory:').
-- Retención: perf_baseline_samples conserva como máximo 10 000 filas (MAX_PERSISTED_PERF_SAMPLES).

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
