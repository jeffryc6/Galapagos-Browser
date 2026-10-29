# **GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE)**

**Modo activo:** GREENFIELD — diseño arquitectónico desde cero.  
**Fase:** especificación y contratos; todavía no se autoriza escribir código de producción.  
**Plataforma inicial:** Windows Desktop, con Servo como motor web y Rust como lenguaje de integración principal.  
**Responsable de las decisiones arquitectónicas:** el usuario, como Director de Arquitectura.  
**Principio rector:** *inteligencia pesada en el diseño; huella casi nula en la ejecución.*

## **BLOQUE 1\. SYSTEM BLUEPRINT / ESPECIFICACIÓN**

### **1.1. Objetivo y límites**

Diseñar **Galapagos**, un navegador de código abierto que combine rendimiento, privacidad, seguridad, navegación libre y herramientas avanzadas de investigación, bajo una arquitectura modular inspirada en el ecosistema de las islas Galápagos.

La arquitectura tendrá tres niveles:

1. **Galapagos Core:** núcleo de coordinación, ciclo de vida, políticas y estado.  
2. **Las seis islas:** subsistemas especializados que implementan las capacidades del navegador.  
3. **Galapagos Ocean Defense:** capa defensiva transversal que protege las fronteras entre Internet, el motor Servo, las islas y el núcleo.

La arquitectura debe cumplir ZNVE desde su diseño, no intentar corregir posteriormente el consumo excesivo de recursos, las dependencias innecesarias o la falta de contratos.

**Fuera del alcance de esta fase:**

* Implementar código de producción.  
* Afirmar capacidades de Servo que todavía no se hayan verificado.  
* Desarrollar un antivirus completo.  
* Incorporar IA residente que analice continuamente la navegación.  
* Crear un sistema propio de VPN o una infraestructura de anonimato.  
* Garantizar anonimato absoluto, protección total contra fingerprinting o inmunidad frente a exploits.  
* Incorporar extensiones, sincronización en la nube o servicios remotos sin justificar primero su necesidad.

Estas exclusiones no impiden evaluar las capacidades posteriormente; evitan introducirlas prematuramente en el MVP.

### **1.2. Arquitectura conceptual**

                          INTERNET  
                              │  
                 ┌────────────▼────────────┐  
                 │ GALAPAGOS OCEAN DEFENSE │  
                 │                          │  
                 │ Threat Intelligence      │  
                 │ Request Guard            │  
                 │ Behavior Sentinel        │  
                 │ Anti-Tracking Shield     │  
                 │ Payload & Download Guard │  
                 │ Exploit Containment      │  
                 └────────────┬─────────────┘  
                              │  
                ┌─────────────▼─────────────┐  
                │       GALAPAGOS CORE      │  
                │                           │  
                │ Lifecycle & State         │  
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
                  │ UI · Files · OS APIs   │  
                  └───────────┬───────────┘  
                              │  
                  ┌───────────▼───────────┐  
                  │     SERVO ENGINE      │  
                  │ Rendering · Web APIs  │  
                  └───────────────────────┘

**Nota de arquitectura:** el diagrama representa responsabilidades lógicas, no procesos independientes. No se creará un proceso por isla por defecto. La separación entre procesos, hilos y módulos se determinará mediante requisitos de aislamiento, APIs disponibles en Servo y mediciones de rendimiento.

La integración exacta entre Servo, la interfaz nativa y Ocean Defense está sujeta a una prueba de viabilidad técnica. No se asumirá que todas las solicitudes de red, descargas o señales de comportamiento son interceptables hasta verificarlo en la versión concreta del motor elegida.

### **1.3. Principios arquitectónicos obligatorios**

| ID | Principio | Contrato arquitectónico |
| ----- | ----- | ----- |
| ZNVE-01 | Cero ruido operativo | No habrá sondeos periódicos cuando los eventos del sistema permitan reaccionar a cambios reales. |
| ZNVE-02 | Contratos primero | Ningún módulo de producción se implementará sin entradas, salidas, estados, errores y permisos definidos. |
| ZNVE-03 | Huella mínima | Cada módulo tendrá un presupuesto de CPU, memoria, actividad de red y almacenamiento. |
| ZNVE-04 | Dependencias justificadas | Se preferirán las APIs nativas y las capacidades ya disponibles en Rust, Windows y Servo. |
| ZNVE-05 | Estado explícito | Perfiles, pestañas, permisos y sesiones tendrán estados y transiciones documentados. |
| ZNVE-06 | Offline-first | Las políticas esenciales de seguridad y privacidad funcionarán sin servicios externos. |
| ZNVE-07 | Zero-trust | Todo contenido, recurso, entrada y mensaje externo se considerará no confiable. |
| ZNVE-08 | Fallos observables | Los errores tendrán categorías, contexto técnico mínimo y procedimientos de recuperación. |
| ZNVE-09 | Recursos deterministas | Los recursos y suscripciones tendrán propietarios y ciclos de vida definidos. |
| ZNVE-10 | Sin capacidades inventadas | Las funciones dependerán de interfaces verificadas, no de supuestos sobre el motor. |
| ZNVE-11 | Privacidad diagnóstica | No se registrarán rutinariamente URL completas, contenido de páginas, credenciales ni datos personales. |
| ZNVE-12 | Rendimiento medido | Toda optimización relevante se contrastará con una línea base reproducible. |

