# Registro de Decisiones de Arquitectura (ADR) — Galapagos Browser

**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  

Este directorio contiene las decisiones arquitectónicas que delimitan las fronteras de diseño, las restricciones de plataforma y la Anti-Bloat Fence de Galapagos.

---

## Estructura Estándar de un ADR
Todo ADR en Galapagos se redacta bajo la siguiente plantilla inmutable:

1. **Título:** `ADR-XXX: <Título descriptivo>`
2. **Estado:** `Propuesto` | `Aceptado` | `Rechazado` | `Superado`
3. **Contexto:** Problema técnico o frontera que motiva la decisión.
4. **Decisión:** Elección arquitectónica adoptada con justificación ZNVE.
5. **Alternativas Consideradas:** Opciones descartadas y motivos técnicos del descarte.
6. **Consecuencias:**
   * Positivas (garantías de huella y seguridad).
   * Negativas / Desafíos (complejidad FFI, pruebas de paridad).
7. **Evidencia y Validación:** Pruebas o contratos que certifican la decisión.

---

## Índice Maestro de Decisiones

| ID | Título | Estado | Enfoque ZNVE |
|---|---|---|---|
| [ADR-001](file:///d:/App/Browser-SaveNav/docs/architecture/adr/ADR-001-browser-engine.md) | Selección de Servo como Motor Web Principal | Aceptado | Rendimiento en Rust, huella reducida, cero Chromium. |
| [ADR-002](file:///d:/App/Browser-SaveNav/docs/architecture/adr/ADR-002-process-isolation.md) | Aislamiento de Procesos en Windows vía AppContainer | Aceptado | Zero-trust: procesos de renderizado enjaulados sin acceso a disco. |
| [ADR-003](file:///d:/App/Browser-SaveNav/docs/architecture/adr/ADR-003-native-ui.md) | Interfaz Nativa de Escritorio Win32 / WinUI 3 | Aceptado | Interfaz desacoplada del motor web, cero Electron. |
| [ADR-004](file:///d:/App/Browser-SaveNav/docs/architecture/adr/ADR-004-persistence.md) | Persistencia Híbrida: SQLite WAL y RAM Efímera | Aceptado | `PRAGMA journal_mode=WAL` para Personal; `:memory:` para efímeros. |
| [ADR-005](file:///d:/App/Browser-SaveNav/docs/architecture/adr/ADR-005-defense-pipeline.md) | Pipeline Defensivo Ocean Defense Determinista | Aceptado | Evaluación síncrona de reglas locales; cero IA en el camino crítico. |
| [ADR-006](file:///d:/App/Browser-SaveNav/docs/architecture/adr/ADR-006-update-security.md) | Integridad Criptográfica de Listas y Artefactos | Aceptado | Manifiestos firmados con Ed25519 y digest SHA-256 inmutable. |
