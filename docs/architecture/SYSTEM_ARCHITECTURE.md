# SYSTEM ARCHITECTURE — Galapagos Browser v0.1

**Estado:** Documento Fundacional de Arquitectura (Fase A · Tarea 1)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Plataforma Objetivo:** Windows 10/11 64-bit nativo (Rust + Servo Embedder API + Win32/WinUI 3)  
**Licencia:** Apache-2.0 compatible con MPL-2.0  

---

## 1. Visión y Topología de Ejecución

Galapagos se concibe como un **monolito modular de escritorio con aislamiento selectivo de procesos**, desacoplando radicalmente la interfaz de usuario y la coordinación de confianza de los motores de renderizado web hostiles.

```
                                INTERNET
                                   │
                      ┌────────────▼────────────┐
                      │ GALAPAGOS OCEAN DEFENSE │
                      │  (Políticas defensivas) │
                      │ Threat Intelligence     │
                      │ Request Guard           │
                      │ Behavior Sentinel       │
                      │ Anti-Tracking Shield    │
                      │ Payload & Download Guard│
                      │ Exploit Containment     │
                      └────────────┬────────────┘
                                   │
                     ┌─────────────▼─────────────┐
                     │       GALAPAGOS CORE      │
                     │   (Trusted Coordinator)   │
                     │ Lifecycle & State Machine │
                     │ Policy Coordinator        │
                     │ Profile & Session Manager │
                     │ Tab & Window Manager      │
                     │ Event Dispatcher          │
                     │ Resource Budget Manager   │
                     └─────────────┬─────────────┘
                                   │
                  ┌────────────────┼────────────────┐
                  │                │                │
            ┌─────▼─────┐    ┌─────▼─────┐    ┌─────▼─────┐
            │ Security  │    │  Privacy  │    │Performance│
            │  Island   │    │  Island   │    │  Island   │
            └───────────┘    └───────────┘    └───────────┘
            ┌───────────┐    ┌───────────┐    ┌───────────┐
            │ Research  │    │  Freedom  │    │Connectivity
            │  Island   │    │  Island   │    │  Island   │
            └───────────┘    └───────────┘    └───────────┘
                                   │
                       ┌───────────▼───────────┐
                       │ WINDOWS NATIVE LAYER  │
                       │ Win32 / HWND Host     │
                       └───────────┬───────────┘
                                   │ IPC (Canal Restringido)
                       ┌───────────▼───────────┐
                       │   SERVO ENGINE WORKER │
                       │ AppContainer Strict   │
                       │ DOM / Layout / JS     │
                       └───────────────────────┘
```

> **Principio de Topología (ZNVE-03):** Las seis islas y Ocean Defense son módulos lógicos en el proceso de coordinación, no microservicios independientes. La única frontera de proceso estricta separa el **Trusted Coordinator** del **Servo Engine Worker**.

---

## 2. Componentes y Responsabilidades

### 2.1. Galapagos Core (Trusted Coordinator)
* **Lifecycle & State Machine:** Controla la máquina de estados estricta del navegador (`Created` → `Initializing` → `Ready` ⇄ `Running` ⇄ `Suspended` → `ShuttingDown` → `Stopped`).
* **Profile & Session Manager:** Garantiza el aislamiento criptográfico y en disco de perfiles (`Personal`, `TemporaryPrivate`, `Laboratory`).
* **Tab & Window Manager:** Mapeo unívoco e inmutable de `ProfileId`, `TabId` y `ProcessId`.
* **Resource Budget Manager:** Aplica límites deterministas de memoria (`working_set`), CPU y cuotas IPC mediante `ResourceQuota`.
* **Event Dispatcher:** Despacho tipado de eventos en memoria sin sondeos continuos ni copias masivas de datos.

### 2.2. Las Seis Islas Especializadas
| Isla | Responsabilidad Primaria | Frontera de Aislamiento |
|---|---|---|
| **Security Island** | Gobernanza de límites de confianza, mitigaciones Win32 y recuperación de fallos. | Configuración de AppContainer, Job Objects y Restricted Tokens. |
| **Privacy Island** | Partición de cookies, almacenamiento web y ciclo de vida de sesiones efímeras. | Base de datos `:memory:` para efímeros; purga de buffers al cierre. |
| **Performance Island** | Instrumentación de latencia, muestras de CPU y memoria privada. | Telemetría determinista (`PerformanceSampleDto`) almacenada en SQLite. |
| **Research Island** | Entorno de diagnóstico bajo demanda (Network, Storage, Security Lab). | Activación explícita; sin recopilación rutinaria en sesiones diarias. |
| **Freedom Island** | Gestión de excepciones justificadas y consentimiento explícito del usuario. | Reglas de revocabilidad inmediata en `site_permissions`. |
| **Connectivity Island** | Enrutamiento de red verificado (`DirectSystem` / `CustomProxy`). | Validación estricta RFC 1123 en host y aislamiento de fugas WebRTC. |