### **1.4. Las seis islas**

**Security Island — aislamiento y recuperación**

Responsable de los límites de confianza, permisos, gestión de fallos, restricciones de procesos y recuperación. Coordina las medidas defensivas, pero no decide unilateralmente sobre todos los datos del usuario.

**Privacy Island — perfiles y privacidad**

Administra perfiles, sesiones efímeras, políticas de almacenamiento, eliminación de datos y preferencias de privacidad. Trabaja con Ocean Defense para aplicar medidas contra rastreadores y mecanismos de identificación.

**Performance Island — eficiencia**

Gestiona el ciclo de vida de pestañas, las métricas de CPU y memoria, la suspensión cuando sea técnicamente segura y los presupuestos de recursos. Las mediciones se harán preferentemente mediante eventos, instrumentación selectiva y pruebas reproducibles, no con sondeos continuos.

**Research Island — investigación y diagnóstico**

Ofrece herramientas de desarrollo, inspección, trazas bajo demanda e información explicable sobre las decisiones de protección. Debe minimizar la exposición de datos sensibles y separar la instrumentación intensiva del funcionamiento normal.

**Freedom Island — control del usuario**

Administra preferencias, excepciones justificadas y controles transparentes. El usuario debe poder entender por qué se bloqueó una solicitud y, cuando sea seguro, decidir si desea permitirla. Las excepciones no podrán desactivar silenciosamente otras protecciones.

**Connectivity Island — contexto de conexión**

Gestiona las opciones de proxy y los parámetros de conectividad que el navegador pueda soportar. No se presentará como una VPN ni como garantía de anonimato. Las protecciones de red dependerán de capacidades verificadas y de la configuración efectiva.

### **1.5. Galapagos Ocean Defense**

Ocean Defense será una **capa transversal de políticas defensivas**, no un antivirus monolítico ni un agente de IA ejecutándose constantemente.

Sus componentes lógicos serán:

| Componente | Responsabilidad | Estrategia ZNVE |
| ----- | ----- | ----- |
| Threat Intelligence | Consultar reputación y reglas de amenazas conocidas. | Datos locales y actualizaciones verificadas; consultas remotas no obligatorias. |
| Request Guard | Aplicar políticas a solicitudes que el motor permita observar o controlar. | Evaluación rápida y reglas deterministas. |
| Behavior Sentinel | Correlacionar señales de comportamiento sospechoso. | Eventos y umbrales contextuales; sin escaneo permanente. |
| Anti-Tracking Shield | Controlar rastreadores, cookies y almacenamiento según capacidades disponibles. | Políticas coherentes y selectivas para reducir incompatibilidades. |
| Payload & Download Guard | Evaluar descargas, procedencia y riesgos conocidos. | Análisis selectivo e integración con mecanismos de Windows cuando corresponda. |
| Exploit Containment | Limitar el impacto de contenido comprometido. | Aislamiento y privilegios mínimos; no depender exclusivamente de detectar amenazas. |

La protección contra bots se tratará como un caso específico de análisis de comportamiento, no como una justificación para inspeccionar toda la navegación de forma continua. La detección deberá distinguir entre automatización legítima, actividad abusiva y comportamiento normal.

**Principio de diseño:** *Shared Signals, Specialized Decisions*. Los módulos compartirán señales mínimas y cada componente tomará únicamente las decisiones que le correspondan.

### **1.6. Contratos de datos y estado**

Antes de implementar módulos, se aprobarán los siguientes contratos conceptuales. Sus campos son propuestas de diseño, no DTOs de producción definitivos.

#### **A. Entidades fundamentales**

