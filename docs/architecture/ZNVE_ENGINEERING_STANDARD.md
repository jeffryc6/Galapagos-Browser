# ZNVE ENGINEERING STANDARD — Galapagos Browser v0.1

**Estado:** Estándar de Ingeniería y Gobernanza Técnica (Fase A · Tarea 2)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Plataforma:** Windows 10/11 64-bit (Rust + Servo + Win32 nativo)  

---

## 1. Axiomas Fundacionales

1. **Axioma 1:** *"Inteligencia pesada en el diseño; huella casi nula en la ejecución."*  
   Todo razonamiento complejo, delimitación de seguridad y diseño de interfaces se resuelve en la fase de especificación y contratos. El código en tiempo de ejecución debe ser predecible, determinista y de mínima huella de CPU y memoria.
2. **Axioma 2:** *"La IA no inventa arquitectura; ejecuta contratos deterministas."*  
   El humano actúa como **Director de Arquitectura**. Ningún agente o desarrollador puede implementar lógica de producción sin un contrato tipado inmutable aprobado previamente en `contracts/`.

---

## 2. Los Doce Principios Obligatorios de Galapagos (ZNVE-01 a ZNVE-12)

| ID | Principio | Criterio de Diseño y Aceptación en Galapagos |
|---|---|---|
| **ZNVE-01** | **Cero ruido operativo** | Prohibidos los sondeos periódicos (`polling`) para chequear estado de pestañas, procesos o red. Se utilizan eventos del sistema operativo (Win32 messages) y canales MPSC/IPC reactivos. |
| **ZNVE-02** | **Contratos primero** | Ningún módulo o endpoint se escribe sin DTOs, esquemas SQL estrictos y enums cerrados de error (ej.: [contracts/galapagos_core_v1.rs](file:///d:/App/Browser-SaveNav/contracts/galapagos_core_v1.rs)). |
| **ZNVE-03** | **Huella mínima** | Cada proceso de contenido tiene asignado un `ResourceQuota` rígido (máx. 512 MiB RAM en perfiles estándar, CPU rate limit via Job Object). |
| **ZNVE-04** | **Dependencias justificadas** | Preferencia absoluta por APIs nativas de Rust y Windows (`windows-sys`). Prohibidas dependencias parásitas que dupliquen capacidades existentes. |
| **ZNVE-05** | **Estado explícito** | Toda pestaña, perfil y sesión responde a una máquina de estados determinista. Prohibidas transiciones implícitas o estados ambiguos. |
| **ZNVE-06** | **Offline-first** | Las decisiones de seguridad (`RequestGuard`), validaciones de URL y partición de perfiles funcionan al 100% sin requerir conectividad a Internet. |
| **ZNVE-07** | **Zero-trust** | El motor Servo y el contenido web se consideran hostiles. Se ejecutan enjaulados en AppContainer y se comunican solo por IPC filtrado. |
| **ZNVE-08** | **Fallos observables** | Prohibido silenciar errores con `catch` vacíos o `unwrap()` a ciegas. Los errores se canalizan tipados en `GalapagosError`. |
| **ZNVE-09** | **Recursos deterministas** | Todo handle Win32, Job Object, conexión SQLite o canal IPC tiene un propietario único y se libera inmediatamente vía RAII (`Drop`). |
| **ZNVE-10** | **Sin capacidades inventadas** | Ninguna función puede asumir capacidades de Servo o Windows que no hayan sido validadas experimentalmente en las pruebas de viabilidad. |
| **ZNVE-11** | **Privacidad diagnóstica** | Telemetría local por excepción. Prohibido registrar URLs completas con query strings, contraseñas, tokens o contenido de formularios. |
| **ZNVE-12** | **Rendimiento medido** | Toda optimización debe contrastarse contra la línea base en `perf_baseline_samples` antes de considerarse aceptada. |

---

## 3. Modos Operativos ZNVE en Galapagos

1. **GREENFIELD (Fase Actual):**
   * Definición exhaustiva de contratos arquitectónicos, modelos de datos y pruebas de viabilidad técnica antes de autorizar código de producción.
2. **IN-FLIGHT:**
   * Las extensiones de funcionalidad no modifican contratos existentes (`contracts/` inmutables); se gestionan mediante deltas auditados (`/znve-contract --delta`) clasificando en Cubo A (etapa activa) y Cubo B (diferido a `contracts/CONTRACT_BACKLOG.md`).
3. **CRISIS / HOTFIX:**
   * Ante fallos en runtime, protocolo estricto `/znve-triage` (diagnóstico de causa raíz sin alterar contratos) seguido de `/znve-hotfix` (parche atómico acotado a un único archivo con test de regresión obligatorio).
4. **HARDENING:**
   * Auditoría periódica de handles huérfanos, descriptores, contención en UI Thread y fugas de memoria con `/znve-audit`.

---

## 4. Anti-Bloat Fence y Criterios de Aceptación de Dependencias

Para admitir cualquier crate externo en `Cargo.toml`, debe cumplirse la siguiente regla de exclusión estricta:

```
¿La funcionalidad puede implementarse con Rust std o windows-sys?
       │
       ├── SÍ ──► RECHAZADO: Usar API nativa.
       │
       └── NO ──► ¿Aporta peso desproporcionado, runtime dinámico o superficie de ataque?
                     │
                     ├── SÍ ──► RECHAZADO: Diseñar solución acotada o diferir a CONTRACT_BACKLOG.
                     │
                     └── NO ──► Documentar justificación en la Anti-Bloat Fence del contrato
                                antes de incluir el crate.
```

### Tabla de Runtimes y Librerías Prohibidas:
* **Chromium / CEF / WebView2:** Prohibido por violar la tesis central del proyecto (Servo nativo).
* **Electron / Node.js:** Prohibido en el proceso privilegiado; la UI es nativa Win32/WinUI 3.
* **ORMs pesados (`diesel`, `sea-orm`):** Prohibidos; persistencia mediante SQL parametrizado nativo en `rusqlite`.
* **Manejadores dinámicos de errores (`anyhow`, `eyre`):** Prohibidos en el dominio central; se usan enums cerrados sin asignaciones dinámicas.
* **SDKs de telemetría remota comercial:** Prohibidos por diseño.

---

## 5. Regla de Implementación de Código (Ejecución Atómica)

Todo cambio de código ejecutado bajo `/znve-execute` debe cumplir:
1. **Un solo `TARGET_FILE` por paso.**
2. **Cero dependencias no autorizadas.**
3. **Verificación en dos pasos:**
   * Primero: ejecutar de forma aislada el test del `TARGET_FILE`.
   * Segundo: ejecutar la suite completa (`cargo test -- --quiet`) confirmando 100% de éxito y cero regresiones.
