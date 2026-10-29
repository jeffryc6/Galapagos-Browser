# PERSISTENCE DESIGN — Galapagos Browser v0.1

**Estado:** Diseño del Modelo de Persistencia y Almacenamiento Local (Fase D · Tarea 13)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Esquema DDL Estricto:** [contracts/schema_v1.sql](file:///d:/App/Browser-SaveNav/contracts/schema_v1.sql)  
**Driver en Código:** [src/storage/mod.rs](file:///d:/App/Browser-SaveNav/src/storage/mod.rs)  

---

## 1. Arquitectura de Almacenamiento Híbrido

Galapagos implementa una estrategia de persistencia dual desacoplada por tipo de compartimento:

```
                              COORDINADOR DE CONFIANZA
                                         │
                   ┌─────────────────────┴─────────────────────┐
                   │                                           │
       Perfil Personal (`Personal`)        Perfil Efímero (`TemporaryPrivate`)
                   │                                           │
                   ▼                                           ▼
       Almacenamiento Persistente en Disco         Almacenamiento Volátil en RAM
      [AppData/Local/Galapagos/personal.db]            [SQLite URI: ":memory:"]
                   │                                           │
        ├── PRAGMA journal_mode=WAL;                ├── Cero archivos .wal / .shm
        ├── PRAGMA synchronous=NORMAL;              ├── Cero escrituras a disco
        └── PRAGMA foreign_keys=ON;                 └── Purga atómica al cerrar sesión
```

---

## 2. Garantías de Transaccionalidad y Escrituras Atómicas

1. **Write-Ahead Logging (WAL):**
   * Las lecturas no bloquean las escrituras y las escrituras no bloquean las lecturas.
   * La verificación de permisos en ruta caliente (`site_permissions`) se ejecuta de forma concurrente sin contención en el hilo principal.
2. **Escrituras Atómicas en Transacciones Explícitas:**
   * Toda mutación de estado (asignación de permisos, actualización de perfiles o inserción de telemetría) se realiza mediante transacciones parametrizadas inmediatas (`BEGIN IMMEDIATE ... COMMIT`).
3. **Recuperación Automática ante Cierres Abruptos:**
   * En caso de corte de energía o crash del sistema operativo, el motor SQLite recupera el estado consistente reejecutando o truncando el archivo de log `.wal` al reiniciar la conexión. Prohibido el uso de modos peligrosos (`journal_mode=OFF` o `synchronous=OFF` en disco).

---

## 3. Modelo de Migraciones Controladas

El versionado del esquema se administra mediante el pragma nativo `user_version`:

```sql
PRAGMA user_version; -- Retorna la versión entera del esquema activo (v1 = 1)
```

### Protocolo de Migración:
1. Al abrir la conexión persistente, `StorageEngine` inspecciona `user_version`.
2. Si `user_version < SCHEMA_VERSION_TARGET`:
   * Se inicia una transacción exclusiva (`BEGIN EXCLUSIVE`).
   * Se aplican secuencialmente los scripts de migración (`migrations/vX_to_vY.sql`).
   * Se actualiza `PRAGMA user_version = Y;`.
   * Se confirma la transacción (`COMMIT`).
3. Si la migración falla, la transacción hace rollback completo y el Coordinador aborta con `GalapagosError::StorageFailure`, impidiendo corromper los datos del usuario.

---

## 4. Proyecciones Autorizadas y Prohibición de ORMs

> **Regla ZNVE-04 (Anti-Bloat Fence):** Queda estrictamente prohibida la inclusión de ORMs pesados (`diesel`, `sea-orm`). Toda consulta se realiza mediante sentencias SQL estáticas parametrizadas.

### Proyecciones Estrictas:
* **Ruta Caliente de Permisos:**
  ```sql
  SELECT grant_state FROM site_permissions 
  WHERE profile_id = ?1 AND origin_host = ?2 AND capability_code = ?3 
  LIMIT 1;
  ```
* **Línea Base de Telemetría:**
  ```sql
  SELECT timestamp_ns, working_set_bytes, cpu_delta_us, sec_overhead_us 
  FROM perf_baseline_samples 
  WHERE tab_id = ?1 
  ORDER BY timestamp_ns DESC 
  LIMIT ?2;
  ```
* **Prohibición de `SELECT *`:** Cada consulta proyecta únicamente los campos necesarios para poblar DTOs inmutables, maximizando la eficiencia de la caché de páginas.