| Entidad | Propósito | Datos mínimos |
| ----- | ----- | ----- |
| `BrowserInstance` | Representar una ejecución del navegador. | Identificador de ejecución, estado, versión y perfil activo. |
| `Profile` | Separar los datos de navegación. | Identificador, tipo, ubicación de almacenamiento y política asociada. |
| `BrowserWindow` | Representar una ventana. | Identificador, estado y pestañas asociadas. |
| `Tab` | Representar una pestaña. | Identificador, estado, perfil, contexto del sitio y referencia al motor. |
| `SiteContext` | Describir el contexto de un sitio. | Origen normalizado, permisos aplicables y políticas activas. |
| `NetworkContext` | Representar el contexto de conectividad. | Configuración efectiva, modo de conexión y estado de verificación. |
| `DiagnosticSession` | Controlar una sesión de investigación. | Identificador, alcance, duración, permisos y estado. |
| `DefenseDecision` | Registrar una decisión defensiva. | Regla aplicada, categoría, resultado, motivo y nivel de confianza. |

Los identificadores internos no contendrán datos personales. Las URL completas, los parámetros de consulta y otros datos potencialmente sensibles no se incluirán por defecto en eventos de diagnóstico.

#### **B. Contrato de estado del navegador**

Estados iniciales propuestos:

Created  
   │  
   ▼  
Initializing  
   │  
   ▼  
Ready ◄──────────────┐  
   │                 │  
   ▼                 │  
Running ───────► Suspended  
   │                 │  
   ├──────► Recovering  
   │             │  
   │             └────► Ready  
   ▼  
ShuttingDown  
   │  
   ▼  
Stopped

Reglas obligatorias:

* `Ready` solo se alcanza cuando los componentes esenciales están inicializados.  
* `Suspended` no implica que todas las pestañas, tareas o conexiones estén suspendidas; cada componente deberá informar qué recursos controla y qué puede suspender realmente.  
* `Recovering` no ocultará fallos repetidos ni reintentará indefinidamente.  
* `ShuttingDown` impedirá la creación de nuevas tareas no esenciales.  
* `Stopped` implica que se han liberado los recursos propiedad de la instancia y que no quedan procesos secundarios propios sin una justificación explícita.  
* Un fallo irrecuperable deberá producir un estado terminal identificable, sin simular una recuperación exitosa.

La implementación podrá refinar esta máquina de estados, pero no eliminará sus garantías sin una decisión arquitectónica documentada.

#### **C. Contrato de evaluación defensiva**

Toda evaluación de Ocean Defense tendrá un resultado explícito:

| Resultado | Significado |
| ----- | ----- |
| `Allow` | La solicitud puede continuar según la política aplicable. |
| `Block` | La acción se rechaza por una regla definida. |
| `RequireConfirmation` | Hace falta una decisión del usuario antes de continuar. |
| `Restrict` | Se aplica una limitación específica compatible con las capacidades disponibles. |
| `Unavailable` | No se puede evaluar una parte de la protección. |

`Unavailable` nunca se interpretará como `Allow` de forma implícita. El comportamiento ante una evaluación incompleta dependerá de la operación y de la política de seguridad.

Por ejemplo, la imposibilidad de consultar una reputación remota no debe impedir automáticamente la navegación normal. En cambio, una operación que requiera una verificación de seguridad obligatoria no podrá presentarse como verificada si esa comprobación no se realizó.

Contrato conceptual:

DefenseEvaluation  
    input:  
        context  
        action  
        applicable\_policy  
        available\_signals

    output:  
        decision  
        reason\_code  
        applied\_rule  
        protection\_status

Restricciones:

* Sin propiedades adicionales inventadas por los módulos.  
* Sin llamadas de red implícitas.  
* Sin modificaciones del perfil durante una evaluación.  
* Sin registros rutinarios de datos sensibles.  
* Sin decisiones de seguridad basadas exclusivamente en una puntuación opaca.  
* Sin presentar una comprobación parcial como protección completa.

#### **D. Contrato de eventos**

Se definirán categorías cerradas de eventos antes de implementar el sistema:

* `LifecycleEvent`  
* `NavigationEvent`  
* `ResourceEvent`  
* `SecurityEvent`  
* `PrivacyEvent`  
* `PerformanceEvent`  
* `DiagnosticEvent`

Cada evento tendrá un tipo conocido, un contexto mínimo y una política de retención. No se transmitirán objetos completos del motor a todos los módulos.

Los eventos tendrán estas reglas:

1. Se publican cuando ocurre un cambio significativo, no mediante sondeos repetidos.  
2. Se consumen únicamente por componentes autorizados.  
3. Las suscripciones tienen un propietario y se liberan al terminar su ciclo de vida.  
4. Los errores de un suscriptor no bloquean el resto del navegador.  
5. La instrumentación de diagnóstico ampliada requiere una activación explícita.  
6. Los eventos no se convierten en una copia permanente del historial de navegación.

#### **E. Contrato de perfiles y sesiones**

Los perfiles deberán distinguirse por tipo:

