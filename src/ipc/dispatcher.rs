//! src/ipc/dispatcher.rs
//! Dispatcher y Serializador de Eventos Servo -> Coordinator (Fase F5)
//! Gobernado bajo estándar ZNVE v2.4.0 (Axiomas 1 y 2, ARC-01, ARC-02, SEC-01).
//!
//! Responsabilidades:
//! 1. Procesamiento y deserialización tipada de `ContentToCoordinatorIpc`:
//!    - Tipo 1: `FrameReady`
//!    - Tipo 2: `RequestNetworkFetch`
//!    - Tipo 3: `RequestCapability`
//!    - Tipo 4: `TelemetrySample` (rechazado si llega del contenido, P-04)
//!    - Tipo 6: `UrlChanged` (DELTA_02)
//!    - Tipo 7: `TitleChanged` (DELTA_02)
//!    - Tipo 8: `WorkerFault` (DELTA_05)
//!    - Tipo 9: `NavState` (DELTA_08)
//!    - Tipo 18: `ContentCrashed` (DELTA_11)
//! 2. Desacoplamiento unidireccional y barrera anti-spoofing: Rechazo estricto
//!    de comandos privilegiados (`UiToCoordinatorCmd`) provenientes de procesos de contenido.

#![forbid(unsafe_code)]

use crate::coordinator::Coordinator;
use crate::defense::{DefenseAction, DefenseContext, DefenseDecision};
use crate::galapagos_core_v1::{
    ContentToCoordinatorIpc, GalapagosError, HttpMethod, IpcViolationReason,
    ProcessId, MAX_ALLOWED_TITLE_LEN, MAX_ALLOWED_URL_LEN, SiteCapability, TabId, UiToCoordinatorCmd, WindowsSandboxLevel, WorkerHandshake,
};
use crate::ipc::framer::{decode_frame, encode_frame};
use std::num::NonZeroU32;

// R-14: los tipos de mensaje se definen una sola vez en `galapagos-ipc`
pub const MSG_TYPE_FRAME_READY: u16 = galapagos_ipc::MSG_FRAME_READY;
pub const MSG_TYPE_REQUEST_NETWORK_FETCH: u16 = galapagos_ipc::MSG_REQUEST_NETWORK_FETCH;
pub const MSG_TYPE_REQUEST_CAPABILITY: u16 = galapagos_ipc::MSG_REQUEST_CAPABILITY;
pub const MSG_TYPE_TELEMETRY_SAMPLE: u16 = galapagos_ipc::MSG_TELEMETRY_SAMPLE;
pub const MSG_TYPE_HANDSHAKE: u16 = galapagos_ipc::MSG_HANDSHAKE;
pub const MSG_TYPE_URL_CHANGED: u16 = galapagos_ipc::MSG_URL_CHANGED;
pub const MSG_TYPE_TITLE_CHANGED: u16 = galapagos_ipc::MSG_TITLE_CHANGED;
pub const MSG_TYPE_WORKER_FAULT: u16 = galapagos_ipc::MSG_WORKER_FAULT;
pub const MSG_TYPE_NAV_STATE: u16 = galapagos_ipc::MSG_NAV_STATE;
pub const MSG_TYPE_CONTENT_CRASHED: u16 = galapagos_ipc::MSG_CONTENT_CRASHED;

fn malformed(tab_id: TabId, reason: IpcViolationReason) -> GalapagosError {
    GalapagosError::IpcBoundaryViolation { tab_id, reason }
}

/// Codifica `tab_id u64 · len u16 · texto` (DELTA_02, tipos 6 y 7)
fn encode_tab_text(
    msg_type: u16,
    tab_id: TabId,
    text: &str,
    max_len: usize,
) -> Result<Vec<u8>, GalapagosError> {
    if text.len() > max_len {
        return Err(malformed(tab_id, IpcViolationReason::PayloadTooLarge));
    }
    galapagos_ipc::encode_tab_text(msg_type, tab_id.0, text)
        .map_err(|_| malformed(tab_id, IpcViolationReason::PayloadTooLarge))
}

