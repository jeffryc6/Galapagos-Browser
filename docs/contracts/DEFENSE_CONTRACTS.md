# DEFENSE CONTRACTS — Galapagos Browser v0.1

**Estado:** Contrato Formal de Evaluación Defensiva (Fase B · Tarea 7)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Implementación Ejecutable:** [src/defense/request_guard.rs](file:///d:/App/Browser-SaveNav/src/defense/request_guard.rs)  

---

## 1. Axiomas del Modelo Defensivo (ZNVE-06 / ZNVE-07)

1. **Determinismo Local (Offline-first):** La evaluación de seguridad se ejecuta de forma síncrona en memoria mediante reglas explícitas y bases de datos locales verificadas. Cero consultas de red en el camino crítico.
2. **Invariante de Fallo Incompleto (`Unavailable != Allow`):** Si una verificación defensiva obligatoria no puede completarse (por ejemplo, firma corrupta o base de datos no disponible), el resultado es `Unavailable`. **`Unavailable` jamás se tratará como `Allow` de forma implícita**.
3. **Cero Heurísticas Opacas:** No se admiten decisiones tomadas por "puntuaciones de riesgo" inexplicables o cajas negras de IA en la ruta caliente de navegación.

---

## 2. Matriz de Resultados de Decisión

| Decisión (`DefenseDecision`) | Semántica de Seguridad | Acción del Coordinador | Comportamiento en UI |
|---|---|---|---|
| **`Allow`** | La solicitud cumple todas las reglas de seguridad y privacidad vigentes. | Despacha la solicitud al motor Servo (`WebEnginePort`). | Carga normal de la página. |
| **`Block`** | La acción viola una regla determinista o coincide con una amenaza confirmada. | Aborta la solicitud inmediatamente; no se transmiten bytes por la red. | Muestra pantalla nativa de bloqueo con el `reason_code`. |
| **`RequireConfirmation`** | Acción de alto riesgo que requiere consentimiento explícito e informado del usuario. | Pausa la operación hasta recibir un comando de confirmación del usuario. | Despliega diálogo modal nativo Win32 (ej.: descarga de archivo ejecutable). |
| **`Restrict`** | La acción se autoriza con capacidades acotadas para mitigar riesgos. | Modifica la solicitud (ej.: omite cabeceras de tracking, bloquea WebRTC o cookies 3P). | Muestra indicador de protección en la barra de direcciones. |
| **`Unavailable`** | El motor de evaluación no pudo verificar el recurso o la regla está corrupta. | Aplica la política de fallo seguro (`fail-secure`): se bloquea o suspende la acción. | Informa al usuario que la protección no pudo certificar el origen. |

---

## 3. Especificación Formal de Entrada y Salida

### 3.1. Estructura de Entrada (`DefenseEvaluationInput`)
```rust
pub struct DefenseEvaluationInput<'a> {
    /// Contexto inmutable del origen de la petición
    pub context: &'a DefenseContext,
    /// Acción sujeta a escrutinio (Navegación, Subrecurso, Descarga)
    pub action: &'a DefenseAction,
    /// Señales locales precalculadas (reputación en caché, procedencia)
    pub available_signals: LocalSecuritySignals,
}
```

### 3.2. Estructura de Salida (`DefenseEvaluation`)
```rust
pub struct DefenseEvaluation {
    /// Veredicto determinista inmutable
    pub decision: DefenseDecision,
    /// Código identificador de la causa (máx. 48 caracteres alfanuméricos)
    pub reason_code: &'static str,
    /// Identificador de la regla auditada que disparó el veredicto
    pub applied_rule: &'static str,
}
```

---

## 4. Catálogo de Reglas y Códigos de Motivo del MVP

| Código de Regla | Motivo (`reason_code`) | Decisión Asociada | Descripción |
|---|---|---|---|
| `RULE-CORE-00-INTERNAL-SAFE` | `INTERNAL_ABOUT_BLANK` | `Allow` | Navegación segura hacia el esquema interno local `about:blank`. |
| `RULE-SEC-01-SCHEME-VALIDATION` | `INVALID_URL_SCHEME` | `Block` | Rechazo de esquemas no autorizados (`javascript:`, `file:`, `data:`). |
| `RULE-SEC-02-HOST-PARSING` | `MALFORMED_HOST` | `Block` | Host ausente, caracteres de control o formato inválido RFC 3986. |
| `RULE-THREAT-01-LOCAL-BLOCKLIST` | `THREAT_INTEL_MALICIOUS_HOST` | `Block` | Coincidencia en la lista local de phishing o malware autenticado. |
| `RULE-TRACKER-01-THIRD-PARTY-SHIELD` | `THIRD_PARTY_TRACKER_BLOCKED` | `Block` | Dominio de telemetría/rastreo bloqueado en contexto de tercero. |
| `RULE-PRIV-03-HTTPS-ONLY-ENFORCEMENT` | `HARDENED_PLAINTEXT_HTTP_RESTRICTED` | `Restrict` | Perfil `Hardened` restringe el transporte HTTP sin cifrar. |
| `RULE-DOWNLOAD-01-EXECUTABLE-BARRIER` | `EXECUTABLE_PAYLOAD_CONFIRMATION_REQUIRED` | `RequireConfirmation` | Descarga de payload ejecutable (`.exe`, `.msi`, `.bat`, etc.). |
| `RULE-DEFAULT-ALLOW-VERIFIED` | `NAVIGATION_POLICY_SATISFIED` | `Allow` | Navegación verificada conforme a la política predeterminada. |

---

## 5. Restricciones Inviolables de Implementación

1. **Prohibición de Modificación de Estado:** Una evaluación defensiva es una función pura o de solo lectura. No puede mutar el perfil del usuario, ni alterar la base de datos de historial durante la evaluación.
2. **Prohibición de Filtración:** No se transmitirán hashes de las URLs navegadas a servicios de terceros para "consultar reputación" en tiempo real sin consentimiento explícito y anonimización de k-anonimato previa.
3. **Integridad de Artefactos:** Toda lista de filtrado o conjunto de firmas consumido por Ocean Defense debe validarse previamente contra su firma Ed25519 registrada en la tabla `verified_artifacts`.