* **Normal:** datos persistentes sujetos a las preferencias del usuario.  
* **Privado:** datos efímeros que se eliminan según la política definida al cerrar la sesión.  
* **Diagnóstico:** contexto temporal con permisos y recopilación restringidos.

Un perfil privado no garantizará invisibilidad frente a sitios web, proveedor de Internet, administrador del dispositivo o servicios externos. La eliminación de datos locales tampoco implica borrar datos que ya hayan sido enviados a terceros.

La política de persistencia deberá especificar qué ocurre con cookies, cachés, almacenamiento web, descargas, permisos, sesiones y datos de diagnóstico. No se asumirá que eliminar un único directorio elimina todos los rastros.

### **1.7. Modelo de hilos y procesos**

La distribución de responsabilidades propuesta es la siguiente:

| Contexto | Trabajo permitido | Trabajo prohibido |
| ----- | ----- | ----- |
| Hilo de interfaz | Actualizar vistas, responder a eventos y coordinar acciones breves. | E/S bloqueante, análisis de archivos, cálculos costosos o espera síncrona del motor. |
| Coordinación del núcleo | Gestionar estados, políticas y tareas de corta duración. | Convertirse en un punto de bloqueo para todo el navegador. |
| Trabajadores de fondo | E/S, validaciones, procesamiento de reglas y análisis selectivo. | Acceder directamente a la interfaz sin un mecanismo de comunicación definido. |
| Motor Servo | Renderizado y operaciones web según las capacidades de su integración. | Suponer que el navegador anfitrión puede controlar internamente cualquier operación del motor. |
| Procesos aislados | Ejecutar contenido o componentes separados cuando la arquitectura de Servo y Windows lo permita. | Acceder libremente al perfil, archivos o credenciales del proceso principal. |

No se fijará una cantidad arbitraria de hilos. El número y la ubicación de los trabajadores dependerán de las necesidades reales, la concurrencia de Servo y las mediciones.

La comunicación entre componentes utilizará mensajes pequeños y contratos tipados. No se copiarán árboles completos de datos en cada actualización de interfaz.

### **1.8. Persistencia y configuración**

La primera versión no necesita una base de datos compleja.

Se evaluarán, por orden de simplicidad:

1. Configuración estructurada local para preferencias y políticas.  
2. Archivos separados por perfil cuando sea necesario.  
3. Almacenamiento indexado solo si los requisitos de consulta lo justifican.  
4. Base de datos local únicamente cuando se demuestre una necesidad que las opciones anteriores no satisfagan.

Requisitos de persistencia:

* Escrituras atómicas cuando la plataforma lo permita.  
* Validación de datos al leerlos.  
* Versionado explícito de los esquemas.  
* Migraciones controladas.  
* Recuperación ante archivos incompletos o corruptos.  
* Permisos de acceso mínimos.  
* Ninguna dependencia de red para cargar la configuración esencial.  
* Ninguna lectura indiscriminada de colecciones cuando basta una consulta específica.

Las claves criptográficas, credenciales y secretos no se almacenarán en texto plano. Si alguna función necesita secretos, deberá definir su almacenamiento seguro en Windows antes de implementarse.

### **1.9. Seguridad, actualizaciones y distribución**

Galapagos deberá contemplar desde el diseño:

* Validación de toda entrada externa.  
* Separación de privilegios entre interfaz, motor y servicios auxiliares.  
* Verificación de integridad de las actualizaciones y reglas defensivas.  
* Protección de los archivos de configuración frente a modificaciones no autorizadas.  
* Tratamiento seguro de enlaces, descargas y archivos malformados.  
* Política de divulgación de vulnerabilidades.  
* Inventario de componentes y licencias.  
* Procedimientos de recuperación ante una actualización fallida.  
* Compilaciones identificables y pruebas de distribución.

El proyecto no declarará que sus compilaciones son reproducibles hasta demostrarlo mediante un procedimiento documentado.

### **1.10. Anti-Bloat Fence: funciones que no entran por defecto**

No se incorporarán al núcleo inicial:

* Telemetría comercial.  
* Analítica remota obligatoria.  
* Un servicio de IA permanente.  
* Múltiples motores web.  
* Un sistema propio de antivirus.  
* Un servicio de sincronización.  
* Un gestor de extensiones completo.  
* Un sistema de cuentas de usuario en la nube.  
* Un sistema de VPN propio.  
* Una base de datos introducida por comodidad.  
* Un framework adicional de interfaz sin una justificación comparativa.  
* Bibliotecas de utilidades que dupliquen funciones nativas.

Cada excepción necesitará un contrato, una justificación técnica, un análisis de coste y un criterio de aceptación.