/// Decodifica `tab_id u64 · len u16 · texto`. El contenido es hostil (ARC-02): longitud exacta,
/// tope por tipo, UTF-8 válido y sin caracteres de control ASCII.
fn decode_tab_text(payload: &[u8], max_len: usize) -> Result<(TabId, String), GalapagosError> {
    if payload.len() < 10 {
        return Err(malformed(TabId(0), IpcViolationReason::MalformedMessage));
    }
    let tab_id = TabId(u64::from_be_bytes(payload[0..8].try_into().unwrap()));
    let len = u16::from_be_bytes(payload[8..10].try_into().unwrap()) as usize;
    if len > max_len {
        return Err(malformed(tab_id, IpcViolationReason::PayloadTooLarge));
    }
    if payload.len() != 10 + len {
        return Err(malformed(tab_id, IpcViolationReason::MalformedMessage));
    }
    let text = std::str::from_utf8(&payload[10..])
        .map_err(|_| malformed(tab_id, IpcViolationReason::MalformedMessage))?;
    if text.bytes().any(|b| b < 0x20 || b == 0x7F) {
        return Err(malformed(tab_id, IpcViolationReason::ControlCharacterDetected));
    }
    Ok((tab_id, text.to_string()))
}

/// Codifica un `WorkerHandshake` en una trama binaria enmarcada (MSG_TYPE_HANDSHAKE = 5)
pub fn encode_worker_handshake(handshake: &WorkerHandshake) -> Result<Vec<u8>, GalapagosError> {
    let sandbox = match handshake.applied_sandbox {
        WindowsSandboxLevel::TrustedCoordinator => 1,
        WindowsSandboxLevel::AppContainerStrict => 2,
        WindowsSandboxLevel::RestrictedService => 3,
    };
    Ok(galapagos_ipc::encode_handshake(
        handshake.process_id.0.get(),
        sandbox,
        handshake.protocol_version.get(),
    ))
}

/// Decodifica el payload de un mensaje `WorkerHandshake`
pub fn decode_worker_handshake(payload: &[u8]) -> Result<WorkerHandshake, GalapagosError> {
    if payload.len() < 7 {
        return Err(GalapagosError::IpcBoundaryViolation {
            tab_id: TabId(0),
            reason: IpcViolationReason::MalformedMessage,
        });
    }
    let pid_raw = u32::from_be_bytes(payload[0..4].try_into().unwrap());
    let process_id = ProcessId(NonZeroU32::new(pid_raw).ok_or_else(|| {
        GalapagosError::IpcBoundaryViolation {
            tab_id: TabId(0),
            reason: IpcViolationReason::MalformedMessage,
        }
    })?);
    let applied_sandbox = match payload[4] {
        1 => WindowsSandboxLevel::TrustedCoordinator,
        2 => WindowsSandboxLevel::AppContainerStrict,
        3 => WindowsSandboxLevel::RestrictedService,
        _ => {
            return Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::MalformedMessage,
            })
        }
    };
    let version_raw = u16::from_be_bytes(payload[5..7].try_into().unwrap());
    let protocol_version = std::num::NonZeroU16::new(version_raw).ok_or_else(|| {
        GalapagosError::IpcBoundaryViolation {
            tab_id: TabId(0),
            reason: IpcViolationReason::MalformedMessage,
        }
    })?;
    Ok(WorkerHandshake {
        process_id,
        applied_sandbox,
        protocol_version,
    })
}

