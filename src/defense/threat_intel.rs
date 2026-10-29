//! src/defense/threat_intel.rs
//! Galapagos Ocean Defense — Threat Intelligence Feed Updater (F4.3)
//! Gobernado bajo estándar ZNVE v2.4.0 (Axiomas 1 y 2, SEC-01, SEC-03, DEFENSE_POLICY_MODEL.md).
//!
//! Responsabilidades:
//! 1. Verificación Criptográfica Local: Validación estricta de listas locales de reputación
//!    mediante `SignedArtifactManifest` (digest SHA-256 y firma digital Ed25519 según SEC-03).
//! 2. Actualización en Caliente Atómica: Conmutación atómica de conjuntos de reglas en
//!    `RequestGuard` sin reinicio del navegador ni bloqueo del hilo de navegación.
//! 3. Protección Anti-Downgrade: Rechazo estricto de paquetes con número de versión inferior.
//! 4. Auditoría de Artefactos: Registro atómico en la tabla `verified_artifacts` de SQLite.

#![forbid(unsafe_code)]

use crate::defense::request_guard::RequestGuard;
use crate::galapagos_core_v1::{GalapagosError, SignedArtifactManifest};
use crate::storage::StorageEngine;
use std::collections::HashSet;

/// Clave pública maestra preembebida de Galapagos para verificación de firmas Ed25519 (SEC-03)
pub const GALAPAGOS_MASTER_PUBKEY: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7,
    0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25,
    0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

/// Resumen inmutable emitido tras la verificación e instalación exitosa de un feed
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreatFeedUpdateSummary {
    pub artifact_id: String,
    pub version_semver: (u16, u16, u16),
    pub hosts_added: usize,
    pub trackers_added: usize,
    pub sha256_hex: String,
}

/// Actualizador y Validador en Caliente de Feeds de Inteligencia de Amenazas
pub struct ThreatIntelFeedUpdater {
    master_public_key: [u8; 32],
    current_version: (u16, u16, u16),
    verified_hosts: HashSet<String>,
    verified_trackers: HashSet<String>,
}

impl Default for ThreatIntelFeedUpdater {
    fn default() -> Self {
        Self::with_default_master_key()
    }
}

impl ThreatIntelFeedUpdater {
    /// Inicializa con clave pública maestra explícita
    pub fn new(master_public_key: [u8; 32]) -> Self {
        Self {
            master_public_key,
            current_version: (0, 0, 0),
            verified_hosts: HashSet::new(),
            verified_trackers: HashSet::new(),
        }
    }

    /// Inicializa con la clave pública maestra por defecto de Galapagos
    pub fn with_default_master_key() -> Self {
        Self::new(GALAPAGOS_MASTER_PUBKEY)
    }

    /// Retorna la versión actual instalada de las listas de amenazas
    pub fn current_version(&self) -> (u16, u16, u16) {
        self.current_version
    }

    /// Proceso de Admisión y Actualización en Caliente Atómica (SEC-03)
    pub fn verify_and_apply_manifest(
        &mut self,
        manifest: &SignedArtifactManifest,
        feed_content: &[u8],
        request_guard: &mut RequestGuard,
        storage: Option<&StorageEngine>,
    ) -> Result<ThreatFeedUpdateSummary, GalapagosError> {
        // 1. Barrera Anti-Downgrade: La versión debe ser monótonamente mayor o igual
        if manifest.version_semver < self.current_version {
            return Err(GalapagosError::IntegrityVerificationFailed {
                artifact_id: manifest.artifact_id.clone(),
            });
        }

        // 2. Verificación criptográfica del digest SHA-256
        let computed_digest = compute_sha256(feed_content);
        if computed_digest != manifest.sha256_digest {
            return Err(GalapagosError::IntegrityVerificationFailed {
                artifact_id: manifest.artifact_id.clone(),
            });
        }

        // 3. Verificación de firma digital Ed25519 contra la clave maestra
        if !verify_ed25519_signature(
            &self.master_public_key,
            &manifest.sha256_digest,
            &manifest.ed25519_signature,
        ) {
            return Err(GalapagosError::IntegrityVerificationFailed {
                artifact_id: manifest.artifact_id.clone(),
            });
        }

        // 4. Parseo determinista del feed verificado (cero dependencias externas)
        let feed_text = std::str::from_utf8(feed_content).map_err(|_| {
            GalapagosError::IntegrityVerificationFailed {
                artifact_id: manifest.artifact_id.clone(),
            }
        })?;

        let mut new_hosts = Vec::new();
        let mut new_trackers = Vec::new();

        for line in feed_text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some(tracker) = trimmed.strip_prefix("tracker:") {
                let tracker_clean = tracker.trim().to_ascii_lowercase();
                if !tracker_clean.is_empty() {
                    new_trackers.push(tracker_clean);
                }
            } else {
                new_hosts.push(trimmed.to_ascii_lowercase());
            }
        }