### **1.11. Roadmap arquitectónico**

La arquitectura se desarrollará en fases con puertas de aprobación. Una fase no se considerará terminada solo porque compile.

| Fase | Entregable | Condición para avanzar |
| ----- | ----- | ----- |
| F0 — Viabilidad | Prueba de integración Servo/Windows, inventario de APIs y riesgos. | Se demuestra que la integración propuesta es técnicamente viable. |
| F1 — Foundation | Núcleo mínimo, ventana, navegación y ciclo de vida. | Navegación funcional, cierre limpio y mediciones iniciales. |
| F2 — Trusted Browser | Límites de confianza, permisos y recuperación. | Los controles esenciales se verifican mediante pruebas. |
| F3 — Private Browser | Perfiles, sesión privada y políticas de almacenamiento. | Los datos se separan y eliminan según los contratos. |
| F4 — Ocean Defense MVP | Reglas locales, protección inicial de navegación y decisiones explicables. | Cobertura demostrada de las superficies realmente controlables. |
| F5 — Adaptive Browser | Gestión del ciclo de vida de pestañas y presupuestos de recursos. | Mejoras medidas sin regresiones inaceptables. |
| F6 — Research Browser | Diagnóstico y herramientas de investigación disponibles. | Instrumentación controlada y protección de datos sensibles. |
| F7 — Open Ecosystem | Distribución, documentación, licencias y proceso de seguridad. | Procedimientos auditables de publicación y mantenimiento. |

**Importante:** F4 no promete una defensa universal contra bots, malware o exploits. Su alcance exacto dependerá de las capacidades verificadas de Servo, Windows y los mecanismos de integración disponibles.

## **BLOQUE 2\. RACIONAL DE INGENIERÍA**

### **2.1. Arquitectura modular sin sobrecarga de microservicios**

Galapagos tendrá separación lógica estricta, pero no una infraestructura distribuida innecesaria.

* Las seis islas serán módulos con responsabilidades y contratos definidos. Solo se separarán en procesos cuando exista una razón de seguridad, aislamiento o estabilidad.  
* Galapagos Core coordinará los estados y las políticas sin convertirse en intermediario obligatorio para cada operación de bajo nivel.  
* Ocean Defense compartirá señales mínimas para evitar que cada subsistema repita las mismas comprobaciones.

**Resultado esperado:** menor complejidad operativa, menos comunicación interna y menor consumo de recursos que una arquitectura con numerosos procesos independientes.

### **2.2. Protección mediante decisiones deterministas**

La defensa inicial utilizará reglas explícitas, políticas locales y mecanismos de aislamiento. La IA no será una dependencia del camino crítico de navegación.

* Las decisiones defensivas serán explicables y comprobables mediante pruebas.  
* Las fuentes externas podrán ampliar la información disponible, pero su indisponibilidad no desactivará automáticamente todas las protecciones locales.  
* Los análisis costosos se activarán de forma selectiva, con límites definidos de CPU, memoria y duración.

**Resultado esperado:** protección útil sin mantener un agente inteligente ejecutándose constantemente ni depender de servicios remotos para las operaciones esenciales.

### **2.3. Rendimiento verificable, no prometido**

No se establecerán objetivos de velocidad arbitrarios antes de medir una integración real de Servo en Windows.

* Se establecerá una línea base de arranque, navegación, consumo de memoria, CPU en reposo y coste de las defensas.  
* Cada capacidad nueva deberá demostrar su coste marginal.  
* Las optimizaciones se validarán con escenarios reproducibles, pruebas de carga y análisis de regresiones.

**Resultado esperado:** decisiones técnicas fundamentadas en mediciones y no en suposiciones sobre Rust, Servo o la arquitectura elegida.

---

## **BLOQUE 3\. TAREAS ATÓMICAS DE IMPLEMENTACIÓN**

Las siguientes tareas definen los documentos que deben crearse. **Todavía no son instrucciones para escribir código de producción.**

Las rutas son propuestas para el repositorio y se consideran contratos documentales iniciales.

### **3.1. Fase A — Fundamentos de arquitectura**

**Tarea 1 — Crear el documento principal de arquitectura**

* `TARGET_FILE`: `docs/architecture/SYSTEM_ARCHITECTURE.md`  
* `ACTION`: documentar componentes, responsabilidades, límites de confianza, dependencias permitidas, flujos de navegación y relación entre Core, las seis islas, Ocean Defense, Servo y Windows.  
* `RESTRICTION`: no convertir cada módulo lógico en un proceso independiente por defecto; no asumir capacidades de Servo sin evidencia.

**Tarea 2 — Formalizar el protocolo ZNVE para el proyecto**

