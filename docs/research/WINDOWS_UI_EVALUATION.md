# WINDOWS UI EVALUATION — Evaluación del Modelo de Interfaz Nativa

**Estado:** Evaluación Técnica de Interfaz Nativa (Fase E · Tarea 16)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Decisión Asociada:** [docs/architecture/adr/ADR-003-native-ui.md](file:///d:/App/Browser-SaveNav/docs/architecture/adr/ADR-003-native-ui.md)  

---

## 1. Contexto y Requisitos de Interfaz en Galapagos (ARC-02)

La arquitectura de Galapagos exige que la interfaz de escritorio (marcos de ventana, pestañas, barra de direcciones, menús de privacidad y diálogos de seguridad) esté **completamente desacoplada del motor web de renderizado**:
1. **Seguridad y Disponibilidad:** Si el motor de renderizado sufre un bloqueo o es explotado, la UI del navegador debe seguir respondiendo para permitir cerrar la pestaña o aislar el proceso.
2. **Cero Sobrecarga en Reposo:** La UI no puede instanciar runtimes de JavaScript (Node.js/Electron) ni navegadores secundarios dentro del proceso privilegiado.
3. **Integración con Servo:** La ventana anfitriona debe proveer un descriptor de ventana nativo (`HWND`) donde Servo proyecta la superficie gráfica de navegación (DirectX / WGL / ANGLE).

---

## 2. Comparativa de Alternativas Evaluadas

| Criterio de Selección | Alternativa A: Win32 Clásico Nativo (`windows-sys`) | Alternativa B: WinUI 3 / Windows App SDK | Alternativa C: Framework Web (Electron / Tauri UI) | Alternativa D: GUI Multiplataforma (egui / Slint) |
|---|---|---|---|---|
| **Huella de Memoria (RAM)** | **Mínima (< 10 MiB)** | Media (~45 - 80 MiB) | Prohibitiva (> 120 MiB en reposo) | Baja (~15 - 30 MiB) |
| **Tiempo de Arranque** | **Instantáneo (< 20 ms)** | Lento (~200 - 400 ms por inicialización XAML) | Lento (> 500 ms) | Rápido (~40 - 80 ms) |
| **Integración HWND con Servo**| **Directa y nativa:** `CreateWindowExW`, subclasificación Win32 y control de mensajes. | Compleja: requiere `XamlHost` y swapchains intermedios. | Muy compleja: composición de un navegador dentro de otro. | Requiere puente de ventana nativo en cada plataforma. |
| **Superficie de Dependencias** | **Cero dependencias:** bindings directos a `user32.dll` y `gdi32.dll`. | Pesada: runtime redistribuible de Windows App SDK. | Masiva: Chromium + V8 + Node.js embebidos. | Media: compilación de crates adicionales de renderizado. |
| **Accesibilidad Nativa** | Soportada via Microsoft UI Automation y MSAA nativo. | Alta: soporte XAML nativo. | Mediada por HTML accessibility tree. | Parcial o experimental según backend. |
| **Veredicto ZNVE** | **SELECCIONADA (MVP)** | Diferida a Fase P2 (si se justifica modernización visual) | **RECHAZADA (Violación Anti-Bloat Fence)** | Descartada para Windows P0 |

---

## 3. Modelo de Integración Seleccionado: Win32 HWND Nativo

### 3.1. Arquitectura de Ventana Anfitriona
```
[Proceso Trusted Coordinator]
  ├── Hilo UI Principal (Win32 Message Loop)
  │     ├── HWND Principal (Marco de Ventana, Barra de URL, Controles)
  │     └── HWND Hijo Contenedor (Surface Host)
  │           │
  │           ▼ WGL / ANGLE SwapChain
  └── Proceso Worker de Servo (AppContainer)
        └── DIB / DXGI Shared Handle para renderizado acelerado
```

### 3.2. Bucle de Mensajes Reactivo (ZNVE-01)
* El hilo de la UI opera con un bucle estándar reactivo sin retardos arbitrarios:
  ```rust
  let mut msg = std::mem::MaybeUninit::uninit();
  while unsafe { GetMessageW(msg.as_mut_ptr(), null_mut(), 0, 0) } > 0 {
      let m = unsafe { msg.assume_init() };
      unsafe {
          TranslateMessage(&m);
          DispatchMessageW(&m);
      }
  }
  ```
* Las notificaciones desde el hilo coordinador o workers secundarios se canalizan mediante `PostMessageW(hwnd, WM_APP_COORDINATOR_EVENT, ...)` sin bloquear el bucle de renderizado.

---

## 4. Conclusiones y Próximos Pasos

1. **Win32 Nativo** es la única alternativa que garantiza huella nula, arranque instantáneo y compatibilidad directa con el embedding de Servo en Windows 10/11.
2. No se introducirá ningún framework visual adicional en el MVP. La barra de navegación y controles de pestaña se construirán con controles nativos de Windows, garantizando accesibilidad y consistencia con el sistema operativo.
