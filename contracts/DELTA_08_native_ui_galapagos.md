# DELTA 08 — Interfaz nativa Galapagos (barra, marca y panel)

**Estado:** CONGELADO (2026-10-26) · D29–D35 aprobadas por el Director (plan de 7 pasos) · **Origen:** el Director informa que la interfaz «parece Java»; adjunta el tablero de marca (`Galápagos Browser Logo Concept Board.png`), un mockup del panel y `graficosUI.txt` · **Extiende:** `contract.md` (stack «Win32/WinUI 3 nativo», ARC-02), DELTA_02 (barra y entrada), `T-02b` (sin afirmaciones falsas) · solo aditivo, salvo R-18 (§2.4)

## 0. Por qué hoy «parece Java»

| Causa | Evidencia |
|---|---|
| **Sin manifiesto de aplicación:** Windows dibuja los controles con el estilo clásico de Windows 95 (Common Controls v5) y trata el proceso como no consciente de DPI | No existe `build.rs` ni `.manifest` en el Coordinador |
| **Fuente de sistema por omisión:** la barra y el botón usan la fuente bitmap negrita «System» (se ve en las capturas: `https://google.com`, «Navegar») | `window/mod.rs` crea `EDIT`/`BUTTON`/`STATIC` sin `WM_SETFONT` |
| **Barra de título y fondo claros** con controles grises estándar | Sin `DwmSetWindowAttribute`; `hbrBackground = COLOR_WINDOW` |
| **Panel dibujado con GDI básico:** rectángulos sin antialiasing y textos fijos en el código | `paint_galapagos_dashboard` |
| **La paleta está en el contrato y no se usa:** `GALAPAGOS_DARK` y `LiveDashboardState` (R-18) | `contracts/galapagos_core_v1.rs:512-584`; el panel usa `rgb()` fijo en el código |

## 1. Auditoría del mockup y de `graficosUI.txt` frente al contrato

| Elemento | Viable en v0.1 | Cubo |
|---|---|---|
| Tema oscuro, logo, Segoe UI, barra redondeada con iconos | Sí, con APIs del sistema (§5) | A |
| Atrás / Adelante / Recargar | Necesita mensajes IPC nuevos al worker (es UX-01 del backlog) | A (D31) |
| Inicio (`galapagos://dashboard`) | Sí, sin IPC | A |
| Insignia de seguridad («SEGURO», «Evolution Green — Protección Activa») | Solo con estados **reales**. Hoy no hay sandbox ni filtro de rastreadores activo, así que mostrar «SEGURO» sería repetir T-02b | A (D33) |
| Panel: barra lateral + tarjetas + gráfico CPU/RAM | Sí. Gráfico con muestras **reales** del worker (`GetProcessMemoryInfo` + `GetProcessTimes`, 1 Hz, búfer circular) | A |
| Pestañas múltiples (`+`, lista) | Requiere varios workers (UX-02) | B |
| Favoritos | Requiere tabla nueva (esquema v2, PERS-01) | B |
| Laboratorio / Ajustes | DEV-01 diferido | B |
| `adblock-rust` | Requiere que la red pase por el Coordinador (T-04b) | B (delta propio) |
| `ed25519-dalek` + `sha2` | Es el verificador SEC-03 pendiente | B (delta propio) |
| `zeroize` + `VirtualLock` | PRIV-02 diferido | B |
| `wasmtime`, `symphonia`, `harfbuzz_rs`/`swash` | Sustituir componentes internos de Servo | Fuera de alcance (vendor) |
| `cargo-audit`, `cargo-deny`, `cargo-geiger` | Herramientas de CI, no de runtime | B (`CI-01`) |
| `egui`, `Slint`, `winit` | Dependencias de UI no necesarias: el stack del contrato es Win32 nativo y GDI+ cubre lo visual | Descartado (D29) |