* `TARGET_FILE`: `docs/architecture/ZNVE_ENGINEERING_STANDARD.md`  
* `ACTION`: convertir los cinco pilares, los modos operativos y las restricciones de plataforma en criterios de diseño, implementación y aceptación específicos de Galapagos.  
* `RESTRICTION`: no permitir que un agente genere código de producción sin contratos aprobados.

**Tarea 3 — Crear el registro de decisiones arquitectónicas**

* `TARGET_FILE`: `docs/architecture/adr/README.md`  
* `ACTION`: establecer el formato para registrar decisiones, alternativas, consecuencias, estado y evidencia.  
* `RESTRICTION`: no presentar una decisión pendiente como definitiva.

Las decisiones relevantes tendrán documentos individuales, por ejemplo:

docs/architecture/adr/  
├── README.md  
├── ADR-001-browser-engine.md  
├── ADR-002-process-isolation.md  
├── ADR-003-native-ui.md  
├── ADR-004-persistence.md  
├── ADR-005-defense-pipeline.md  
└── ADR-006-update-security.md

Estos ADR son una estructura propuesta, no decisiones ya aprobadas. La selección definitiva de tecnologías deberá seguir el análisis de viabilidad.

### **3.2. Fase B — Contratos y estado**

**Tarea 4 — Definir las entidades del sistema**

* `TARGET_FILE`: `docs/contracts/DOMAIN_MODEL.md`  
* `ACTION`: especificar `BrowserInstance`, `Profile`, `BrowserWindow`, `Tab`, `SiteContext`, `NetworkContext` y `DiagnosticSession`.  
* `RESTRICTION`: no añadir campos sin una necesidad funcional identificada; no mezclar estado de navegación con datos de diagnóstico.

**Tarea 5 — Definir las máquinas de estado**

* `TARGET_FILE`: `docs/contracts/STATE_MACHINES.md`  
* `ACTION`: documentar estados, transiciones permitidas, eventos desencadenantes, fallos y condiciones terminales.  
* `RESTRICTION`: no permitir transiciones implícitas ni reintentos infinitos.

**Tarea 6 — Definir el sistema de eventos**

* `TARGET_FILE`: `docs/contracts/EVENT_CONTRACTS.md`  
* `ACTION`: especificar tipos de eventos, productores, consumidores autorizados, contexto mínimo, errores y ciclo de vida de las suscripciones.  
* `RESTRICTION`: no utilizar eventos genéricos que transporten objetos completos, ni registrar rutinariamente información sensible.

**Tarea 7 — Definir el contrato defensivo**

* `TARGET_FILE`: `docs/contracts/DEFENSE_CONTRACTS.md`  
* `ACTION`: formalizar entradas, salidas, resultados, códigos de motivo y comportamiento cuando falte información.  
* `RESTRICTION`: no tratar una evaluación no disponible como una evaluación satisfactoria.

### **3.3. Fase C — Ocean Defense**

**Tarea 8 — Formalizar la arquitectura de protección**

* `TARGET_FILE`: `docs/security/OCEAN_DEFENSE_ARCHITECTURE.md`  
* `ACTION`: especificar Threat Intelligence, Request Guard, Behavior Sentinel, Anti-Tracking Shield, Payload & Download Guard y Exploit Containment.  
* `RESTRICTION`: no duplicar mecanismos de protección existentes sin una justificación; no prometer inspección universal del tráfico.

**Tarea 9 — Definir políticas y reglas**

* `TARGET_FILE`: `docs/security/DEFENSE_POLICY_MODEL.md`  
* `ACTION`: establecer prioridades, reglas deterministas, excepciones, actualización autenticada y comportamiento ante fallos.  
* `RESTRICTION`: no introducir decisiones opacas ni bloqueos indiscriminados basados únicamente en reputación desconocida.

**Tarea 10 — Definir el modelo de amenazas**

* `TARGET_FILE`: `docs/security/THREAT_MODEL.md`  
* `ACTION`: identificar activos, adversarios, superficies de ataque, límites de confianza, amenazas y mitigaciones verificables.  
* `RESTRICTION`: no declarar mitigada una amenaza sin un control y una prueba asociados.

**Tarea 11 — Definir privacidad y retención**

* `TARGET_FILE`: `docs/security/PRIVACY_AND_RETENTION.md`  
* `ACTION`: especificar qué datos pueden observarse, procesarse, almacenarse y eliminarse en cada perfil.  
* `RESTRICTION`: no recopilar historial completo para diagnosticar un evento aislado.

### **3.4. Fase D — Eficiencia y persistencia**

**Tarea 12 — Establecer presupuestos de recursos**