/// Codifica un evento `ContentToCoordinatorIpc` en una trama binaria enmarcada
pub fn encode_content_ipc(msg: &ContentToCoordinatorIpc) -> Result<Vec<u8>, GalapagosError> {
    match msg {
        ContentToCoordinatorIpc::FrameReady { tab_id, frame_seq } => {
            let mut payload = Vec::with_capacity(16);
            payload.extend_from_slice(&tab_id.0.to_be_bytes());
            payload.extend_from_slice(&frame_seq.to_be_bytes());
            encode_frame(MSG_TYPE_FRAME_READY, &payload)
        }
        ContentToCoordinatorIpc::RequestNetworkFetch {
            tab_id,
            request_id,
            url,
            method,
            is_third_party,
        } => {
            let url_bytes = url.as_bytes();
            let mut payload = Vec::with_capacity(8 + 8 + 1 + 1 + 2 + url_bytes.len());
            payload.extend_from_slice(&tab_id.0.to_be_bytes());
            payload.extend_from_slice(&request_id.to_be_bytes());
            payload.push(match method {
                HttpMethod::Get => 1,
                HttpMethod::Post => 2,
                HttpMethod::Head => 3,
                HttpMethod::Options => 4,
                HttpMethod::Put => 5,
                HttpMethod::Delete => 6,
            });
            payload.push(if *is_third_party { 1 } else { 0 });
            payload.extend_from_slice(&(url_bytes.len() as u16).to_be_bytes());
            payload.extend_from_slice(url_bytes);
            encode_frame(MSG_TYPE_REQUEST_NETWORK_FETCH, &payload)
        }
        ContentToCoordinatorIpc::RequestCapability {
            tab_id,
            origin,
            capability,
        } => {
            let origin_bytes = origin.as_bytes();
            let mut payload = Vec::with_capacity(8 + 1 + 2 + origin_bytes.len());
            payload.extend_from_slice(&tab_id.0.to_be_bytes());
            payload.push(match capability {
                SiteCapability::ClipboardRead => 1,
                SiteCapability::Notifications => 2,
                SiteCapability::MediaCapture => 3,
                SiteCapability::WebRtcPeerConnection => 4,
                SiteCapability::LocalStorage => 5,
            });
            payload.extend_from_slice(&(origin_bytes.len() as u16).to_be_bytes());
            payload.extend_from_slice(origin_bytes);
            encode_frame(MSG_TYPE_REQUEST_CAPABILITY, &payload)
        }
        ContentToCoordinatorIpc::TelemetrySample(sample) => {
            let mut payload = Vec::with_capacity(48);
            payload.extend_from_slice(&sample.tab_id.0.to_be_bytes());
            payload.extend_from_slice(&sample.process_id.0.get().to_be_bytes());
            payload.extend_from_slice(&sample.timestamp_monotonic_ns.to_be_bytes());
            payload.extend_from_slice(&sample.private_working_set_bytes.to_be_bytes());
            payload.extend_from_slice(&sample.cpu_time_delta_us.to_be_bytes());
            payload.extend_from_slice(&sample.navigation_latency_ms.to_be_bytes());
            payload.extend_from_slice(&sample.security_filter_overhead_us.to_be_bytes());
            payload.extend_from_slice(&sample.blocked_trackers_count.to_be_bytes());
            encode_frame(MSG_TYPE_TELEMETRY_SAMPLE, &payload)
        }
        ContentToCoordinatorIpc::UrlChanged { tab_id, url } => {
            encode_tab_text(MSG_TYPE_URL_CHANGED, *tab_id, url, MAX_ALLOWED_URL_LEN)
        }
        ContentToCoordinatorIpc::TitleChanged { tab_id, title } => {
            encode_tab_text(MSG_TYPE_TITLE_CHANGED, *tab_id, title, MAX_ALLOWED_TITLE_LEN)
        }
        ContentToCoordinatorIpc::WorkerFault { tab_id, message } => {
            encode_tab_text(MSG_TYPE_WORKER_FAULT, *tab_id, message, MAX_ALLOWED_TITLE_LEN)
        }
        ContentToCoordinatorIpc::NavState { tab_id, can_back, can_forward, loading } => {
            let state = galapagos_ipc::NavState { can_back: *can_back, can_forward: *can_forward, loading: *loading };
            Ok(galapagos_ipc::encode_nav_state(tab_id.0, state))
        }
        ContentToCoordinatorIpc::ContentCrashed { tab_id, reason } => {
            encode_tab_text(MSG_TYPE_CONTENT_CRASHED, *tab_id, reason, MAX_ALLOWED_TITLE_LEN)
        }
    }
}

