# DEFENSE POLICY MODEL — Galapagos Browser v0.1

**Estado:** Modelo de Políticas, Excepciones y Reglas Defensivas (Fase C · Tarea 9)  
**Estándar de Gobernanza:** Zero-Noise Vibe Engineering (ZNVE v2.4.0)  
**Contrato Maestro:** [contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md](file:///d:/App/Browser-SaveNav/contracts/GALAPAGOS%20%E2%80%94%20Arquitectura%20integral%20bajo%20Zero-Noise%20Vibe%20Engineering%20%28ZNVE%29.md)  
**Contrato Formal:** [docs/contracts/DEFENSE_CONTRACTS.md](file:///d:/App/Browser-SaveNav/docs/contracts/DEFENSE_CONTRACTS.md)  

---

## 1. Orden de Precedencia y Evaluación de Políticas

Para garantizar decisiones deterministas y predecibles en menos de 1 milisegundo, las reglas se evalúan en una tubería secuencial estricta:

```
[Solicitud de Navegación / Recurso]
                │
                ▼
      Prioridad 1: Integridad y Sintaxis de Frontera (ARC-02)
      Esquema válido (https/http/about:blank) y longitud <= 2048 bytes
                │
                ├── Fallo ──► Block [INVALID_URL_SCHEME / PAYLOAD_TOO_LARGE]
                ▼
      Prioridad 2: Excepciones Explícitas del Usuario (Freedom Island)
      Consulta en site_permissions (grant_state: AllowExplicitRevocable / Deny)
                │
                ├── Deny ───► Block [USER_RULE_EXPLICIT_DENY]
                ├── Allow ──► Allow [USER_RULE_EXPLICIT_ALLOW]
                ▼
      Prioridad 3: Threat Intelligence Local (Threat Intel)
      Lista local de malware, phishing y dominios bloqueados autenticados
                │
                ├── Coincidencia ──► Block [THREAT_INTEL_MALICIOUS_HOST]
                ▼
      Prioridad 4: Anti-Tracking Shield (Privacidad de Contexto)
      Verificación de subrecursos y dominios rastreadores (is_third_party)
                │
                ├── Rastreador 3P ──► Block [THIRD_PARTY_TRACKER_BLOCKED]
                ▼
      Prioridad 5: Endurecimiento por Perfil (Privacy Island)
      Nivel Hardened: HTTPS-Only enforcement (bloqueo de HTTP plano)
                │
                ├── HTTP plano ────► Restrict [HARDENED_PLAINTEXT_HTTP_RESTRICTED]
                ▼
      Prioridad 6: Barrera de Carga Útil (Payload Guard)
      Verificación de extensiones ejecutables en descargas
                │
                ├── Ejecutable ────► RequireConfirmation [EXECUTABLE_PAYLOAD_CONFIRMATION_REQUIRED]
                ▼
      Decisión Predeterminada: Allow [NAVIGATION_POLICY_SATISFIED]
```

---

## 2. Modelo de Excepciones Justificadas (Freedom Island)

El usuario mantiene el control soberano sobre su navegación (`ZNVE Pilar 5`):
1. **Consentimiento Explícito:** Cuando una solicitud es bloqueada o restringida, la UI expone el motivo técnico exacto (`reason_code`) y la regla aplicada.
2. **Revocabilidad Inmediata:** Si el usuario decide permitir un origen previamente bloqueado, se genera un registro en la tabla `site_permissions`:
   ```sql
   INSERT INTO site_permissions (profile_id, origin_host, capability_code, grant_state, updated_at_epoch_s)
   VALUES (?1, ?2, ?3, 2, ?4);
   ```
3. **Restricción de Alcance:** Una excepción de usuario solo aplica al `ProfileId` activo; no debilita las defensas de otros perfiles ni afecta sesiones privadas efímeras.

---

## 3. Actualización Autenticada de Listas de Amenazas (SEC-03)

Las listas de filtrado y reputación no se descargan de forma arbitraria ni se procesan sin verificación criptográfica:

1. **Estructura del Manifiesto Firmado (`SignedArtifactManifest`):**
   * `artifact_id: String` (ej.: `tracking-rules-v1`)
   * `version_semver: (u16, u16, u16)`
   * `sha256_digest: [u8; 32]`
   * `ed25519_signature: [u8; 64]`
2. **Proceso de Admisión:**
   * El archivo se descarga en un directorio temporal aislado.
   * Se computa su digest SHA-256. Si no coincide con el manifiesto, se descarta (`IntegrityVerificationFailed`).
   * Se verifica la firma Ed25519 contra la clave pública maestra de Galapagos preembebida en el binario del Coordinador.
   * Tras la verificación exitosa, se inserta en `verified_artifacts` y se carga en el conjunto en memoria de `RequestGuard`.

---

## 4. Comportamiento Determinista ante Fallos de Evaluación

* Si la base de datos de firmas locales está inaccesible o bloqueada, la evaluación de esa regla retorna `DefenseDecision::Unavailable`.
* **Regla de oro ZNVE:** En operaciones críticas (como descargas o peticiones que involucran permisos de hardware), `Unavailable` se trata como bloqueo seguro (`Fail-Secure`), requiriendo confirmación del usuario para proceder.
