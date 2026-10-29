//! src/defense/request_guard.rs
//! Request Guard — Evaluación síncrona de solicitudes en Galapagos Ocean Defense
//! Conforme al Bloque 1.6 C de contracts/GALAPAGOS — Arquitectura integral bajo Zero-Noise Vibe Engineering (ZNVE).md

#![forbid(unsafe_code)]

use crate::defense::payload_guard::PayloadGuard;
use crate::galapagos_core_v1::{PrivacyLevel, ProfileId, UiToCoordinatorCmd};
use std::collections::HashSet;

/// Resultado explícito de evaluación defensiva según contrato ZNVE
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefenseDecision {
    /// La solicitud puede continuar según la política aplicable
    Allow,
    /// La acción se rechaza por una regla definida
    Block,
    /// Hace falta una decisión explícita del usuario antes de continuar
    RequireConfirmation,
    /// Se aplica una limitación específica compatible con las capacidades disponibles
    Restrict,
    /// No se puede evaluar una parte de la protección (NUNCA se interpreta como Allow)
    Unavailable,
}

/// Acción que solicita someterse a evaluación defensiva
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefenseAction {
    Navigate { target_url: String },
    SubresourceFetch { url: String, is_third_party: bool },
    DownloadPayload { url: String, filename: String },
}

/// Contexto inmutable de la solicitud
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefenseContext {
    pub profile_id: ProfileId,
    pub privacy_level: PrivacyLevel,
    pub origin_host: String,
}

/// Salida inmutable de evaluación de Ocean Defense
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefenseEvaluation {
    pub decision: DefenseDecision,
    pub reason_code: &'static str,
    pub applied_rule: &'static str,
}