        let hosts_count = new_hosts.len();
        let trackers_count = new_trackers.len();

        // 5. Auditoría SEC-03 antes de conmutar: si no queda registrada, no se aplica nada
        let sha256_hex = hex_encode(&computed_digest);
        if let Some(engine) = storage {
            let verified_at_epoch_s = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64);
            engine.insert_verified_artifact(
                &manifest.artifact_id,
                manifest.version_semver.0,
                manifest.version_semver.1,
                manifest.version_semver.2,
                &sha256_hex,
                verified_at_epoch_s,
            )?;
        }

        // 6. Conmutación en caliente en memoria dentro de RequestGuard
        for h in &new_hosts {
            request_guard.block_host(h.clone());
            self.verified_hosts.insert(h.clone());
        }

        for t in &new_trackers {
            self.verified_trackers.insert(t.clone());
        }

        // 7. Congelar nueva versión activa
        self.current_version = manifest.version_semver;

        Ok(ThreatFeedUpdateSummary {
            artifact_id: manifest.artifact_id.clone(),
            version_semver: manifest.version_semver,
            hosts_added: hosts_count,
            trackers_added: trackers_count,
            sha256_hex,
        })
    }
}

// ============================================================================
// CRIPTOGRAFÍA NATIVA DETERMINISTA (Anti-Bloat Fence / ZNVE Guardrail 1)
// Cero dependencias externas. Implementación pura, auditada y segura en Rust.
// ============================================================================

/// Computa el digest criptográfico SHA-256 conforme a NIST FIPS 180-4
pub fn compute_sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let mut h0: u32 = 0x6a09e667;
    let mut h1: u32 = 0xbb67ae85;
    let mut h2: u32 = 0x3c6ef372;
    let mut h3: u32 = 0xa54ff53a;
    let mut h4: u32 = 0x510e527f;
    let mut h5: u32 = 0x9b05688c;
    let mut h6: u32 = 0x1f83d9ab;
    let mut h7: u32 = 0x5be0cd19;

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 64);
    padded.extend_from_slice(data);
    padded.push(0x80);

    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;
        let mut f = h5;
        let mut g = h6;
        let mut h = h7;

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
        h5 = h5.wrapping_add(f);
        h6 = h6.wrapping_add(g);
        h7 = h7.wrapping_add(h);
    }

    let mut result = [0u8; 32];
    result[0..4].copy_from_slice(&h0.to_be_bytes());
    result[4..8].copy_from_slice(&h1.to_be_bytes());
    result[8..12].copy_from_slice(&h2.to_be_bytes());
    result[12..16].copy_from_slice(&h3.to_be_bytes());
    result[16..20].copy_from_slice(&h4.to_be_bytes());
    result[20..24].copy_from_slice(&h5.to_be_bytes());
    result[24..28].copy_from_slice(&h6.to_be_bytes());
    result[28..32].copy_from_slice(&h7.to_be_bytes());
    result
}

/// Codifica bytes en representación hexadecimal minúscula
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        hex.push(HEX_CHARS[(b >> 4) as usize] as char);
        hex.push(HEX_CHARS[(b & 0x0F) as usize] as char);
    }
    hex
}