* `TARGET_FILE`: `docs/performance/RESOURCE_BUDGETS.md`  
* `ACTION`: definir las métricas, escenarios de prueba, criterios de reposo y límites que se deberán validar por componente.  
* `RESTRICTION`: no inventar umbrales de rendimiento antes de obtener mediciones de referencia.

**Tarea 13 — Diseñar el modelo de persistencia**

* `TARGET_FILE`: `docs/architecture/PERSISTENCE_DESIGN.md`  
* `ACTION`: definir almacenamiento por perfil, escrituras atómicas, recuperación, migraciones y eliminación de datos.  
* `RESTRICTION`: no introducir una base de datos ni una dependencia externa sin demostrar su necesidad.

**Tarea 14 — Definir el ciclo de vida de recursos**

* `TARGET_FILE`: `docs/architecture/RESOURCE_LIFECYCLE.md`  
* `ACTION`: especificar propietarios, creación, liberación y recuperación de handles, procesos, trabajadores y suscripciones.  
* `RESTRICTION`: no dejar tareas sin propietario ni recursos que sobrevivan al cierre sin una justificación documentada.

### **3.5. Fase E — Viabilidad técnica**

**Tarea 15 — Investigar la integración de Servo en Windows**

* `TARGET_FILE`: `docs/research/SERVO_WINDOWS_FEASIBILITY.md`  
* `ACTION`: verificar APIs disponibles, integración de interfaz, navegación, eventos de red, descargas, perfiles, diagnóstico, aislamiento y limitaciones.  
* `RESTRICTION`: no inferir que una API existe porque aparezca en una versión anterior o en un componente distinto del motor.

**Tarea 16 — Seleccionar el modelo de interfaz nativa**

* `TARGET_FILE`: `docs/research/WINDOWS_UI_EVALUATION.md`  
* `ACTION`: comparar las alternativas compatibles con la integración real de Servo, considerando rendimiento, mantenimiento, accesibilidad y complejidad.  
* `RESTRICTION`: no introducir un framework de interfaz antes de demostrar que es necesario.

**Tarea 17 — Crear el plan de pruebas**

* `TARGET_FILE`: `docs/testing/VERIFICATION_STRATEGY.md`  
* `ACTION`: establecer pruebas unitarias, integración, seguridad, fallos controlados, compatibilidad web y rendimiento.  
* `RESTRICTION`: no considerar la compilación satisfactoria como prueba suficiente de seguridad o corrección.

### **3.6. Orden de ejecución**

La secuencia recomendada es:

A. Fundamentos ZNVE  
       │  
       ▼  
B. Contratos y máquinas de estado  
       │  
       ▼  
C. Modelo de amenazas y Ocean Defense  
       │  
       ▼  
D. Recursos, persistencia y rendimiento  
       │  
       ▼  
E. Viabilidad Servo \+ Windows  
       │  
       ▼  
   Revisión humana  
       │  
       ▼  
Aprobación de la arquitectura  
       │  
       ▼  
Implementación del MVP

Existe una excepción importante: la investigación de Servo debe comenzar temprano y ejecutarse en paralelo con la documentación fundacional, porque sus limitaciones podrían obligar a modificar contratos y responsabilidades.

No se aprobará la implementación del MVP hasta resolver las incompatibilidades críticas entre la arquitectura deseada y las capacidades reales del motor.

---

## **BLOQUE 4\. PLAN DE VERIFICACIÓN ATÓMICA**

Como estamos en la fase de diseño, la verificación inicial será documental y de viabilidad. **No hay todavía comandos de compilación o pruebas de código que puedan considerarse válidos**, porque no se ha establecido el repositorio ni la versión concreta de Servo.

### **4.1. Verificación documental**

| ID | Prueba | Criterio de aceptación |
| ----- | ----- | ----- |
| V-01 | Integridad de contratos | Cada componente tiene entradas, salidas, estados y errores definidos. |
| V-02 | Responsabilidades | No hay responsabilidades críticas duplicadas sin justificación. |
| V-03 | Dependencias | Cada dependencia entre módulos está documentada y autorizada. |
| V-04 | Ciclo de vida | Los recursos tienen propietario, liberación y tratamiento de errores definidos. |
| V-05 | Privacidad | Cada dato recopilado tiene una finalidad, alcance y política de retención. |
| V-06 | Fallos | Cada operación crítica define su comportamiento ante errores y desconexión. |
| V-07 | Anti-Bloat Fence | Las dependencias y servicios opcionales tienen justificación explícita. |
| V-08 | Ocean Defense | Cada capacidad tiene una amenaza, una política y un mecanismo verificable. |
| V-09 | Viabilidad | Las capacidades críticas de Servo se contrastan con documentación y pruebas. |
| V-10 | Trazabilidad | Los requisitos se relacionan con decisiones y futuras pruebas de aceptación. |