/// Decodifica una trama binaria en un evento tipado `ContentToCoordinatorIpc`
pub fn decode_content_ipc(
    msg_type: u16,
    payload: &[u8],
) -> Result<ContentToCoordinatorIpc, GalapagosError> {
    match msg_type {
        MSG_TYPE_FRAME_READY => {
            if payload.len() < 16 {
                return Err(GalapagosError::IpcBoundaryViolation {
                    tab_id: TabId(0),
                    reason: IpcViolationReason::MalformedMessage,
                });
            }
            let tab_id = TabId(u64::from_be_bytes(payload[0..8].try_into().unwrap()));
            let frame_seq = u64::from_be_bytes(payload[8..16].try_into().unwrap());
            Ok(ContentToCoordinatorIpc::FrameReady { tab_id, frame_seq })
        }
        MSG_TYPE_REQUEST_NETWORK_FETCH => {
            if payload.len() < 20 {
                return Err(GalapagosError::IpcBoundaryViolation {
                    tab_id: TabId(0),
                    reason: IpcViolationReason::MalformedMessage,
                });
            }
            let tab_id = TabId(u64::from_be_bytes(payload[0..8].try_into().unwrap()));
            let request_id = u64::from_be_bytes(payload[8..16].try_into().unwrap());
            let method = match payload[16] {
                1 => HttpMethod::Get,
                2 => HttpMethod::Post,
                3 => HttpMethod::Head,
                4 => HttpMethod::Options,
                5 => HttpMethod::Put,
                6 => HttpMethod::Delete,
                _ => {
                    return Err(GalapagosError::IpcBoundaryViolation {
                        tab_id,
                        reason: IpcViolationReason::MalformedMessage,
                    })
                }
            };
            let is_third_party = payload[17] != 0;
            let url_len = u16::from_be_bytes(payload[18..20].try_into().unwrap()) as usize;
            if payload.len() < 20 + url_len {
                return Err(GalapagosError::IpcBoundaryViolation {
                    tab_id,
                    reason: IpcViolationReason::MalformedMessage,
                });
            }
            let url_str = std::str::from_utf8(&payload[20..20 + url_len]).map_err(|_| {
                GalapagosError::IpcBoundaryViolation {
                    tab_id,
                    reason: IpcViolationReason::MalformedMessage,
                }
            })?;

            // Validar esquema de URL
            UiToCoordinatorCmd::validate_url_scheme(url_str).map_err(|reason| {
                GalapagosError::IpcBoundaryViolation { tab_id, reason }
            })?;

            Ok(ContentToCoordinatorIpc::RequestNetworkFetch {
                tab_id,
                request_id,
                url: url_str.to_string(),
                method,
                is_third_party,
            })
        }
        MSG_TYPE_REQUEST_CAPABILITY => {
            if payload.len() < 11 {
                return Err(GalapagosError::IpcBoundaryViolation {
                    tab_id: TabId(0),
                    reason: IpcViolationReason::MalformedMessage,
                });
            }
            let tab_id = TabId(u64::from_be_bytes(payload[0..8].try_into().unwrap()));
            let capability = match payload[8] {
                1 => SiteCapability::ClipboardRead,
                2 => SiteCapability::Notifications,
                3 => SiteCapability::MediaCapture,
                4 => SiteCapability::WebRtcPeerConnection,
                5 => SiteCapability::LocalStorage,
                _ => {
                    return Err(GalapagosError::IpcBoundaryViolation {
                        tab_id,
                        reason: IpcViolationReason::MalformedMessage,
                    })
                }
            };
            let origin_len = u16::from_be_bytes(payload[9..11].try_into().unwrap()) as usize;
            if payload.len() < 11 + origin_len {
                return Err(GalapagosError::IpcBoundaryViolation {
                    tab_id,
                    reason: IpcViolationReason::MalformedMessage,
                });
            }
            let origin_str =
                std::str::from_utf8(&payload[11..11 + origin_len]).map_err(|_| {
                    GalapagosError::IpcBoundaryViolation {
                        tab_id,
                        reason: IpcViolationReason::MalformedMessage,
                    }
                })?;

            Ok(ContentToCoordinatorIpc::RequestCapability {
                tab_id,
                origin: origin_str.to_string(),
                capability,
            })
        }
        // P-04 (INC-20261025-CORE-DEBLOAT): el worker no emite telemetría; las muestras PERF-01
        // las toma el Coordinador midiendo el proceso. Una muestra del contenido es hostil (antes
        // se insertaba sin tope en SQLite): se rechaza y el llamador cierra el canal.
        MSG_TYPE_TELEMETRY_SAMPLE => Err(GalapagosError::IpcBoundaryViolation {
            tab_id: TabId(0),
            reason: IpcViolationReason::UnauthorizedCommand,
        }),
        MSG_TYPE_URL_CHANGED => {
            let (tab_id, url) = decode_tab_text(payload, MAX_ALLOWED_URL_LEN)?;
            Ok(ContentToCoordinatorIpc::UrlChanged { tab_id, url })
        }
        MSG_TYPE_TITLE_CHANGED => {
            let (tab_id, title) = decode_tab_text(payload, MAX_ALLOWED_TITLE_LEN)?;
            Ok(ContentToCoordinatorIpc::TitleChanged { tab_id, title })
        }
        MSG_TYPE_WORKER_FAULT => {
            let (tab_id, message) = decode_tab_text(payload, MAX_ALLOWED_TITLE_LEN)?;
            Ok(ContentToCoordinatorIpc::WorkerFault { tab_id, message })
        }
        MSG_TYPE_CONTENT_CRASHED => {
            let (tab_id, reason) = decode_tab_text(payload, MAX_ALLOWED_TITLE_LEN)?;
            Ok(ContentToCoordinatorIpc::ContentCrashed { tab_id, reason })
        }
        MSG_TYPE_NAV_STATE => {
            let (tab_id, s) = galapagos_ipc::decode_nav_state(payload).ok_or(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::MalformedMessage,
            })?;
            Ok(ContentToCoordinatorIpc::NavState {
                tab_id: TabId(tab_id),
                can_back: s.can_back,
                can_forward: s.can_forward,
                loading: s.loading,
            })
        }
        _ => Err(GalapagosError::IpcBoundaryViolation {
            tab_id: TabId(0),
            reason: IpcViolationReason::UnauthorizedCommand,
        }),
    }
}

