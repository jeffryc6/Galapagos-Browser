# RESOURCE LIFECYCLE — Galapagos Browser v0.1

**Estado:** Ciclo de Vida y Liberación Determinista de Recursos (Fase D · Tarea 14)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  

---

## 1. Principio de Propiedad Determinista (ZNVE-09)

Todo descriptor de sistema operativo (handle Win32, Job Object, proceso worker, socket o conexión SQLite) tiene un **propietario único y unívoco**. Prohibidos los recursos compartidos sin delimitación de ciclo de vida o handles huérfanos que sobrevivan al cierre del navegador.

```
┌────────────────────────────────────────────────────────┐
│             GALAPAGOS CORE (Trusted Coordinator)       │
│                                                        │
│  ├── Propietario de: HWND de Ventana Principal        │
│  ├── Propietario de: Mutex<Connection> SQLite          │
│  ├── Propietario de: Colección de Pestañas (TabState) │
│  └── Propietario de: Windows Job Objects               │
└───────────────────────────┬────────────────────────────┘
                            │
               Spawn & Asignación RAII
                            ▼
┌────────────────────────────────────────────────────────┐
│             SERVO CONTENT PROCESS WORKER               │
│                                                        │
│  ├── Enjaulado en: Windows AppContainer Strict        │
│  ├── Vinculado a: Windows Job Object                   │
│  │   (JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)              │
│  └── Canal IPC: Named Pipe bidireccional acotado      │
└────────────────────────────────────────────────────────┘
```

---

## 2. Matriz de Propiedad y Desecho RAII

| Recurso de Sistema | Propietario | Evento de Creación | Evento de Liberación / Desecho | Garantía de Liberación |
|---|---|---|---|---|
| **Ventana Win32 (`HWND`)** | `BrowserWindow` (UI Thread) | `OpenSession` / `Ready` | `WM_DESTROY` / `CloseWindow` | `DestroyWindow(hwnd)` al salir del bucle de mensajes. |
| **Proceso Worker de Servo** | `Coordinator` (`TabState`) | `UiToCoordinatorCmd::SpawnTab` | `terminate_tab` / `CloseTab` | `TerminateProcess` y `CloseHandle` en el destructor. |
| **Windows Job Object** | `Coordinator` | Al instanciar el primer worker | `Coordinator::drop` | Flag `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: si el Coordinador muere, el kernel de Windows termina forzosamente todos los workers hijos. |
| **Conexión SQLite Persistente** | `StorageEngine` | Arranque del Coordinador | Cierre del Coordinador | `drop(conn)` ejecuta rollback de transacciones abiertas y cierra descriptores `.db`, `.wal` y `.shm`. |
| **Conexión SQLite en RAM** | `Profile` efímero | `CreateProfile` (Ephemeral) | `destroy_ephemeral_profile` | Desasignación inmediata de páginas de RAM (`drop`). |
| **Suscripción de Eventos** | Consumidor registrado | Suscripción a canal | Descarte del consumidor | `drop(SubscriptionHandle)` desregistra el canal del despachador. |

---

## 3. Protocolo de Apagado Limpio (`Clean Shutdown Sequence`)

Cuando el usuario cierra la ventana o el sistema operativo emite `WM_CLOSE` / `WM_QUERYENDSESSION`:

```
[Señal WM_CLOSE / ShutdownCmd]
                │
                ▼
   1. Transición a estado `ShuttingDown`
      (Se rechaza el despacho de nuevos comandos UiToCoordinatorCmd)
                │
                ▼
   2. Terminación ordenada de todos los workers de pestañas
      (Señalización IPC de cierre; fallback a TerminateProcess tras 2 000 ms)
                │
                ▼
   3. Purga forzosa de perfiles efímeros activos
      (Destrucción de bases :memory: y buffers de cookies volátiles)
                │
                ▼
   4. Cierre y checkpoint pasivo de SQLite WAL
      (sqlite3_close_v2 garantiza flush ordenado del log a disco)
                │
                ▼
   5. Destrucción de la ventana Win32 y cierre del Job Object
      (El kernel liquida cualquier proceso residual)
                │
                ▼
   Estado `Stopped`: Proceso anfitrión finaliza con código de salida 0
```

---

## 4. Detección y Prevención de Fugas de Handles

1. **Pruebas Automatizadas de Ciclo de Vida:** La suite de pruebas unitarias (`test_ephemeral_profile_destruction`, `test_coordinator_lifecycle`) valida que al destruir una entidad los contadores de perfiles y pestañas vuelvan a 0.
2. **Inspección en Hardening (`/znve-audit`):** El comando de auditoría supervisa mediante contadores de Windows (`GetProcessHandleCount`) que no existan fugas monótonas de handles tras abrir y cerrar 100 pestañas consecutivas.