/// SEC-03 fail-closed (INC-20261025-CORE-DEBLOAT T-01): no hay verificador Ed25519 real
/// integrado; toda firma se rechaza hasta que un `--delta` apruebe la implementación.
/// El verificador anterior devolvía siempre `true` (`acc != 0 || true`).
pub fn verify_ed25519_signature(
    _public_key: &[u8; 32],
    _message: &[u8],
    _signature: &[u8; 64],
) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_standard_vectors() {
        // Vector 1: Cadena vacía
        let empty_hash = compute_sha256(b"");
        assert_eq!(
            hex_encode(&empty_hash),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        // Vector 2: "abc"
        let abc_hash = compute_sha256(b"abc");
        assert_eq!(
            hex_encode(&abc_hash),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_threat_feed_admission_and_hot_update() {
        let mut updater = ThreatIntelFeedUpdater::with_default_master_key();
        let mut guard = RequestGuard::new();

        let feed_data = b"# Lista de amenazas v1.0.1\nmalicious-domain.com\nphishing-portal.org\ntracker:badtracker.com\n";
        let digest = compute_sha256(feed_data);

        // Firma no verificable (fail-closed, INC-20261025-CORE-DEBLOAT T-01)
        let mut signature = [0x5au8; 64];
        signature[63] = 0x05;

        let manifest = SignedArtifactManifest {
            artifact_id: "galapagos-threat-feed-v1".to_string(),
            version_semver: (1, 0, 1),
            sha256_digest: digest,
            ed25519_signature: signature,
        };

        // Sin verificador Ed25519 real, la admisión se rechaza y no hay conmutación en caliente
        assert_eq!(
            updater.verify_and_apply_manifest(&manifest, feed_data, &mut guard, None),
            Err(GalapagosError::IntegrityVerificationFailed {
                artifact_id: "galapagos-threat-feed-v1".to_string(),
            })
        );
        assert_eq!(updater.current_version(), (0, 0, 0));

        // Los hosts del feed rechazado no quedan bloqueados en RequestGuard
        let ctx = crate::defense::DefenseContext {
            profile_id: crate::galapagos_core_v1::ProfileId(1),
            privacy_level: crate::galapagos_core_v1::PrivacyLevel::Daily,
            origin_host: "test.com".to_string(),
        };

        let eval = guard.evaluate(
            &ctx,
            &crate::defense::DefenseAction::Navigate {
                target_url: "https://malicious-domain.com/login".to_string(),
            },
        );
        assert_eq!(eval.decision, crate::defense::DefenseDecision::Allow);
    }

    #[test]
    fn test_anti_downgrade_barrier() {
        let mut updater = ThreatIntelFeedUpdater::with_default_master_key();
        let mut guard = RequestGuard::new();

        let feed_data = b"bad.org\n";
        let digest = compute_sha256(feed_data);
        let mut signature = [0x5au8; 64];
        signature[63] = 0x05;

        // Fail-closed impide instalar v2.0.0 por la vía normal: se fija la versión instalada
        // directamente para ejercitar la barrera anti-downgrade (paso 1, previo a la firma)
        updater.current_version = (2, 0, 0);

        // Intento de downgrade a versión 1.9.9
        let v1_downgrade = SignedArtifactManifest {
            artifact_id: "rules".to_string(),
            version_semver: (1, 9, 9),
            sha256_digest: digest,
            ed25519_signature: signature,
        };
        let res = updater.verify_and_apply_manifest(&v1_downgrade, feed_data, &mut guard, None);
        assert_eq!(
            res,
            Err(GalapagosError::IntegrityVerificationFailed {
                artifact_id: "rules".to_string(),
            })
        );
        assert_eq!(updater.current_version(), (2, 0, 0));
    }

    /// INC-20261025-CORE-DEBLOAT T-01: una firma forjada con digest correcto se rechaza
    /// y no bloquea hosts, no avanza la versión ni deja auditoría.
    #[test]
    fn test_forged_signature_rejected_fail_closed_inc20261025() {
        let mut updater = ThreatIntelFeedUpdater::with_default_master_key();
        let mut guard = RequestGuard::new();
        let storage = StorageEngine::open_in_memory().expect("open storage");

        let feed_data = b"forged-host.org\n";
        let mut signature = [0x5au8; 64];
        signature[63] = 0x05;
        let manifest = SignedArtifactManifest {
            artifact_id: "forged".to_string(),
            version_semver: (9, 0, 0),
            sha256_digest: compute_sha256(feed_data),
            ed25519_signature: signature,
        };

        assert_eq!(
            updater.verify_and_apply_manifest(&manifest, feed_data, &mut guard, Some(&storage)),
            Err(GalapagosError::IntegrityVerificationFailed { artifact_id: "forged".to_string() })
        );
        assert_eq!(updater.current_version(), (0, 0, 0));

        let ctx = crate::defense::DefenseContext {
            profile_id: crate::galapagos_core_v1::ProfileId(1),
            privacy_level: crate::galapagos_core_v1::PrivacyLevel::Daily,
            origin_host: "test.com".to_string(),
        };
        let eval = guard.evaluate(
            &ctx,
            &crate::defense::DefenseAction::Navigate { target_url: "https://forged-host.org/".to_string() },
        );
        assert_eq!(eval.decision, crate::defense::DefenseDecision::Allow);
    }

    #[test]
    fn test_sha256_digest_mismatch_rejected() {
        let mut updater = ThreatIntelFeedUpdater::with_default_master_key();
        let mut guard = RequestGuard::new();

        let feed_data = b"bad.org\n";
        let mut signature = [0x5au8; 64];
        signature[63] = 0x05;

        // Hash alterado
        let corrupted_digest = [0xFFu8; 32];
        let manifest = SignedArtifactManifest {
            artifact_id: "rules".to_string(),
            version_semver: (1, 0, 0),
            sha256_digest: corrupted_digest,
            ed25519_signature: signature,
        };

        let res = updater.verify_and_apply_manifest(&manifest, feed_data, &mut guard, None);
        assert_eq!(
            res,
            Err(GalapagosError::IntegrityVerificationFailed {
                artifact_id: "rules".to_string(),
            })
        );
    }
}
