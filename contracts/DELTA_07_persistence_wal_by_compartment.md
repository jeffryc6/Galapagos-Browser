# DELTA 07 — Persistencia WAL del perfil Personal y base por compartimento

**Estado:** CONGELADO (2026-10-26) · D23–D28 aprobadas por el Director (recomendaciones) · **Origen:** P-07 de `TRIAGE_INC-20261025-CORE-DEBLOAT.md` · **Extiende:** `contract.md` (Contrato de persistencia, PRIV-01), `contracts/schema_v1.sql`, `docs/architecture/PERSISTENCE_DESIGN.md` (solo aditivo; el esquema v1 no cambia)

## 0. Qué pasa hoy

`contract.md` exige SQLite en modo WAL en disco para los perfiles `Personal`, y `:memory:` sin archivos `.wal`/`.shm` para `TemporaryPrivate` y `Laboratory`. Hoy todo vive en memoria y se pierde al cerrar. Activar el disco sin rediseño rompe dos cosas:
- **Segundo arranque:** `CreateProfile` vuelve a asignar `ProfileId(1)` y el `INSERT` en `profiles` choca con la clave primaria; `main` lo trata como fatal.
- **PRIV-01:** hay una sola base para todos los perfiles, así que los perfiles efímeros y sus permisos acabarían escritos en disco.

## 1. Auditoría contratos ↔ src

| Id | Discrepancia | Evidencia | Sev. | Cubo |
|---|---|---|---|---|
| W-1 | `Personal` se abre en memoria; `open_file` (WAL) no se usa | `main.rs:79-81` | Alta | A |
| W-2 | Una sola `StorageEngine` para todos los compartimentos: un perfil efímero escribe su fila y sus permisos en la misma base que `Personal` | `coordinator.rs:217,460` | Crítica (con disco) | A |
| W-3 | Los `ProfileId` se asignan desde 1 en cada arranque y no se cargan los perfiles persistidos | `coordinator.rs` `next_profile_id: 1` | Crítica (con disco) | A |
| W-4 | `main` adjunta el almacenamiento **después** de crear el perfil y la pestaña: la fila del perfil nunca llega a SQLite | `main.rs:56-81` | Alta | A |
| W-5 | `created_at_epoch_s` y `updated_at_epoch_s` se escriben como `0` | `coordinator.rs:217,460` | Media | A |
| W-6 | `PRAGMA journal_mode=WAL` no se comprueba: si el sistema de archivos no admite WAL, SQLite sigue en otro modo sin avisar | `storage/mod.rs:112` | Media | A |
| W-7 | Sin `user_version`: una base de una versión futura se abriría como v1 | `PERSISTENCE_DESIGN.md §3` | Media | A (solo comprobar y fijar v1) |
| W-8 | `perf_baseline_samples` crecería sin límite en disco | `main.rs:159` (`--smoke-test`) | Media | A (D27) |
| W-9 | `profiles` no guarda la `NetworkRoutePolicy`: un perfil `Personal` con `CustomProxy` volvería como `DirectSystem` tras reiniciar | `schema_v1.sql` | Media | A (rechazo, D26) / B (esquema v2) |
| W-10 | `verified_artifacts` usa `INSERT`: reaplicar el mismo `artifact_id` falla por clave primaria | `storage/mod.rs:279` | Baja (hoy inalcanzable: SEC-03 en fail-closed) | B |
| W-11 | Dos Coordinadores a la vez sobre la misma base | — | Baja | B |

## 2. CONTRATO DE ENTRADA Y SALIDA (Cubo A, aditivo)

Sin mensajes IPC nuevos y sin cambios en `UiToCoordinatorCmd`.

```rust
// contracts/galapagos_core_v1.rs (aditivo)

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

/// Δ7: tope de filas de `perf_baseline_samples` en la base persistente (D27)
pub const MAX_PERSISTED_PERF_SAMPLES: i64 = 10_000;
```

### Invariantes

- **W-I1 (D24) Una base por destino:**
  - Los perfiles `Personal` comparten **una** base en disco, en modo WAL.
  - Cada perfil `TemporaryPrivate` o `Laboratory` tiene **su propia** base `:memory:`. Se crea en `create_profile` y se destruye (cierra la conexión) en `destroy_ephemeral_profile`.
  - Ninguna fila de un perfil efímero toca la base en disco.