### **4.2. Verificación de Servo y Windows**

La investigación deberá utilizar como punto de partida la documentación oficial:

* [Servo Book](https://book.servo.org/)  
* [Documentación de las APIs de Servo](https://doc.servo.org/servo/?utm_source=chatgpt.com)

Para cada capacidad relevante se documentará:

| Campo | Contenido requerido |
| ----- | ----- |
| Capacidad | Función que Galapagos necesita. |
| Evidencia | API, documentación o prueba que demuestra su disponibilidad. |
| Versión | Versión concreta de Servo evaluada. |
| Integración | Forma de utilizar la capacidad desde el navegador anfitrión. |
| Limitaciones | Restricciones conocidas o aspectos pendientes. |
| Riesgo | Consecuencia de que la capacidad no esté disponible. |
| Alternativa | Solución compatible o decisión de retirar la función del MVP. |

La viabilidad no se aprobará basándose únicamente en que Servo pueda ejecutar páginas web. Deben investigarse también los mecanismos necesarios para integrar la interfaz, controlar el ciclo de vida, observar eventos, aplicar políticas y mantener los límites de seguridad.

### **4.3. Verificación de rendimiento**

Cuando exista una compilación funcional, se establecerá una línea base antes de añadir Ocean Defense.

Las mediciones incluirán:

* Tiempo de arranque en frío.  
* Tiempo de apertura de una página.  
* CPU y memoria en reposo.  
* CPU y memoria con múltiples pestañas.  
* Coste de aplicar reglas de navegación.  
* Impacto de las protecciones de privacidad.  
* Recuperación tras cerrar una pestaña o sufrir un fallo.  
* Liberación de procesos y recursos al cerrar el navegador.

Se utilizarán escenarios reproducibles, varias ejecuciones y comparaciones con una línea base equivalente. Las métricas y umbrales de aceptación se aprobarán después de conocer el comportamiento real de la integración.

### **4.4. Verificación de seguridad**

Las pruebas posteriores deberán incluir, como mínimo:

1. URLs malformadas y entradas externas inesperadas.  
2. Redirecciones repetidas y ventanas emergentes abusivas.  
3. Descargas inesperadas y archivos de tipos engañosos.  
4. Reglas defensivas corruptas o con una firma inválida.  
5. Pérdida de conectividad durante una actualización.  
6. Archivos de configuración corruptos.  
7. Cierre durante una operación de escritura.  
8. Fallos de componentes y recuperación.  
9. Intentos de acceder a datos de otro perfil.  
10. Intentos de continuar una operación cuando una comprobación obligatoria no puede realizarse.

Los resultados deberán identificar el control que actuó, el resultado observado y la evidencia de la prueba.

### **4.5. Criterios de aprobación de la arquitectura**

La arquitectura podrá pasar a implementación cuando:

* Los contratos esenciales estén aprobados.  
* Las responsabilidades de Core, las seis islas y Ocean Defense estén delimitadas.  
* Los límites de confianza estén documentados.  
* La integración de Servo en Windows sea viable para el alcance del MVP.  
* Las funciones que dependen de APIs no verificadas estén identificadas y excluidas temporalmente.  
* Exista un plan verificable de privacidad, seguridad y rendimiento.  
* Las dependencias necesarias estén justificadas.  
* Se hayan registrado los riesgos que requieren decisiones del Director de Arquitectura.

---

## **Decisión arquitectónica propuesta**

Galapagos debe construirse como un **monolito modular de escritorio, con aislamiento de procesos selectivo y un motor web integrado**, no como una colección de servicios independientes.

La arquitectura tendrá tres propiedades fundamentales:

1. **Core coordina; las islas especializan; Ocean Defense protege.**  
2. **Las decisiones se diseñan y documentan antes de generar código.**  
3. **Ninguna función se considera implementada hasta demostrar su comportamiento, su coste y sus límites.**

El primer hito técnico será validar Servo en Windows y contrastar esa integración con los contratos definidos. No conviene comenzar a implementar las seis islas simultáneamente: eso multiplicaría las incertidumbres y dificultaría localizar las causas de los problemas.

**Estado final de esta fase:** arquitectura conceptual y plan de verificación definidos; implementación de producción no autorizada todavía.

La siguiente entrega documental recomendada es `SYSTEM_ARCHITECTURE.md` junto con `SERVO_WINDOWS_FEASIBILITY.md`, porque esos dos documentos permitirán convertir el diseño conceptual en una arquitectura ejecutable sin inventar capacidades ni comprometer prematuramente el stack.