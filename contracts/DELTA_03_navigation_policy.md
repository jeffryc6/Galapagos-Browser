# DELTA 03 — Política de navegación: despacho condicionado y confirmación

**Estado:** CONGELADO (2026-10-25) · D9, D10, D11 aprobadas por el Director (recomendaciones) · **Origen:** auditoría `/znve-audit` de `coordinator.rs` y `main.rs` (2026-10-25), hallazgos S1, S2, S4 · **Extiende:** `src/coordinator.rs` (API pública solo aditiva; `Coordinator::navigate` conserva firma y semántica)

## 0. Hallazgos y evidencia

| Id | Hallazgo | Evidencia | Sev. | Cubo |
|---|---|---|---|---|
| S1 | Un error de `Coordinator::navigate` (p. ej. Behavior Sentinel bloqueando un bucle de redirecciones) solo se registra y **la navegación continúa hacia el motor**. | `main.rs` `dispatch_allowed`: `coordinator.navigate(..)` con `log_warn` y despacho incondicional | Alta | A |
| S2 | Tras `RequireConfirmation` + consentimiento del usuario, `Coordinator::navigate` re-evalúa y devuelve `PermissionDenied`. Corregir S1 sin más rompería la navegación confirmada. | `coordinator.rs` `navigate`: `Block \| RequireConfirmation => Err(denied)` | Alta | A |
| S4 | Ocean Defense se evalúa dos veces con `origin_host` distinto (URL destino en `main`, URL actual en `Coordinator`); pueden discrepar. | `main.rs` `navigate_checked` y `coordinator.rs` `navigate` | Media | B (R11) |

## 1. CONTRATO DE ENTRADA Y SALIDA (aditivo)

```rust
// src/coordinator.rs — adición; `navigate` pasa a ser un envoltorio
impl Coordinator {
    /// Igual que `navigate`, pero `user_confirmed == true` permite continuar SOLO cuando la
    /// evaluación de Ocean Defense devuelve `RequireConfirmation`. Nunca anula `Block` ni
    /// `Unavailable`, ni el veredicto del Behavior Sentinel.
    pub fn navigate_confirmed(&mut self, tab_id: TabId, target_url: String, user_confirmed: bool)
        -> Result<(), GalapagosError>;

    /// `navigate(tab, url)` == `navigate_confirmed(tab, url, false)` (sin cambio observable)
    pub fn navigate(&mut self, tab_id: TabId, target_url: String) -> Result<(), GalapagosError>;
}
```

### Invariantes
- **P-I1** El despacho al motor (`dispatch_navigation`) ocurre **solo después** de `Ok` del Coordinador. Un `Err` no llega al motor y deja el estado visible (etiqueta de estado) y una traza `COORDINATOR_NAVIGATE_FAILED`.
- **P-I2** `user_confirmed == true` solo lo pasa el camino de `main` que acaba de recibir `request_user_confirmation == true` para esa misma URL. Ninguna otra entrada (CLI, `UiToCoordinatorCmd::Navigate`, eventos del worker) puede fijarlo.
- **P-I3** Orden de comprobaciones inalterado: perfil → Behavior Sentinel (veredicto activo y salto) → Ocean Defense. `user_confirmed` solo se consulta en la rama `RequireConfirmation`; `Block` y `Unavailable` mantienen su error actual.
- **P-I4** Una navegación confirmada sigue contando como salto para el Sentinel (`record_navigation_hop`) y reinicia los contadores al completarse, igual que una permitida.

## 2. CONTRATO DE PERSISTENCIA

Sin cambios.

## 3. CONTRATO DE ERRORES

Sin variantes nuevas. `Block`/`RequireConfirmation` sin confirmar ⇒ `PermissionDenied`; `Unavailable` ⇒ `IntegrityVerificationFailed` (como hoy).

## 4. ANTI-BLOAT FENCE

- **Entra:** un método y un parámetro booleano. Cero dependencias, cero tipos nuevos.
- **Descartado:** nueva variante `ConfirmationRequired` en `GalapagosError` (obliga a tocar todos los `match` del contrato); token de consentimiento opaco (sobreingeniería con un solo llamador, ver D10).
- **Prohibido:** que cualquier canal derivado del contenido web pueda alcanzar `user_confirmed`.

## 5. Plan de ejecución (un `TARGET_FILE` por paso)

| Paso | TARGET_FILE | Prueba |
|---|---|---|
| 1 | `src/coordinator.rs` (`navigate_confirmed`, `navigate` como envoltorio) | `RequireConfirmation` + confirmado ⇒ Ok; sin confirmar ⇒ Err; `Block` + confirmado ⇒ Err; Sentinel bloqueado + confirmado ⇒ Err; salto contado |
| 2 | `src/main.rs` (`dispatch_allowed` no despacha tras `Err`; ruta confirmada usa `navigate_confirmed(.., true)`; estado visible) | `navigate_checked` con motor ausente: host bloqueado no despacha; ruta de error no llama al motor (función auxiliar pura testeable) |
| 3 | R11 (opcional, ver §6) | — |

## 6. R11 — doble evaluación (diferido hasta cerrar 1-2)

Con P-I1 y `navigate_confirmed` cerrados, la segunda evaluación deja de ser un riesgo de seguridad y pasa a ser un costo (~µs) y una discrepancia de `origin_host`. Opciones:
- **R11-a (recomendada):** conservar ambas y alinear `origin_host` (en `main` pasar la URL actual de la pestaña, como hace el Coordinador). Cambio de una línea, sin API nueva.
- **R11-b:** el Coordinador expone `preflight_navigation` y `main` deja de evaluar. Más código y una API más; solo si se quiere una única fuente de verdad.

## 7. Decisiones para el Director

| # | Decisión | Recomendación |
|---|---|---|
| D9 | `navigate_confirmed(.., bool)` vs. nueva variante de error `ConfirmationRequired` | `navigate_confirmed` (§4) |
| D10 | Booleano simple vs. token opaco de consentimiento | Booleano; reevaluar si aparece un segundo llamador |
| D11 | R11: opción a (alinear `origin_host`), b (preflight) o dejar tal cual | R11-a, tras los pasos 1-2 |

**Ejecución (2026-10-25):** pasos 1 y 2 y R11-a hechos y verificados con `cargo test` (87 lib, 89 bin, 6 y 3 de integración). Nota de alcance: con la política actual, `RequestGuard.evaluate(Navigate)` no devuelve `RequireConfirmation` (solo lo hacen las descargas), así que S2 es hoy latente: `navigate_confirmed` queda cubierto por la tabla `may_proceed` y no por una navegación real. Sigue **NO INTEGRADO** (sin smoke test contra binario real, por decisión del Director).