Los textos de las tarjetas del mockup no tienen sentido («proestıno del mastmatiano…», «efímeondas»): se escriben de nuevo, con contenido real.

## 1b. Regla del Director (2026-10-26): datos reales, cero simulación

**UI-I0** Todo lo que la interfaz muestra sale de datos de producción medidos en ese momento: la navegación real, el handshake real, el Job Object real (`QueryInformationJobObject`) y los contadores del proceso real. Esto implica:
- **Prohibido en el código de la aplicación:** valores de ejemplo, gráficos de relleno, estados fijos en el código («SEGURO», «Protección activa»), datos simulados o modos de demostración.
- **Si un dato no está disponible**, se dice («no disponible», «NO aplicado»); no se inventa.
- **Lo descartado del mockup** (estados falsos, textos sin sentido, pestañas, Favoritos, Laboratorio, barra de título personalizada, `adblock-rust`) y de `graficosUI.txt` (`egui`/`Slint`, `ed25519-dalek`, `zeroize`, herramientas de CI) **no forma parte de este delta**. Lo que tiene valor futuro va al backlog (§7) o a su propio delta.

## 2. CONTRATO DE ENTRADA Y SALIDA (Cubo A)

### 2.1 Tema (en `src/window`, no en el contrato)

```rust
/// Δ8: paleta de marca (tablero de marca). Vive en la capa de UI; sale del contrato (R-18).
pub struct GalapagosTheme {
    pub volcanic_black: u32,   // #0B121A  fondo de ventana
    pub deep_ocean: u32,       // #0F1C27  barra de herramientas
    pub lava_stone: u32,       // #222B34  tarjetas y campo de dirección
    pub stone_border: u32,     // #3A4552  bordes y separadores
    pub iguana_fire: u32,      // #E66A1F  acento, pestaña activa, alertas
    pub pacific_teal: u32,     // #1D9BB8  enlaces, foco, iconos
    pub evolution_green: u32,  // #43A047  estado protegido (solo si es real)
    pub text_primary: u32,     // #E6EDF3
    pub text_secondary: u32,   // #8C9BAB
}
```

### 2.2 Mensajes IPC nuevos Coordinador → worker (D31, UX-01)

| Tipo | Nombre | Payload | Efecto en el worker |
|---|---|---|---|
| 14 | `MSG_GO_BACK` | vacío | `webview.go_back(1)` |
| 15 | `MSG_GO_FORWARD` | vacío | `webview.go_forward(1)` |
| 16 | `MSG_RELOAD` | vacío | `webview.reload()` |
| 17 | `MSG_REPAINT` | vacío | `webview.paint()` + `present()` (cubre R-3: la ventana del motor pide repintar) |

Se definen en `crates/galapagos-ipc`. Un payload no vacío es `MalformedMessage`.

Worker → Coordinador, aditivo:

| Tipo | Nombre | Payload |
|---|---|---|
| 9 | `MSG_NAV_STATE` | `tab_id u64 · can_back u8 · can_forward u8 · loading u8`, para activar o desactivar los botones y la animación de carga |

### 2.3 Estado de seguridad mostrado (D33)

```rust
/// Δ8: insignia de la barra. Se calcula a partir de hechos medidos, nunca de constantes.
pub enum SecurityBadge {
    /// Destino evaluado por Ocean Defense y permitido. Texto: «Ocean Defense: permitido»
    Allowed,
    /// Ocean Defense bloqueó o pidió confirmación. Texto con el motivo (reason_code legible)
    Blocked,
    /// Sin evaluación (página interna). Texto: «Página interna»
    Internal,
}
```

**Implementación (paso 4):** `Blocked(BlockReason)` lleva el motivo como enum cerrado (`Threat`, `ConfirmationRequired`, `VerificationUnavailable`, `CoordinatorDenied`, `InvalidDestination`, `InvalidAddress`), cada uno con su texto. Los fallos del motor (R9) van aparte en `EngineFault` (`Unavailable`, `WorkerFault`, `WorkerTerminated`) y ocupan el sitio de la insignia porque tienen prioridad. Sin navegación evaluada no se dibuja insignia.