/// Despachador de Mensajes IPC de Contenido hacia el Trusted Coordinator
pub struct IpcDispatcher;

impl IpcDispatcher {
    /// Procesa una trama de bytes crudos y despacha el evento al coordinador
    pub fn dispatch_raw_frame(
        coordinator: &mut Coordinator,
        raw_frame: &[u8],
    ) -> Result<ContentToCoordinatorIpc, GalapagosError> {
        let (msg_type, payload, _) = decode_frame(raw_frame)?
            .ok_or_else(|| GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::MalformedMessage,
            })?;

        let event = decode_content_ipc(msg_type, &payload)?;
        Self::dispatch_event(coordinator, &event)?;
        Ok(event)
    }

    /// Despacha un evento tipado `ContentToCoordinatorIpc` validando políticas
    pub fn dispatch_event(
        coordinator: &mut Coordinator,
        event: &ContentToCoordinatorIpc,
    ) -> Result<(), GalapagosError> {
        match event {
            ContentToCoordinatorIpc::FrameReady { tab_id, frame_seq } => {
                // Registrar secuencia de renderizado
                let _ = (tab_id, frame_seq);
                Ok(())
            }
            ContentToCoordinatorIpc::RequestNetworkFetch {
                tab_id: _,
                request_id: _,
                url,
                method: _,
                is_third_party,
            } => {
                // Evaluar la petición de red con Ocean Defense
                let ctx = DefenseContext {
                    profile_id: crate::galapagos_core_v1::ProfileId(1),
                    privacy_level: crate::galapagos_core_v1::PrivacyLevel::Daily,
                    origin_host: "unknown".to_string(),
                };
                let action = DefenseAction::SubresourceFetch {
                    url: url.clone(),
                    is_third_party: *is_third_party,
                };
                let eval = coordinator.defense().evaluate(&ctx, &action);
                match eval.decision {
                    DefenseDecision::Allow | DefenseDecision::Restrict => Ok(()),
                    DefenseDecision::Block => Err(GalapagosError::PermissionDenied {
                        profile_id: ctx.profile_id,
                        origin: url.clone(),
                        capability: SiteCapability::LocalStorage,
                    }),
                    DefenseDecision::RequireConfirmation => Err(GalapagosError::PermissionDenied {
                        profile_id: ctx.profile_id,
                        origin: url.clone(),
                        capability: SiteCapability::LocalStorage,
                    }),
                    DefenseDecision::Unavailable => {
                        Err(GalapagosError::IntegrityVerificationFailed {
                            artifact_id: url.clone(),
                        })
                    }
                }
            }
            ContentToCoordinatorIpc::RequestCapability {
                tab_id,
                origin,
                capability,
            } => {
                // Validar acceso con Freedom Island o Storage
                let _ = (tab_id, origin, capability);
                Ok(())
            }
            // P-04: la telemetría del contenido no se acepta (ver `decode_content_ipc`)
            ContentToCoordinatorIpc::TelemetrySample(sample) => Err(GalapagosError::IpcBoundaryViolation {
                tab_id: sample.tab_id,
                reason: IpcViolationReason::UnauthorizedCommand,
            }),
            // DELTA_02: la UI aplica estos eventos desde el bucle principal (paso 7);
            // el dispatcher solo los entrega ya validados por el codec.
            ContentToCoordinatorIpc::UrlChanged { .. }
            | ContentToCoordinatorIpc::TitleChanged { .. }
            | ContentToCoordinatorIpc::WorkerFault { .. }
            | ContentToCoordinatorIpc::NavState { .. }
            | ContentToCoordinatorIpc::ContentCrashed { .. } => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::galapagos_core_v1::PerformanceSampleDto;

    #[test]
    fn test_ipc_event_encode_decode_roundtrip() {
        let event = ContentToCoordinatorIpc::RequestNetworkFetch {
            tab_id: TabId(10),
            request_id: 1001,
            url: "https://servo.org/engine".to_string(),
            method: HttpMethod::Get,
            is_third_party: false,
        };

        let frame = encode_content_ipc(&event).expect("Codificación exitosa");
        let (msg_type, payload, _) = decode_frame(&frame).unwrap().unwrap();
        let decoded = decode_content_ipc(msg_type, &payload).expect("Decodificación exitosa");

        assert_eq!(decoded, event);
    }

    fn violation(tab: u64, reason: IpcViolationReason) -> Result<ContentToCoordinatorIpc, GalapagosError> {
        Err(GalapagosError::IpcBoundaryViolation { tab_id: TabId(tab), reason })
    }

    fn text_payload(tab: u64, declared_len: u16, body: &[u8]) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&tab.to_be_bytes());
        p.extend_from_slice(&declared_len.to_be_bytes());
        p.extend_from_slice(body);
        p
    }

    #[test]
    fn test_url_and_title_changed_roundtrip_delta02() {
        for event in [
            ContentToCoordinatorIpc::UrlChanged {
                tab_id: TabId(1),
                url: "https://servo.org/ñ".to_string(),
            },
            ContentToCoordinatorIpc::TitleChanged {
                tab_id: TabId(1),
                title: "Servo — título".to_string(),
            },
        ] {
            let frame = encode_content_ipc(&event).unwrap();
            let (msg_type, payload, _) = decode_frame(&frame).unwrap().unwrap();
            assert_eq!(decode_content_ipc(msg_type, &payload), Ok(event));
        }
    }

    #[test]
    fn test_url_and_title_changed_hostile_payloads_rejected_delta02() {
        // Truncado (< 10 bytes)
        assert_eq!(
            decode_content_ipc(MSG_TYPE_URL_CHANGED, &[0; 9]),
            violation(0, IpcViolationReason::MalformedMessage)
        );
        // Longitud declarada mayor que el cuerpo
        assert_eq!(
            decode_content_ipc(MSG_TYPE_URL_CHANGED, &text_payload(3, 5, b"abc")),
            violation(3, IpcViolationReason::MalformedMessage)
        );
        // Bytes sobrantes tras la longitud declarada
        assert_eq!(
            decode_content_ipc(MSG_TYPE_TITLE_CHANGED, &text_payload(3, 1, b"abc")),
            violation(3, IpcViolationReason::MalformedMessage)
        );
        // UTF-8 inválido
        assert_eq!(
            decode_content_ipc(MSG_TYPE_TITLE_CHANGED, &text_payload(3, 2, &[0xFF, 0xFE])),
            violation(3, IpcViolationReason::MalformedMessage)
        );
        // Carácter de control
        assert_eq!(
            decode_content_ipc(MSG_TYPE_URL_CHANGED, &text_payload(3, 3, b"a\nb")),
            violation(3, IpcViolationReason::ControlCharacterDetected)
        );
        // Tope por tipo: título 513 B rechazado; URL de 513 B admitida
        let long = vec![b'a'; MAX_ALLOWED_TITLE_LEN + 1];
        assert_eq!(
            decode_content_ipc(MSG_TYPE_TITLE_CHANGED, &text_payload(3, long.len() as u16, &long)),
            violation(3, IpcViolationReason::PayloadTooLarge)
        );
        assert!(decode_content_ipc(MSG_TYPE_URL_CHANGED, &text_payload(3, long.len() as u16, &long)).is_ok());
        // Codificación respeta los mismos topes
        assert_eq!(
            encode_content_ipc(&ContentToCoordinatorIpc::TitleChanged {
                tab_id: TabId(3),
                title: String::from_utf8(long).unwrap(),
            }),
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(3),
                reason: IpcViolationReason::PayloadTooLarge,
            })
        );
    }

    #[test]
    fn test_worker_fault_codec_delta05() {
        let event = ContentToCoordinatorIpc::WorkerFault {
            tab_id: TabId(1),
            message: "panic en renderer.rs:60: GL_VERSION".to_string(),
        };
        let frame = encode_content_ipc(&event).unwrap();
        let (msg_type, payload, _) = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(msg_type, MSG_TYPE_WORKER_FAULT);
        assert_eq!(decode_content_ipc(msg_type, &payload), Ok(event));

        // Hostil: control, tope de 512 B y longitud inconsistente
        assert_eq!(
            decode_content_ipc(MSG_TYPE_WORKER_FAULT, &text_payload(1, 2, b"a ")),
            violation(1, IpcViolationReason::ControlCharacterDetected)
        );
        let long = vec![b'x'; MAX_ALLOWED_TITLE_LEN + 1];
        assert_eq!(
            decode_content_ipc(MSG_TYPE_WORKER_FAULT, &text_payload(1, long.len() as u16, &long)),
            violation(1, IpcViolationReason::PayloadTooLarge)
        );
        assert_eq!(
            decode_content_ipc(MSG_TYPE_WORKER_FAULT, &text_payload(1, 9, b"abc")),
            violation(1, IpcViolationReason::MalformedMessage)
        );
    }

    #[test]
    fn test_content_crashed_codec_delta11() {
        let event = ContentToCoordinatorIpc::ContentCrashed {
            tab_id: TabId(1),
            reason: "contenido falló: panic en script".to_string(),
        };
        let frame = encode_content_ipc(&event).unwrap();
        let (msg_type, payload, _) = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(msg_type, MSG_TYPE_CONTENT_CRASHED);
        assert_eq!(decode_content_ipc(msg_type, &payload), Ok(event));
        // Hostil: mismas defensas que los tipos 6–8
        assert_eq!(
            decode_content_ipc(MSG_TYPE_CONTENT_CRASHED, &text_payload(1, 2, b"a\x07")),
            violation(1, IpcViolationReason::ControlCharacterDetected)
        );
        let long = vec![b'x'; MAX_ALLOWED_TITLE_LEN + 1];
        assert_eq!(
            decode_content_ipc(MSG_TYPE_CONTENT_CRASHED, &text_payload(1, long.len() as u16, &long)),
            violation(1, IpcViolationReason::PayloadTooLarge)
        );
        assert_eq!(
            decode_content_ipc(MSG_TYPE_CONTENT_CRASHED, &text_payload(1, 9, b"abc")),
            violation(1, IpcViolationReason::MalformedMessage)
        );
    }

    #[test]
    fn test_nav_state_codec_delta08() {
        let event = ContentToCoordinatorIpc::NavState { tab_id: TabId(2), can_back: true, can_forward: false, loading: true };
        let frame = encode_content_ipc(&event).unwrap();
        let (msg_type, payload, _) = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(msg_type, MSG_TYPE_NAV_STATE);
        assert_eq!(decode_content_ipc(msg_type, &payload), Ok(event));
        // Hostil: longitud distinta o byte que no es 0/1
        assert_eq!(decode_content_ipc(MSG_TYPE_NAV_STATE, &payload[..10]), violation(0, IpcViolationReason::MalformedMessage));
        let mut bad = payload.clone();
        bad[10] = 7;
        assert_eq!(decode_content_ipc(MSG_TYPE_NAV_STATE, &bad), violation(0, IpcViolationReason::MalformedMessage));
    }

    /// INC-20261025-CORE-DEBLOAT P-04: una muestra de telemetría bien formada que llega del
    /// contenido se rechaza (antes se decodificaba y se insertaba sin tope en SQLite)
    #[test]
    fn test_content_telemetry_sample_is_unauthorized_inc20261025() {
        let sample = PerformanceSampleDto {
            tab_id: TabId(1),
            process_id: ProcessId(NonZeroU32::new(42).unwrap()),
            timestamp_monotonic_ns: 1,
            private_working_set_bytes: 2,
            cpu_time_delta_us: 3,
            navigation_latency_ms: 4,
            security_filter_overhead_us: 5,
            blocked_trackers_count: 6,
        };
        let frame = encode_content_ipc(&ContentToCoordinatorIpc::TelemetrySample(sample)).unwrap();
        let (msg_type, payload, _) = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(
            decode_content_ipc(msg_type, &payload),
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::UnauthorizedCommand,
            })
        );

        // Defensa en profundidad: el despacho tampoco la persiste
        let mut coordinator = Coordinator::new();
        coordinator.attach_storage(crate::storage::StorageEngine::open_in_memory().unwrap());
        assert!(IpcDispatcher::dispatch_event(&mut coordinator, &ContentToCoordinatorIpc::TelemetrySample(sample)).is_err());
        assert!(coordinator.storage().unwrap().query_perf_baseline(TabId(1), 10).unwrap().is_empty());
    }

    #[test]
    fn test_spoofed_unauthorized_command_rejected() {
        // Enviar tipo de mensaje no reconocido o comando fuera del contrato
        let res = decode_content_ipc(999, b"unauthorized privileged command");
        assert_eq!(
            res,
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::UnauthorizedCommand,
            })
        );
    }

    #[test]
    fn test_worker_handshake_codec_roundtrip() {
        let handshake = WorkerHandshake {
            process_id: ProcessId(NonZeroU32::new(9988).unwrap()),
            applied_sandbox: WindowsSandboxLevel::AppContainerStrict,
            protocol_version: std::num::NonZeroU16::new(1).unwrap(),
        };

        let frame = encode_worker_handshake(&handshake).expect("Codificación exitosa");
        let (msg_type, payload, _) = decode_frame(&frame).unwrap().unwrap();
        assert_eq!(msg_type, MSG_TYPE_HANDSHAKE);

        let decoded = decode_worker_handshake(&payload).expect("Decodificación exitosa");
        assert_eq!(decoded, handshake);

        // H4: Handshake recibido por decode_content_ipc debe ser rechazado como UnauthorizedCommand
        let content_res = decode_content_ipc(msg_type, &payload);
        assert_eq!(
            content_res,
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::UnauthorizedCommand,
            })
        );
    }

    #[test]
    fn test_worker_handshake_malformed_rejected() {
        // Payload truncado (< 7 bytes)
        assert_eq!(
            decode_worker_handshake(&[0; 6]),
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::MalformedMessage,
            })
        );

        // PID cero
        let mut zero_pid = vec![0; 7];
        zero_pid[4] = 2; // AppContainerStrict
        zero_pid[6] = 1; // v1
        assert_eq!(
            decode_worker_handshake(&zero_pid),
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::MalformedMessage,
            })
        );

        // Sandbox inválido (e.g. 99)
        let invalid_sandbox = vec![0, 0, 0, 1, 99, 0, 1];
        assert_eq!(
            decode_worker_handshake(&invalid_sandbox),
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::MalformedMessage,
            })
        );

        // Versión cero
        let zero_version = vec![0, 0, 0, 1, 2, 0, 0];
        assert_eq!(
            decode_worker_handshake(&zero_version),
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::MalformedMessage,
            })
        );
    }
}