fn normalize_list_entry(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// T-04 (INC-20261025-CORE-DEBLOAT): host de una URL http(s) tal como lo resolverá el motor.
/// La autoridad termina en `/ ? # \`; se descartan userinfo y puerto, el punto final y las
/// mayúsculas. Un host con `%` se rechaza (el motor lo decodificaría y evitaría la lista).
fn extract_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let end = rest
        .bytes()
        .position(|b| matches!(b, b'/' | b'?' | b'#' | b'\\'))
        .unwrap_or(rest.len());
    let authority = &rest[..end];
    let host_port = match authority.rfind('@') {
        Some(i) => &authority[i + 1..],
        None => authority,
    };
    let host = if host_port.starts_with('[') {
        &host_port[..=host_port.find(']')?]
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    let host = host.trim_end_matches('.');
    if host.is_empty() || host.contains('%') {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

/// `host` o alguno de sus dominios padre está en `list` (`a.b.evil.com` coincide con `evil.com`).
/// Las IP (IPv4 o IPv6 entre corchetes) solo coinciden de forma exacta.
fn matches_domain(list: &HashSet<String>, host: &str) -> bool {
    // Un nombre de dominio nunca termina en dígito: solo entonces se intenta leer como IPv4
    let maybe_ipv4 = host.as_bytes().last().is_some_and(u8::is_ascii_digit);
    if host.starts_with('[') || (maybe_ipv4 && host.parse::<std::net::Ipv4Addr>().is_ok()) {
        return list.contains(host);
    }
    let mut candidate = host;
    loop {
        if list.contains(candidate) {
            return true;
        }
        match candidate.find('.') {
            Some(i) => candidate = &candidate[i + 1..],
            None => return false,
        }
    }
}

/// Guardián de solicitudes de navegación y recursos de red
pub struct RequestGuard {
    blocked_hosts: HashSet<String>,
    blocked_trackers: HashSet<String>,
    payload_guard: PayloadGuard,
}

impl Default for RequestGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestGuard {
    /// Inicializa una instancia limpia de RequestGuard con reglas base
    pub fn new() -> Self {
        // Sin listas precargadas: los dominios de relleno (`telemetry.badtracker.com`, …) eran datos
        // de prueba en producción. Las listas reales llegan por `block_host` /
        // `register_tracker_domain` (feed firmado SEC-03).
        Self {
            blocked_hosts: HashSet::new(),
            blocked_trackers: HashSet::new(),
            payload_guard: PayloadGuard::new(),
        }
    }

    /// Registra un host malicioso o bloqueado en la lista local de reputación (Threat Intelligence).
    /// Bloquea también sus subdominios.
    pub fn block_host(&mut self, host: String) {
        self.blocked_hosts.insert(normalize_list_entry(&host));
    }

    /// Registra un dominio de rastreo conocido (Anti-Tracking Shield), subdominios incluidos
    pub fn register_tracker_domain(&mut self, domain: String) {
        self.blocked_trackers.insert(normalize_list_entry(&domain));
    }

    /// Evalúa síncronamente una acción conforme a políticas deterministas locales (ZNVE-06)
    pub fn evaluate(&self, ctx: &DefenseContext, action: &DefenseAction) -> DefenseEvaluation {
        match action {
            DefenseAction::Navigate { target_url } => self.evaluate_navigation(ctx, target_url),
            DefenseAction::SubresourceFetch { url, is_third_party } => {
                self.evaluate_subresource(ctx, url, *is_third_party)
            }
            DefenseAction::DownloadPayload { url, filename } => {
                self.evaluate_download(ctx, url, filename)
            }
        }
    }

    fn evaluate_navigation(&self, ctx: &DefenseContext, url: &str) -> DefenseEvaluation {
        // 1. Validación de esquema conforme a contratos ARC-02 / SEC-01
        if let Err(_reason) = UiToCoordinatorCmd::validate_url_scheme(url) {
            return DefenseEvaluation {
                decision: DefenseDecision::Block,
                reason_code: "INVALID_URL_SCHEME",
                applied_rule: "RULE-SEC-01-SCHEME-VALIDATION",
            };
        }

        if url == "about:blank" {
            return DefenseEvaluation {
                decision: DefenseDecision::Allow,
                reason_code: "INTERNAL_ABOUT_BLANK",
                applied_rule: "RULE-CORE-00-INTERNAL-SAFE",
            };
        }

        // 2. Extracción y verificación de host contra Threat Intelligence local
        let host = match extract_host(url) {
            Some(h) => h,
            None => {
                return DefenseEvaluation {
                    decision: DefenseDecision::Block,
                    reason_code: "MALFORMED_HOST",
                    applied_rule: "RULE-SEC-02-HOST-PARSING",
                };
            }
        };

        if matches_domain(&self.blocked_hosts, &host) {
            return DefenseEvaluation {
                decision: DefenseDecision::Block,
                reason_code: "THREAT_INTEL_MALICIOUS_HOST",
                applied_rule: "RULE-THREAT-01-LOCAL-BLOCKLIST",
            };
        }

        // 3. Política de endurecimiento de transporte (PRIV-01 / Hardened)
        if ctx.privacy_level == PrivacyLevel::Hardened && url.starts_with("http://") {
            return DefenseEvaluation {
                decision: DefenseDecision::Restrict,
                reason_code: "HARDENED_PLAINTEXT_HTTP_RESTRICTED",
                applied_rule: "RULE-PRIV-03-HTTPS-ONLY-ENFORCEMENT",
            };
        }

        DefenseEvaluation {
            decision: DefenseDecision::Allow,
            reason_code: "NAVIGATION_POLICY_SATISFIED",
            applied_rule: "RULE-DEFAULT-ALLOW-VERIFIED",
        }
    }

    fn evaluate_subresource(
        &self,
        ctx: &DefenseContext,
        url: &str,
        is_third_party: bool,
    ) -> DefenseEvaluation {
        let host = match extract_host(url) {
            Some(h) => h,
            None => {
                return DefenseEvaluation {
                    decision: DefenseDecision::Block,
                    reason_code: "SUBRESOURCE_MALFORMED_HOST",
                    applied_rule: "RULE-SEC-03-SUBRESOURCE-HOST",
                };
            }
        };

        // Bloqueo de rastreadores y analíticas de terceros (Anti-Tracking Shield)
        if is_third_party && matches_domain(&self.blocked_trackers, &host) {
            return DefenseEvaluation {
                decision: DefenseDecision::Block,
                reason_code: "THIRD_PARTY_TRACKER_BLOCKED",
                applied_rule: "RULE-TRACKER-01-THIRD-PARTY-SHIELD",
            };
        }

        // En perfil Hardened, los subrecursos de terceros en HTTP sin cifrar se restringen
        if ctx.privacy_level == PrivacyLevel::Hardened
            && is_third_party
            && url.starts_with("http://")
        {
            return DefenseEvaluation {
                decision: DefenseDecision::Restrict,
                reason_code: "HARDENED_THIRD_PARTY_PLAINTEXT_RESTRICTED",
                applied_rule: "RULE-PRIV-04-HARDENED-SUBRESOURCE",
            };
        }

        DefenseEvaluation {
            decision: DefenseDecision::Allow,
            reason_code: "SUBRESOURCE_ALLOWED",
            applied_rule: "RULE-DEFAULT-ALLOW-SUBRESOURCE",
        }
    }

    fn evaluate_download(
        &self,
        _ctx: &DefenseContext,
        _url: &str,
        filename: &str,
    ) -> DefenseEvaluation {
        self.payload_guard.inspect_payload(filename, None, &[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_context(privacy_level: PrivacyLevel) -> DefenseContext {
        DefenseContext {
            profile_id: ProfileId(1),
            privacy_level,
            origin_host: "example.com".to_string(),
        }
    }

    #[test]
    fn test_navigation_evaluation_allow_and_block() {
        let mut guard = RequestGuard::new();
        guard.block_host("phishing.evil.com".to_string());

        let ctx = dummy_context(PrivacyLevel::Daily);

        // 1. Navegación válida HTTPS
        let res = guard.evaluate(
            &ctx,
            &DefenseAction::Navigate {
                target_url: "https://servo.org/docs".to_string(),
            },
        );
        assert_eq!(res.decision, DefenseDecision::Allow);

        // 2. Navegación a host malicioso bloqueado en Threat Intelligence
        let res_blocked = guard.evaluate(
            &ctx,
            &DefenseAction::Navigate {
                target_url: "https://phishing.evil.com/login".to_string(),
            },
        );
        assert_eq!(res_blocked.decision, DefenseDecision::Block);
        assert_eq!(res_blocked.reason_code, "THREAT_INTEL_MALICIOUS_HOST");

        // 3. Esquema inválido (e.g. javascript:)
        let res_invalid = guard.evaluate(
            &ctx,
            &DefenseAction::Navigate {
                target_url: "javascript:alert(1)".to_string(),
            },
        );
        assert_eq!(res_invalid.decision, DefenseDecision::Block);
        assert_eq!(res_invalid.reason_code, "INVALID_URL_SCHEME");
    }

    /// INC-20261025-CORE-DEBLOAT T-04: variantes de URL que antes evitaban la blocklist
    #[test]
    fn test_blocklist_evasion_variants_are_blocked_inc20261025() {
        let mut guard = RequestGuard::new();
        guard.block_host("phishing.evil.com".to_string());
        let ctx = dummy_context(PrivacyLevel::Daily);
        let nav = |url: &str| {
            guard.evaluate(&ctx, &DefenseAction::Navigate { target_url: url.to_string() }).decision
        };

        for url in [
            "https://phishing.evil.com/login",
            "https://phishing.evil.com?x=1",
            "https://phishing.evil.com#frag",
            "https://phishing.evil.com./",
            "https://PHISHING.evil.com:8443/",
            "https://a.b.phishing.evil.com/",
            "https://phishing.evil.com\\login",
            "http://phishing.evil.com.:80?q",
            "https://phishing%2Eevil.com/",
        ] {
            assert_eq!(nav(url), DefenseDecision::Block, "{}", url);
        }

        // Sin sobrebloqueo: padres, hermanos y dominios que solo comparten el texto
        for url in [
            "https://evil.com/",
            "https://other.evil.com/",
            "https://notphishing.evil.com/",
            "https://phishing.evil.company/",
            "https://[::1]:8080/",
        ] {
            assert_eq!(nav(url), DefenseDecision::Allow, "{}", url);
        }
    }

    /// T-04: el escudo de rastreadores cubre subdominios y variantes de autoridad
    #[test]
    fn test_tracker_subdomain_and_query_variants_blocked_inc20261025() {
        let mut guard = RequestGuard::new();
        guard.register_tracker_domain("telemetry.badtracker.com".to_string());
        let ctx = dummy_context(PrivacyLevel::Daily);
        for url in [
            "https://cdn.telemetry.badtracker.com/p.js",
            "https://telemetry.badtracker.com?id=1",
            "https://telemetry.badtracker.com./x",
        ] {
            let eval = guard.evaluate(&ctx, &DefenseAction::SubresourceFetch { url: url.to_string(), is_third_party: true });
            assert_eq!(eval.decision, DefenseDecision::Block, "{}", url);
        }
    }

    #[test]
    fn test_hardened_privacy_level_http_restriction() {
        let guard = RequestGuard::new();
        let ctx_hardened = dummy_context(PrivacyLevel::Hardened);
        let ctx_daily = dummy_context(PrivacyLevel::Daily);

        // En perfil Hardened, HTTP plano debe ser Restrict
        let res_hardened = guard.evaluate(
            &ctx_hardened,
            &DefenseAction::Navigate {
                target_url: "http://insecure-news.org".to_string(),
            },
        );
        assert_eq!(res_hardened.decision, DefenseDecision::Restrict);
        assert_eq!(res_hardened.reason_code, "HARDENED_PLAINTEXT_HTTP_RESTRICTED");

        // En perfil Daily, HTTP plano está permitido
        let res_daily = guard.evaluate(
            &ctx_daily,
            &DefenseAction::Navigate {
                target_url: "http://insecure-news.org".to_string(),
            },
        );
        assert_eq!(res_daily.decision, DefenseDecision::Allow);
    }

    #[test]
    fn test_anti_tracking_third_party_shield() {
        let mut guard = RequestGuard::new();
        guard.register_tracker_domain("telemetry.badtracker.com".to_string());
        let ctx = dummy_context(PrivacyLevel::Daily);

        // Subrecurso de rastreador third-party debe ser bloqueado
        let res_tracker = guard.evaluate(
            &ctx,
            &DefenseAction::SubresourceFetch {
                url: "https://telemetry.badtracker.com/collect".to_string(),
                is_third_party: true,
            },
        );
        assert_eq!(res_tracker.decision, DefenseDecision::Block);
        assert_eq!(res_tracker.reason_code, "THIRD_PARTY_TRACKER_BLOCKED");

        // Mismo subrecurso como first-party no se bloquea por la regla third-party
        let res_first_party = guard.evaluate(
            &ctx,
            &DefenseAction::SubresourceFetch {
                url: "https://telemetry.badtracker.com/collect".to_string(),
                is_third_party: false,
            },
        );
        assert_eq!(res_first_party.decision, DefenseDecision::Allow);
    }

    #[test]
    fn test_executable_payload_download_guard() {
        let guard = RequestGuard::new();
        let ctx = dummy_context(PrivacyLevel::Daily);

        // Descarga de archivo ejecutable requiere confirmación modal
        let res_exe = guard.evaluate(
            &ctx,
            &DefenseAction::DownloadPayload {
                url: "https://software.org/setup.exe".to_string(),
                filename: "setup.exe".to_string(),
            },
        );
        assert_eq!(res_exe.decision, DefenseDecision::RequireConfirmation);
        assert_eq!(res_exe.reason_code, "EXECUTABLE_PAYLOAD_CONFIRMATION_REQUIRED");

        // Descarga de documento seguro se permite directamente
        let res_pdf = guard.evaluate(
            &ctx,
            &DefenseAction::DownloadPayload {
                url: "https://software.org/whitepaper.pdf".to_string(),
                filename: "whitepaper.pdf".to_string(),
            },
        );
        assert_eq!(res_pdf.decision, DefenseDecision::Allow);
    }
}
