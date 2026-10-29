# STATE MACHINES — Galapagos Browser v0.1

**Estado:** Contrato Formal de Máquinas de Estado (Fase B · Tarea 5)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  

---

## 1. Reglas Fundacionales de Transición (ZNVE-05 / ZNVE-08)

1. **Determinismo Estricto:** Toda transición de estado requiere un evento desencadenante explícito y una precondición validada. Prohibidas las transiciones implícitas o basadas en temporizadores arbitrarios (`sleep`).
2. **Cero Reintentos Infinitos:** Ningún estado de fallo o recuperación (`Recovering`) podrá reintentar una operación más de 3 veces consecutivas. Tras el tercer fallo, la transición conduce forzosamente a un estado terminal explícito.
3. **Observabilidad:** Cada cambio de estado emite un `LifecycleEvent` tipado con contexto mínimo.

---

## 2. Máquina de Estados del Navegador (`BrowserLifecycleState`)

```
Created
   │
   ▼ (InitCmd)
Initializing
   │
   ▼ (AllSubsystemsReady)
 Ready ◄──────────────────┐
   │                      │
   ▼ (UserOpenWindow)     │ (ResumeEvent)
Running ────────────► Suspended
   │                      │
   ├──────► Recovering    │ (ShutdownSignal)
   │   (Crash)    │       │
   │              └───────┘ (Success)
   ▼ (ShutdownCmd)
ShuttingDown
   │
   ▼ (ResourcesReleased)
Stopped
```

### Tabla de Transiciones del Navegador
| Estado Origen | Evento Desencadenante | Precondición | Estado Destino | Acción Invariable |
|---|---|---|---|---|
| `Created` | `StartCoordinator` | Configuración base cargada | `Initializing` | Inicializa `StorageEngine`, `RequestGuard` y canal IPC. |
| `Initializing` | `SubsystemsReady` | SQLite y reglas verificadas | `Ready` | Abre la ventana principal Win32. |
| `Ready` | `OpenSession` | Perfil válido registrado | `Running` | Admite apertura de pestañas y despacho de comandos. |
| `Running` | `SystemSuspend` / `AllTabsBackground` | Proceso inactivo por OS | `Suspended` | Notifica a workers para reducción de memoria de trabajo. |
| `Suspended` | `SystemResume` / `UserForeground` | Foco de ventana restablecido | `Ready` | Restaura prioridad de hilos y valida canales IPC. |
| `Running` | `WorkerCrashDetected` | Contador de reintentos < 3 | `Recovering` | Aísla el fallo, registra error y recrea el worker. |
| `Recovering` | `RecoverySuccess` | Estado restaurado | `Ready` | Reinicia contador de fallos. |
| `Recovering` | `RecoveryExhausted` | Contador de reintentos >= 3 | `ShuttingDown` | Emite diagnóstico terminal y aborta la recuperación. |
| `Running` / `Suspended` | `UserQuit` / `OsShutdown` | Señal de apagado | `ShuttingDown` | Rechaza nuevas solicitudes de navegación. |
| `ShuttingDown` | `AllHandlesClosed` | Workers terminados y SQLite cerrado | `Stopped` | Proceso finaliza con código 0. |

---

## 3. Máquina de Estados de Pestañas (`TabLifecycleState`)

```
   Created
      │
      ▼ (SpawnTabCmd)
SpawningWorker
      │
      ▼ (ProcessIdAssigned)
   Active ◄──────────────┐
      │                  │
      ├── (NavigateCmd)  │ (UserFocus)
      │       │          │
      │       ▼          │
      │   Navigating     │
      │       │          │
      │       ▼          │
      │     Active ──────┴─► Background
      │                         │
      ▼ (QuotaExceeded / Close) │ (CloseCmd)
  Terminating ◄─────────────────┘
      │
      ▼ (JobObjectCleaned)
  Terminated
```

### Reglas de Pestaña:
1. `Created` → `SpawningWorker`: Asigna `TabId`, asocia `ResourceQuota` y lanza el proceso enjaulado en AppContainer.
2. `Active` ⇄ `Background`: Señaliza al Job Object para reducir la cuota de CPU cuando la pestaña pierde el foco visual.
3. `Terminating`: Desecho explícito del estado en memoria (`drop(TabState)`), cierre del handle de proceso y reducción de contadores.
4. `Terminated`: La pestaña se elimina de la colección `tabs: HashMap<TabId, TabState>`.

---

## 4. Máquina de Estados de Perfil (`ProfileLifecycleState`)

| Estado | Significado | Comportamiento en Compartimento `Personal` | Comportamiento en `TemporaryPrivate` / `Laboratory` |
|---|---|---|---|
| `Registered` | Perfil instanciado en el Coordinador | Metadatos guardados en tabla `profiles` (SQLite WAL). | Registro en memoria volátil. |
| `Active` | Tiene pestañas abiertas asociadas | Permite persistencia de permisos y caché en disco. | Almacenamiento en RAM `:memory:`. |
| `Purging` | Se ejecuta orden de destrucción | No aplica (perfiles personales persisten en disco). | Cierra todas las pestañas, purga buffers y borra la base `:memory:`. |
| `Destroyed` | Perfil eliminado del Coordinador | Eliminación explícita bajo confirmación. | Desecho inmediato de memoria; cero residuos en disco. |