- **W-I2 (D25) Carga de perfiles persistidos:** antes de crear perfiles o pestañas, `Coordinator::load_persisted_profiles()`:
  - lee todos los perfiles de la base persistente;
  - los registra en memoria con `NetworkRoutePolicy::DirectSystem`;
  - fija `next_profile_id = max(profile_id) + 1`.
  - `main` reutiliza el perfil `Personal` de menor id; solo si no hay ninguno lo crea. Así, el segundo arranque no vuelve a insertar el perfil.
- **W-I3 Orden en `main`:** abrir la base → adjuntarla → cargar perfiles → obtener o crear el perfil → abrir la pestaña. La pestaña usa el `ProfileId` real (hoy `ProfileId(1)` está fijo en el código).
- **W-I4 Identificadores:** los `ProfileId` persistidos nunca se reutilizan.
  - Los efímeros se toman del mismo contador, pero pueden repetirse entre sesiones, porque no se persisten.
  - SEC-AUDIT-03 («monótono, sin reutilizar») se garantiza dentro de una sesión para todos, y entre sesiones solo para `Personal`.
- **W-I5 (D26) Ruta de red:** `create_profile(Personal, …, CustomProxy {..})` con base persistente adjunta falla con `PersistenceViolation::RoutePolicyNotPersistable`, porque el esquema v1 no puede guardarla. Con `DirectSystem` funciona igual que hoy. Los efímeros admiten `CustomProxy`, porque no se persisten.
- **W-I6 Marcas de tiempo:** `created_at_epoch_s` y `updated_at_epoch_s` se toman de `SystemTime::now()` en segundos.
- **W-I7 Compatibilidad con los tests:**
  - `attach_storage(StorageEngine)` mantiene su firma y significa «base del compartimento `Personal`».
  - `storage()` devuelve esa base.
  - Los tests actuales que adjuntan una base en memoria siguen siendo válidos sin cambios.
  - Sin base persistente adjunta, `SetSitePermission` de un perfil `Personal` (o de un id desconocido) sigue devolviendo `StorageUnattached`; el de un perfil efímero usa su propia base `:memory:`.
- **W-I8 (D28) `--smoke-test`** adjunta una base **en memoria**: la certificación nunca escribe en los datos del usuario.

## 3. CONTRATO DE PERSISTENCIA

**Ubicación (D23):** `%LOCALAPPDATA%\GalapagosBrowser\profiles\personal.db`, más `personal.db-wal` y `personal.db-shm` gestionados por SQLite. Sin `LOCALAPPDATA` no hay respaldo en `%TEMP%`: no se puede abrir la base persistente, y `main` termina con el diálogo fatal (Guardrail 9, sin degradar en silencio a memoria). En MSIX la ruta la virtualiza el sistema en el `LocalState` del paquete.

**Apertura de la base en disco** (`StorageEngine::open_file`):
1. `PRAGMA journal_mode=WAL;` debe **devolver** `wal`; si no → `PersistenceViolation::WalUnavailable`.
2. `PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;`
3. Lectura de `PRAGMA user_version;`:
   - `0` y sin tablas → aplicar `schema_v1.sql` y fijar `PRAGMA user_version = 1;`. Las dos cosas van en una única transacción `BEGIN IMMEDIATE … COMMIT`.
   - `0` con tablas ya creadas (bases escritas por versiones anteriores de este binario) → validar con `CREATE … IF NOT EXISTS` y fijar `1`.
   - `1` → abrir sin cambios.
   - `> 1` → `PersistenceViolation::SchemaVersionUnsupported { found }`. No se escribe nada.

**Bases en memoria (efímeros):** `Connection::open_in_memory()`, `foreign_keys=ON`. `synchronous=OFF` se admite **solo** en memoria. Sin archivos en disco.

**Proyecciones autorizadas nuevas** (parametrizadas, sin `SELECT *`):
- Carga de perfiles (lectura completa **intencionada**: tabla de pocas filas, recorrido por la clave primaria):
  `SELECT profile_id, compartment_code, privacy_level_code, created_at_epoch_s FROM profiles ORDER BY profile_id;`
  Una fila con `compartment_code ≠ 1` en la base persistente → `PersistenceViolation::ForeignCompartmentRow { profile_id }`, y no se carga nada.