Panel «Aislamiento»: muestra `applied_sandbox` real del handshake, el límite de memoria efectivo del Job (consultado con `QueryInformationJobObject`) y el PID del worker. Mientras T-02 siga pendiente, dice «AppContainer: NO aplicado».

### 2.4 Retirada del contrato (R-18)

`GalapagosTheme`, `GALAPAGOS_DARK`, `DASHBOARD_HISTORY_LEN` y `LiveDashboardState` salen de `contracts/galapagos_core_v1.rs`, porque son estado de UI. El búfer del gráfico pasa a `src/window/dashboard.rs`, con 60 muestras.

## 3. CONTRATO DE PERSISTENCIA

Sin cambios. Favoritos queda en el Cubo B (requiere esquema v2).

## 4. CONTRATO DE ERRORES

Sin variantes nuevas. Si falla `GdiplusStartup` o la carga del logo, se registra `UI_ASSET_UNAVAILABLE` y la UI dibuja sin logo; no es fatal, porque es cosmético. Un fallo de `DwmSetWindowAttribute` (Windows 10 anterior a 20H1) deja la barra de título clara, sin error.

## 5. ANTI-BLOAT FENCE

- **Entra (sin crates nuevos):** features de `windows-sys` `Win32_Graphics_GdiPlus` (antialiasing, rutas redondeadas, carga de PNG), `Win32_Graphics_Dwm` (barra de título oscura) y `Win32_UI_HiDpi` (escala por monitor). Fuentes del sistema: **Segoe UI Variable** (`SegUIVar.ttf`) e iconos **Segoe Fluent Icons** (`SegoeIcons.ttf`), con respaldo `segmdl2.ttf` en Windows 10. Manifiesto de aplicación embebido por `build.rs` con `/MANIFEST:EMBED /MANIFESTINPUT:` (Common Controls v6, PerMonitorV2, compatibilidad con Windows 10/11).
- **Logo:** `assets/galapagos-logo.png` (recorte del emblema 04 del tablero) cargado con `GdipCreateBitmapFromFile`. El icono de ventana y de barra de tareas sale de `assets/galapagos.ico`, generado una vez con un script de PowerShell (System.Drawing) y cargado con `LoadImageW`.
- **Descartado:**
  - `egui`/`Slint`/`winit`: un segundo sistema de UI cuando Win32 + GDI+ cubre el diseño.
  - Crate `windows` (Direct2D/DirectWrite por COM): dependencia grande; GDI+ basta para v0.1.
  - Barra de título personalizada (`DwmExtendFrameIntoClientArea`): complejidad alta; se usa la barra del sistema en modo oscuro.
  - Animaciones.
- **Prohibido:** afirmaciones de seguridad no medidas (T-02b); HTML, CSS o WebView para la UI del Coordinador (ARC-02).

## 6. Plan de ejecución (un `TARGET_FILE` por paso)

| Paso | TARGET_FILE | Resultado |
|---|---|---|
| 1 | `build.rs` (nuevo) + `assets/galapagos.manifest` | Controles modernos, nitidez en pantallas de alta densidad |
| 2 | `assets/` + `packaging/generate_ui_assets.ps1` | `galapagos-logo.png` y `galapagos.ico` a partir del tablero de marca |
| 3 | `src/window/theme.rs` (nuevo) | Paleta, fuentes, ayudas GDI+ (rectángulo redondeado, texto, icono-glifo) |
| 4 | `src/window/mod.rs` (+ `src/engine/mod.rs`: `set_toolbar_height`; `src/main.rs`: insignia tipada; aprobado por el Director el 2026-10-26) | Barra de título oscura, icono, barra de herramientas dibujada: ← → ⟳ ⌂, campo de dirección redondeado (EDIT con colores del tema y Segoe UI), insignia real, botón Navegar retirado (Enter) |
| 5 | `crates/galapagos-ipc` + `src/engine/mod.rs` + `crates/galapagos-content/src/main.rs` | Mensajes 9, 14–17; atrás, adelante y recargar funcionan; repintado ante `WM_PAINT` |
| 6 | `src/window/dashboard.rs` (nuevo) | Panel: barra lateral (Seguridad, Privacidad, Rendimiento), tarjetas con estado real, gráfico CPU/RAM del worker a 1 Hz |
| 7 | `contracts/galapagos_core_v1.rs` | Retirada de R-18 |

