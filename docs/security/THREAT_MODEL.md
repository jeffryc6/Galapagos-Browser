# THREAT MODEL — Galapagos Browser v0.1

**Estado:** Modelo Formal de Amenazas y Mitigaciones STRIDE (Fase C · Tarea 10)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Límites de Confianza:** [docs/architecture/SYSTEM_ARCHITECTURE.md](file:///d:/App/Browser-SaveNav/docs/architecture/SYSTEM_ARCHITECTURE.md)  

---

## 1. Inventario de Activos y Adversarios

### 1.1. Activos Críticos del Sistema
1. **Credenciales y Tokens de Sesión:** Almacenamiento de cookies y sesiones web del usuario.
2. **Integridad del Sistema Operativo Anfitrión:** Evitar que código web malicioso ejecute comandos o persista binarios fuera del navegador.
3. **Privacidad de Navegación e Historial:** Impedir el rastreo cruzado entre perfiles y la persistencia accidental en sesiones efímeras.
4. **Disponibilidad de Recursos (CPU y RAM):** Proteger el hilo principal de la UI contra bloqueos provocados por scripts abusivos o fugas de memoria.

### 1.2. Perfiles de Adversarios
* **Sitios Web Maliciosos:** Entidades que intentan explotar fallos en el motor Servo (exploits de layout/DOM), inyectar mineros de criptomonedas o descargar ejecutables sin consentimiento.
* **Redes de Vigilancia y Rastreadores (Trackers):** Redes de publicidad que correlacionan el comportamiento del usuario a través de huellas de navegador (`fingerprinting`) y cookies de terceros.
* **Adversarios de Red (MitM):** Entidades con capacidad de interceptar o modificar tráfico en tránsito en redes Wi-Fi públicas o enlaces no cifrados.

---

## 2. Superficies de Ataque

1. **El Motor Web de Renderizado (Servo):** Analizador sintáctico de HTML/CSS, motor de ejecución JavaScript y decodificadores de imágenes/fuentes.
2. **El Canal IPC (Coordinador ⇄ Worker):** Mensajes de comunicación entre procesos que cruzan la frontera de AppContainer.
3. **El Sistema de Archivos y Persistencia:** Directorio de perfiles y base de datos SQLite en disco.
4. **Flujo de Descargas y Esquemas de URL:** Tratamiento de URLs malformadas y enlaces hacia esquemas peligrosos (`javascript:`, `file:`, etc.).

---

## 3. Matriz de Amenazas STRIDE y Mitigaciones Verificables

| Categoría STRIDE | Amenaza Identificada | Superficie de Ataque | Control de Mitigación ZNVE | Prueba / Verificación Asociada |
|---|---|---|---|---|
| **Spoofing** (Suplantación) | Un script web hostil simula ser el Trusted Coordinator e inyecta comandos privilegiados. | Canal IPC (`UiToCoordinatorCmd`). | Desacoplamiento unidireccional: el worker web **solo** puede emitir `ContentToCoordinatorIpc`, jamás `UiToCoordinatorCmd`. | `test_zero_silent_suppression_audit` en [src/coordinator.rs](file:///d:/App/Browser-SaveNav/src/coordinator.rs). |
| **Tampering** (Manipulación) | Un atacante manipula una lista de amenazas para desactivar bloqueos de seguridad. | Actualizaciones de listas de reglas. | Verificación criptográfica obligatoria de firmas Ed25519 y hash SHA-256 (`SEC-03`) antes de registrar en `verified_artifacts`. | `insert_verified_artifact` en [src/storage/mod.rs](file:///d:/App/Browser-SaveNav/src/storage/mod.rs). |
| **Repudiation** (Repudio) | Un componente niega haber asignado o revocado un permiso sensible a un origen web. | Permisos por sitio (`site_permissions`). | Tabla SQLite estricta `WITHOUT ROWID` con timestamps unívocos y claves primarias compuestas (`profile_id`, `origin_host`, `capability_code`). | `test_storage_lifecycle_and_projections` en [src/storage/mod.rs](file:///d:/App/Browser-SaveNav/src/storage/mod.rs). |
| **Information Disclosure** (Fuga de Información) | Cookies y almacenamiento local de una sesión efímera permanecen en disco tras cerrar la pestaña. | Persistencia local (`PRIV-01` / `PRIV-02`). | Uso exclusivo de `:memory:` sin archivos `.wal`/`.shm` en disco para `TemporaryPrivate`; purga atómica de buffers en `destroy_ephemeral_profile`. | `test_ephemeral_profile_destruction` en [src/coordinator.rs](file:///d:/App/Browser-SaveNav/src/coordinator.rs). |
| **Denial of Service** (Denegación de Servicio) | Un script web ejecuta bucles infinitos o asigna gigabytes de memoria para congelar el sistema. | Proceso de renderizado web. | Enjaulado en **Windows Job Object** con cuota rígida `ResourceQuota` (máx. 512 MiB RAM y rate limit de CPU) y límite IPC de 1 MiB. | `test_resource_quota_invariants` en [contracts/galapagos_core_v1.rs](file:///d:/App/Browser-SaveNav/contracts/galapagos_core_v1.rs). |
| **Elevation of Privilege** (Escalada de Privilegios) | Un exploit de día cero en Servo logra ejecución remota de código (RCE) dentro del proceso worker. | Runtime de Servo. | Confinamiento en **Windows AppContainer Strict** sin acceso al sistema de archivos, registro ni tokens de usuario estándar. | ADR-002 y verificación en [docs/research/SERVO_WINDOWS_FEASIBILITY.md](file:///d:/App/Browser-SaveNav/docs/research/SERVO_WINDOWS_FEASIBILITY.md). |

---

## 4. Regla de Verificación Inviolable

> **Directiva ZNVE:** Ninguna amenaza se considera mitigada por mera afirmación documental. Cada control listado cuenta con una estructura tipada inmutable en `contracts/`, una implementación en `src/` y una prueba automatizada en la suite de `cargo test`.