- Retención de muestras (D27), tras cada inserción cuyo `sample_id` sea múltiplo de 1 000:
  `DELETE FROM perf_baseline_samples WHERE sample_id <= (SELECT MAX(sample_id) FROM perf_baseline_samples) - ?1;` con `?1 = MAX_PERSISTED_PERF_SAMPLES − 1 000`: se conservan 9 000 filas para que, entre dos podas, la tabla nunca supere 10 000. Ajustado en la ejecución del paso 2: con `?1 = MAX` el tope se superaba en hasta 999 filas. El borrado va por `sample_id` (clave primaria `INTEGER PRIMARY KEY` = rowid).
- Se mantienen las dos proyecciones de `contract.md` (permiso por sitio y línea base) sin cambios.

**Esquema:** `schema_v1.sql` **no cambia**. Solo se documenta en su cabecera `PRAGMA user_version = 1`.

## 4. CONTRATO DE ERRORES (aditivo)

```rust
// contracts/galapagos_core_v1.rs — variante nueva de GalapagosError
/// Δ7: la persistencia no puede garantizar su contrato (WAL, versión de esquema o compartimentación)
PersistenceInvariantViolated { reason: PersistenceViolation },

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
```
`Display` se implementa en el mismo estilo que las variantes existentes. Los fallos de SQLite siguen siendo `StorageFailure { sqlite_extended_code }`.

## 5. ANTI-BLOAT FENCE

- **Entra:** nada externo. Solo `rusqlite` (ya dependencia) y `std`.
- **Descartado:**
  - Un ORM o un framework de migraciones: con una sola versión de esquema basta con `user_version`.
  - Una base por cada perfil `Personal`: multiplica archivos y conexiones sin un requisito que lo pida.
  - Cifrar la base (SQLCipher): dependencia nueva, fuera de P0.
  - Respaldo en `%TEMP%`: datos del usuario en un directorio que se purga.
  - Guardar la ruta de red en una columna nueva: necesita esquema v2 (Cubo B).
- **Prohibido:**
  - Degradar a memoria en silencio si la base en disco falla.
  - Escribir filas de perfiles efímeros en disco.
  - `synchronous=OFF` o `journal_mode=OFF` en disco.

## 6. Plan de ejecución (un `TARGET_FILE` por paso)

| Paso | TARGET_FILE | Cubre |
|---|---|---|
| 1 | `contracts/galapagos_core_v1.rs` (`PersistedProfileRow`, `SCHEMA_VERSION`, `MAX_PERSISTED_PERF_SAMPLES`, `PersistenceViolation`, variante y `Display`) | §2, §4 |
| 2 | `src/storage/mod.rs`: comprobación de WAL, `user_version` en transacción, `list_profiles`, retención de muestras | W-6, W-7, W-8 |
| 3 | `src/coordinator.rs`: base por compartimento, `load_persisted_profiles`, rechazo de `CustomProxy` persistente, marcas de tiempo | W-2, W-3, W-5, W-9 |
| 4 | `src/main.rs`: ruta de datos, apertura WAL (fatal si falla), orden W-I3, reutilización del perfil, `--smoke-test` en memoria | W-1, W-4, D28 |
| 5 | `tests/persistence_wal.rs` (nuevo): integración sobre un archivo temporal | todo |
| 6 | `contracts/schema_v1.sql` (cabecera) y `CONTRACT_BACKLOG.md` (Cubo B) | documentación |

**Pruebas:**
- **Unitarias:** cada paso añade las suyas.
- **Integración (paso 5)**, sobre un `personal.db` en una carpeta temporal:
  - Dos «arranques» seguidos (dos `Coordinator` que abren el mismo archivo) → el segundo carga el perfil 1, no lo reinserta y asigna el siguiente id a partir de 2.
  - Un perfil `TemporaryPrivate` con permisos → **0 filas** suyas en el archivo (se comprueba abriéndolo con una conexión aparte).
  - `journal_mode` del archivo = `wal`.
  - `user_version = 2` forzado → `SchemaVersionUnsupported { found: 2 }`.
  - `CustomProxy` en `Personal` → `RoutePolicyNotPersistable`.
  - 10 500 muestras → como máximo 10 000 filas.
- **Smoke test real:** arrancar el binario dos veces y comprobar que no aparece el diálogo fatal. Requiere al Director: hay ventana.

## 7. Diferido (Cubo B → `CONTRACT_BACKLOG.md` al congelar)

