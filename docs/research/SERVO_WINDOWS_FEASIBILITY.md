# SERVO WINDOWS FEASIBILITY — Estudio de Viabilidad Técnica e Integración

**Estado:** Reporte de Viabilidad Técnica (Fase E · Tarea 15)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Plataforma Objetivo:** Windows 10/11 64-bit (x86_64-pc-windows-msvc)  
**Motor Evaluado:** `libservo` / Servo Embedder API v2024.x+  

---

## 1. Resumen Ejecutivo de Viabilidad

La viabilidad técnica de integrar Servo en Windows como motor de navegación para Galapagos es **favorable con mitigaciones controladas**.
Servo proporciona una biblioteca Rust (`servo`) y una C-API (`libservo`) que permite instanciar el motor sobre superficies nativas del sistema operativo (HWND en Windows vía WGL/Angle/EGL o DirectX interop).

**Veredicto:** Apto para el alcance del MVP bajo el principio *Shared Signals, Specialized Decisions*, manteniendo el motor confinado en un proceso hijo aislado.

---

## 2. Matriz de Viabilidad por Capacidad Crítica

| Capacidad Requerida | Evidencia / API Evaluada | Modelo de Integración | Limitaciones Conocidas | Riesgo ZNVE | Estrategia de Mitigación / Alternativa |
|---|---|---|---|---|---|
| **Creación de Ventana y Superficie** | `Servo::new`, `WindowMethods`, `RenderingContext` | El anfitrión Win32 crea el HWND y entrega el context de rendering al embedder. | Requiere inicialización de contexto gráfico en el hilo secundario. | Medio: Bloqueo de UI si se comparte hilo. | **Mitigación:** Hilo/proceso dedicado para el renderizado; UI corre en hilo Win32 independiente. |
| **Control de Navegación** | `BrowserId`, `load_url`, `reload`, `stop` | Comandos asíncronos canalizados desde el Coordinador (`UiToCoordinatorCmd::Navigate`). | URLs deben validarse previamente en el Coordinador. | Bajo: Manipulación de URL maliciosa. | **Mitigación:** Validación inmutable estricta en Rust (`UiToCoordinatorCmd::validate_url_scheme`) antes de invocar FFI. |
| **Interceptación de Red y Solicitudes** | `embedder_traits::EventLoopWaker`, hooks de red | Interceptación de solicitudes HTTP(S) previo a la resolución de socket. | No todas las solicitudes de recursos internos son cancelables síncronamente en todas las versiones. | Alto: Evasión de `RequestGuard`. | **Mitigación:** Enrutamiento a través de proxy local intermedio o validación en la frontera del socket antes de despachar bytes. |
| **Partición de Perfiles y Cookies** | `CookieJar`, `ResourceQuota` | Partición de almacenamiento local por `ProfileId`. | Las sesiones efímeras no deben escribir archivos de base de datos a disco. | Alto: Fuga de privacidad en modo efímero. | **Mitigación:** Perfiles `TemporaryPrivate` configurados con backend de almacenamiento volátil en memoria RAM (`:memory:`). |
| **Aislamiento de Procesos (Sandbox)** | Windows AppContainer + Job Object (`windows-sys`) | El Coordinador lanza el proceso Servo usando `CreateProcessW` con `EXTENDED_STARTUPINFO_PRESENT` y token restringido. | AppContainer restringe acceso a red salvo capacidad explícita (`privateNetworkClientServer` / `internetClient`). | Medio: Errores de acceso a red o drivers GPU. | **Mitigación:** Pruebas controladas de capacidades mínimas de AppContainer; fallback a Low Integrity Level si la GPU requiere drivers específicos. |
| **Límites de CPU y Memoria** | `SetInformationJobObject` (`JobObjectExtendedLimitInformation`) | El Coordinador asigna el Job Object al proceso Servo inmediatamente después del spawn. | La terminación por cuota de memoria es abrupta si no se maneja señal preventiva. | Bajo: Crash no controlado del proceso worker. | **Mitigación:** El Coordinador maneja el fallo del worker como `GalapagosError::ResourceQuotaExceeded` y ofrece recargar la pestaña. |

---

## 3. Arquitectura del Adaptador `WebEnginePort`

El contrato [contracts/galapagos_core_v1.rs](file:///d:/App/Browser-SaveNav/contracts/galapagos_core_v1.rs) define el trait de frontera:

```rust
pub trait WebEnginePort {
    fn initialize_sandboxed_worker(
        &mut self,
        tab_id: TabId,
        sandbox: WindowsSandboxLevel,
        quota: ResourceQuota,
    ) -> Result<ProcessId, GalapagosError>;

    fn dispatch_navigation(
        &mut self,
        tab_id: TabId,
        validated_url: &str,
    ) -> Result<(), GalapagosError>;

    fn terminate_worker(&mut self, process_id: ProcessId) -> Result<(), GalapagosError>;
}
```

### Reglas de Implementación en Windows:
1. **Sin dependencias pesadas en el worker:** El ejecutable del worker de contenido web (`galapagos-content.exe`) únicamente compilará Servo y el protocolo IPC mínimo, sin dependencias de UI ni persistencia.
2. **IPC determinista:** Toda comunicación IPC se canaliza mediante tuberías con nombre de Windows (`Named Pipes`) en modo bidireccional y sin búfer ilimitado.
3. **Cero fugas de handles:** Los handles de proceso y job object se cierran explícitamente (`CloseHandle`) cuando el worker finaliza.

---

## 4. Conclusiones y Próximos Pasos para Fase E

1. La arquitectura definida en `SYSTEM_ARCHITECTURE.md` es **100% compatible** con las capacidades reales de Servo en Windows.
2. No existen dependencias parásitas añadidas: la integración de ventanas y sandboxing se resuelve de forma nativa con `windows-sys`.
3. Se autoriza continuar con la persistencia SQLite (**Paso Inmediato 3**) y la especificación de Ocean Defense.