### 2.3. Galapagos Ocean Defense
Capa transversal de políticas de seguridad basada en el principio **Shared Signals, Specialized Decisions**:
* **Threat Intelligence:** Base de datos local de firmas SHA-256 / Ed25519 de listas de filtrado y artefactos verificados.
* **Request Guard:** Interceptación síncrona en memoria antes de despachar solicitudes al motor de red.
* **Behavior Sentinel:** Detección de patrones abusivos locales sin escaneo permanente ni agentes de IA residentes.
* **Anti-Tracking Shield:** Bloqueo de rastreadores conocidos y aislamiento de contexto third-party.
* **Payload & Download Guard:** Inspección de cabeceras, procedencia y delegación a zonas de seguridad de Windows.
* **Exploit Containment:** Ejecución del motor web con permisos mínimos para neutralizar vulnerabilidades de día cero.

---

## 3. Límites de Confianza y Modelo de Seguridad (Security Boundaries)

### 3.1. Niveles de Privilegio
1. **Nivel Confiable (Trusted Coordinator / UI Thread):**
   * Corre con el token de usuario estándar (Medium Integrity Level).
   * Tiene prohibido ejecutar parsers web (HTML/CSS), motores JS o decodificadores de imágenes no confiables.
   * Maneja el almacenamiento persistente (`schema_v1.sql`) y la ventana nativa Win32.
2. **Nivel Hostil (Servo Web Engine Worker):**
   * Encapsulado en **AppContainer Strict** + **Windows Job Object**.
   * Sin acceso directo al sistema de archivos, registro o red externa sin mediación del Coordinador.
   * Límites de Job Object: cuota estricta de memoria de trabajo (`max_working_set_bytes`) y límite de ciclos de CPU (`cpu_rate_limit_basis_points`).

### 3.2. Frontera IPC Determinista
* Toda comunicación entre el Coordinador y el proceso Servo ocurre a través de canales IPC con mensajes estrictamente tipados (`UiToCoordinatorCmd` y `ContentToCoordinatorIpc`).
* **Límites inmutables de frontera:**
  * Carga útil máxima por mensaje: **1 048 576 bytes (1 MiB)**.
  * Tiempo límite de respuesta (timeout): **5 000 ms**.
  * Longitud máxima de URL: **2048 bytes**.
  * Esquemas permitidos: `https://`, `http://`, `about:blank`.

---

## 4. Flujo de Navegación Determinista

```
[Usuario ingresa URL / Clic en enlace]
                  │
                  ▼
         [Win32 Native UI]
                  │ UiToCoordinatorCmd::Navigate
                  ▼
      [Coordinator / Core]
                  │
                  ├── 1. Validar esquema y longitud (max 2048 bytes, https/http/about:blank)
                  ├── 2. Verificar permisos de origen en site_permissions (SQLite WAL)
                  │
                  ▼
         [Ocean Defense]
                  │ Request Guard evalúa URL y contexto
                  ├── Allow ──────────────────────┐
                  ├── Block ────────► Notifica UI y aborta
                  ├── RequireConfirmation ─► Diálogo nativo modal
                  └── Unavailable ──► Falla segura según política
                                                  │
                                                  ▼
                                       [WebEnginePort Trait]
                                                  │ dispatch_navigation
                                                  ▼
                                     [Servo Worker en AppContainer]
                                                  │ Renderizado DOM/CSS/JS
                                                  ▼
                                       [Win32 HWND Host]
```

---

## 5. Anti-Bloat Fence y Dependencias Autorizadas

### 5.1. Runtimes y Paquetes Prohibidos
* **Prohibido Chromium / CEF / WebView2:** El motor debe ser Servo compilado con Rust.
* **Prohibido Electron:** La UI de escritorio debe ser nativa Win32/WinUI 3 para garantizar costo de memoria nulo en reposo.
* **Prohibidos ORMs dinámicos (`diesel`, `sea-orm`):** SQL parametrizado estricto contra SQLite nativo.
* **Prohibido `anyhow` / `eyre` / `Box<dyn Error>`:** Los errores deben pertenecer al enum cerrado `GalapagosError`.
* **Prohibida telemetría remota o servicios cloud obligatorios:** Filosofía offline-first y telemetría local por excepción.

### 5.2. Stack Aprobado
* **Lenguaje:** Rust 2021 (edición con `#![forbid(unsafe_code)]` en la lógica de negocio; `unsafe` encapsulado exclusivamente en wrappers FFI).
* **Motor Web:** `libservo` (MPL-2.0).
* **Persistencia:** `rusqlite` con `PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;`.
* **Bindings OS:** `windows-sys` para AppContainers, Job Objects y HWND Win32.

---

## 6. Verificación de Conformidad

1. **Compilación e integridad de tipos:** `cargo test` debe certificar todas las invariantes de contratos en [contracts/galapagos_core_v1.rs](file:///d:/App/Browser-SaveNav/contracts/galapagos_core_v1.rs).
2. **Esquema de datos:** [contracts/schema_v1.sql](file:///d:/App/Browser-SaveNav/contracts/schema_v1.sql) debe cargarse sin errores en motores SQLite en modo estricto (`STRICT`, `WITHOUT ROWID`).
3. **Cero fugas de recursos:** Todo perfil efímero (`TemporaryPrivate`) debe purgarse al 100% de la memoria al cerrar sus pestañas asociadas.