**Pruebas:**
- Lógica pura del tema (contraste mínimo 4.5:1 entre el texto y su fondo) y del búfer circular.
- Codec de los mensajes nuevos.
- Que ninguna insignia diga «SEGURO» o «Protección activa» sin el hecho que lo respalda.
- Que la ventana se crea con el estilo y los atributos DWM esperados.
- La validación visual final la hace el Director.

## 7. Diferido (Cubo B → `CONTRACT_BACKLOG.md` al congelar)

| Código | Descripción |
|---|---|
| `UX-02` | Pestañas múltiples (ya en backlog) |
| `UX-06` | Favoritos (requiere esquema v2, PERS-01) |
| `UX-07` | Barra de título personalizada con el logo integrado |
| `NET-02` | Filtro de rastreadores con `adblock-rust` (requiere T-04b) |
| `CI-01` | `cargo-audit`, `cargo-deny`, `cargo-geiger` en la compilación |

## 8. Decisiones para el Director

| # | Decisión | Recomendación |
|---|---|---|
| D29 | UI con Win32 + GDI+ del sistema (sin `egui`, `Slint` ni el crate `windows`) | Sí |
| D30 | Barra de título del sistema en modo oscuro (no personalizada) en v0.1 | Sí |
| D31 | Añadir atrás, adelante, recargar y repintar (mensajes 14–17 y 9) en este delta | Sí |
| D32 | Retirar el botón «Navegar» (Enter en la barra basta) | Sí |
| D33 | Insignias y tarjetas solo con estados medidos; nada de «SEGURO» o «Protección activa» hasta que sean ciertos | Sí |
| D34 | Logo: emblema circular 04 del tablero de marca, recortado y convertido en PNG e ICO dentro del repositorio | Sí |
| D35 | Retirar del contrato el estado de UI (R-18) en el paso 7 | Sí |

**Contrato Δ8 congelado (2026-10-26): D29–D35 aceptadas, más la regla UI-I0. Cubo B movido a `CONTRACT_BACKLOG.md`. En ejecución.**

**Avance (2026-10-26):** pasos 1–4 HECHOS (manifiesto, logo e icono, `src/window/theme.rs`, barra nativa en `src/window/mod.rs`). Paso 4 probado con ventana por el Director. **Paso 5 HECHO** (sin probar con ventana): mensajes 9 y 14–17 en `galapagos-ipc` (`NavState`, `encode_nav_state`, `decode_nav_state` estricto), variante `ContentToCoordinatorIpc::NavState` en `galapagos_core_v1.rs`, códec en `src/ipc/dispatcher.rs`, `TabCommand` en `src/engine/mod.rs`, botones habilitados por el estado real (`ToolbarButton::enabled_with`) y línea teal de carga en `src/window/mod.rs`, y en el worker `input.rs` (14–17 con payload vacío obligatorio), `delegates.rs` (`notify_history_changed`, `notify_load_status_changed`) y `main.rs` (estado enviado solo si cambia). R-3: el `WM_PAINT` de la superficie se valida en el bucle y pide `MSG_REPAINT`. Atrás/Adelante/Recargar con el panel interno a la vista vuelven a la página del worker. Punto de reanudación: [RETOMAR.md](RETOMAR.md).