| Código | Prioridad | Descripción |
|---|---|---|
| `PERS-01` | P1 | Esquema v2: persistir `NetworkRoutePolicy` de los perfiles `Personal` y protocolo de migraciones (`user_version` 1→2 en `BEGIN EXCLUSIVE`). |
| `PERS-02` | P1 | Instancia única del Coordinador (mutex con nombre) para que dos procesos no compartan `personal.db`. |
| `PERS-03` | P2 | `verified_artifacts` con upsert al reaplicar un artefacto (tras el verificador Ed25519 real). |
| `PERS-04` | P2 | `StorageEngine` sin `lock().unwrap()`: un `Mutex` envenenado debe dar `StorageFailure`, no un pánico. |

## 8. Verificación y riesgos

- **Datos existentes:** si el Director ya tiene un `personal.db` de una prueba manual, se abre con `user_version = 0` y tablas, y se valida y fija a 1. Si contiene filas efímeras, el arranque falla con `ForeignCompartmentRow`, que es el comportamiento intencionado (no se borra nada automáticamente).
- **Cambio visible:** al reiniciar, los permisos por sitio del perfil `Personal` se conservan. Los efímeros siguen desapareciendo.
- Hasta ejecutar el smoke test real del paso 6, el estado es **NO INTEGRADO**.

## 8b. Estado de ejecución (2026-10-26)

| Paso | Estado |
|---|---|
| 1 | HECHO: `galapagos_core_v1.rs` (tipos, constantes y error nuevos). |
| 2 | HECHO: `storage/mod.rs` (WAL comprobado, `user_version`, `list_profiles`, retención). Ajuste: cada poda conserva `MAX − 1 000` filas (ver §3). El DDL se lee de `schema_v1.sql` con `include_str!`. |
| 3 | HECHO: `coordinator.rs` (una base por compartimento, `load_persisted_profiles`, `lowest_profile_id`, `storage_for_profile` / `storage_for_tab`, D26, fechas reales). |
| 4 | HECHO: `main.rs` (ruta D23, WAL fatal si falla, orden W-I3, reutilización del perfil, D28). |
| 5 | HECHO: `tests/persistence_wal.rs`, 6/6 sobre un archivo real. |
| 6 | HECHO: cabecera de `schema_v1.sql`. **Smoke test con el binario real en modo `--headless`**, con `%LOCALAPPDATA%` aislado en una carpeta temporal: 2 arranques con código 0, 1 perfil (id 1, Personal, Daily, fecha real), `journal_mode = wal`, `user_version = 1`, sin WARN/ERROR/CRITICAL en `app_trace.log`. |
| Pendiente | **Prueba del Director con ventana** (guía abajo). Hasta entonces: INTEGRADO en headless, NO verificado con interfaz. |

### Guía de la prueba manual (Director)

1. Cerrar cualquier Galapagos abierto. Ejecutar: `D:\App\Browser-SaveNav\target\debug\galapagos-browser.exe`
2. Comprobar que la ventana abre en `galapagos://dashboard` **sin** el diálogo «Error al abrir la base del perfil Personal».
3. Cerrar la ventana y volver a abrirla. Tampoco debe aparecer el diálogo «Error al cargar o crear el perfil predeterminado» (era el fallo del segundo arranque).
4. Comprobar que existe `%LOCALAPPDATA%\GalapagosBrowser\profiles\personal.db`.
5. Si algo falla: enviar `%LOCALAPPDATA%\GalapagosBrowser\logs\app_trace.log` y, si existe, `coordinator_error.log`.

## 9. Decisiones para el Director

| # | Decisión | Recomendación |
|---|---|---|
| D23 | Ubicación `%LOCALAPPDATA%\GalapagosBrowser\profiles\personal.db`; sin respaldo en `%TEMP%` (fatal si no se puede abrir) | Sí |
| D24 | Una base en disco para todos los perfiles `Personal` y una base `:memory:` por cada perfil efímero | Sí |
| D25 | Al arrancar se cargan los perfiles persistidos y se reutiliza el `Personal` de menor id | Sí |
| D26 | `CustomProxy` en un perfil `Personal` se rechaza hasta el esquema v2 (no se guarda a medias) | Sí |
| D27 | Muestras de rendimiento de `Personal` en disco con tope de 10 000 filas; las de efímeros, en su base en memoria | Sí |
| D28 | `--smoke-test` usa una base en memoria | Sí |

**Contrato Δ7 congelado (2026-10-26): D23–D28 aceptadas. Cubo B movido a `CONTRACT_BACKLOG.md`. Listo para /znve-execute.**
