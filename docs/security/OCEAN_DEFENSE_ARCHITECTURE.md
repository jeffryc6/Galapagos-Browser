# OCEAN DEFENSE ARCHITECTURE — Galapagos Browser v0.1

**Estado:** Especificación de la Capa Defensiva Transversal (Fase C · Tarea 8)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Implementación Base:** [src/defense/request_guard.rs](file:///d:/App/Browser-SaveNav/src/defense/request_guard.rs)  

---

## 1. Misión y Filosofía de Diseño

Galapagos Ocean Defense no es un antivirus monolítico ni un agente de inteligencia artificial en ejecución continua. Es una **capa transversal de políticas defensivas deterministas** diseñada bajo el principio:

> **Shared Signals, Specialized Decisions:** Los subsistemas comparten señales de seguridad mínimas y no sensibles; cada componente evalúa únicamente la frontera que le corresponde, sin duplicar trabajo ni ralentizar la navegación.

```
                              INTERNET (Tráfico Externo)
                                          │
    ┌─────────────────────────────────────▼─────────────────────────────────────┐
    │                         GALAPAGOS OCEAN DEFENSE                           │
    │                                                                           │
    │  ┌──────────────────────┐  ┌──────────────────────┐  ┌─────────────────┐  │
    │  │ Threat Intelligence  │  │    Request Guard     │  │Behavior Sentinel│  │
    │  │ Firmas y Reputación  │  │ Interceptación Local │  │ Señales Anómalas│  │
    │  └──────────┬───────────┘  └──────────┬───────────┘  └────────┬────────┘  │
    │             │                         │                       │           │
    │             └──────────────────┐      │      ┌────────────────┘           │
    │                                ▼      ▼      ▼                            │
    │                         ┌───────────────────────────┐                     │
    │                         │ Decision Pipeline Engine  │                     │
    │                         │ (Allow/Block/Confirm/...) │                     │
    │                         └─────────────┬─────────────┘                     │
    │                                       │                                   │
    │  ┌──────────────────────┐             │              ┌─────────────────┐  │
    │  │ Anti-Tracking Shield │◄────────────┼─────────────►│ Payload Guard   │  │
    │  │ Partición & Cookies  │             │              │ Descargas Win32 │  │
    │  └──────────────────────┘             ▼              └─────────────────┘  │
    │                             Exploit Containment                           │
    │                      (AppContainer & Job Object Limits)                   │
    └───────────────────────────────────────┬───────────────────────────────────┘
                                            │
                                            ▼
                           SERVO WEB ENGINE (Worker enjaulado)
```

---

## 2. Los Seis Pilares de Ocean Defense

### 2.1. Threat Intelligence
* **Responsabilidad:** Consultar firmas criptográficas, dominios de phishing y malware conocidos.
* **Estrategia ZNVE:** Operación 100% local basada en la tabla `verified_artifacts` (SQLite). Las listas de filtrado se descargan firmadas con Ed25519; no se consulta la reputación de cada URL a servidores remotos para no filtrar el historial.

### 2.2. Request Guard
* **Responsabilidad:** Interceptación síncrona en memoria antes de resolver conexiones de red o delegar navegación al motor.
* **Estrategia ZNVE:** Evaluación sin asignaciones pesadas, verificando esquemas (`https`, `http`, `about:blank`), longitud máxima (2048 bytes) y listas de exclusión en tiempo $O(1)$.

### 2.3. Behavior Sentinel
* **Responsabilidad:** Detección de patrones locales abusivos (bucles de redirección rápida, inundación de ventanas emergentes o intentos repetidos de acceso a APIs de sensores).
* **Estrategia ZNVE:** Sin sondeos continuos ni escaneos en reposo; reacciona a contadores de frecuencia impulsados por eventos.

### 2.4. Anti-Tracking Shield
* **Responsabilidad:** Bloqueo de balizas de rastreo (`web beacons`), scripts de analítica de terceros y aislamiento de almacenamiento web.
* **Estrategia ZNVE:** Partición estricta de cookies basada en `ProfileId` y bloqueo determinista de hosts en peticiones categorizadas como `is_third_party: true`.

### 2.5. Payload & Download Guard
* **Responsabilidad:** Inspección de procedencia y tipo MIME de descargas.
* **Estrategia ZNVE:** Barrera modal obligatoria (`RequireConfirmation`) para todo archivo ejecutable (`.exe`, `.msi`, `.bat`, etc.) e integración con el *Mark of the Web* (Zone.Identifier) de Windows.

### 2.6. Exploit Containment
* **Responsabilidad:** Contener el blast radius ante vulnerabilidades desconocidas del motor de renderizado o decodificadores de medios.
* **Estrategia ZNVE:** El contenido web corre en un proceso independiente con AppContainer Strict, token restringido y Job Object que limita la memoria de trabajo física (`ResourceQuota`).

---

## 3. Límites e Invariantes Críticas

1. **No prometer inspección universal de tráfico:** Ocean Defense no es un proxy TLS Man-in-the-Middle ni un firewall de paquetes; sus políticas actúan en los puntos de anclaje que expone el embedder de Servo y el bucle Win32.
2. **Cero persistencia no autorizada:** Ocean Defense no registra los cuerpos de las peticiones HTTP ni las credenciales transmitidas.
3. **Fallo Seguro (`Fail-Secure`):** Si un subcomponente de defensa entra en pánico o pierde acceso a sus tablas de reglas, la evaluación produce `DefenseDecision::Unavailable`, impidiendo la navegación a destinos no certificados.
