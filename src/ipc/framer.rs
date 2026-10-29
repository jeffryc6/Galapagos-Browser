//! src/ipc/framer.rs
//! Enmarcador y Serializador Determinista sin Sobrecarga para IPC (Fase F5)
//! Gobernado bajo estándar ZNVE v2.4.0 (Axiomas 1 y 2, ARC-01, ARC-02, SEC-01).
//!
//! Formato binario inmutable:
//! [ Longitud: 4 bytes (u32 BE) ][ Tipo de mensaje: 2 bytes (u16 BE) ][ Carga útil: N bytes ]
//! Límite estricto: N <= 1_048_576 bytes (1 MiB, conforme a MAX_ALLOWED_IPC_PAYLOAD_BYTES).

#![forbid(unsafe_code)]

use crate::galapagos_core_v1::{
    GalapagosError, IpcViolationReason, TabId, MAX_ALLOWED_IPC_PAYLOAD_BYTES,
};

/// Encabezado inmutable de trama IPC (6 bytes fijos). R-14: definido en `galapagos-ipc`.
pub const IPC_HEADER_SIZE: usize = galapagos_ipc::HEADER_LEN;

// R-14: el formato compartido y el contrato no pueden divergir (falla la compilación)
const _: () = assert!(galapagos_ipc::MAX_PAYLOAD_BYTES == MAX_ALLOWED_IPC_PAYLOAD_BYTES as usize);
const _: () = assert!(galapagos_ipc::MAX_URL_LEN == crate::galapagos_core_v1::MAX_ALLOWED_URL_LEN);
const _: () = assert!(galapagos_ipc::MAX_TITLE_LEN == crate::galapagos_core_v1::MAX_ALLOWED_TITLE_LEN);

fn too_large() -> GalapagosError {
    GalapagosError::IpcBoundaryViolation {
        tab_id: TabId(0),
        reason: IpcViolationReason::PayloadTooLarge,
    }
}

/// Encodifica una carga útil en una trama binaria enmarcada (tope 1 MiB, ARC-02 / SEC-01)
pub fn encode_frame(msg_type: u16, payload: &[u8]) -> Result<Vec<u8>, GalapagosError> {
    galapagos_ipc::encode_frame(msg_type, payload).map_err(|_| too_large())
}

/// Decodifica una trama binaria desde un búfer de bytes recibidos
/// Retorna `(tipo_mensaje, carga_util, bytes_consumidos)` si la trama está completa
pub fn decode_frame(buffer: &[u8]) -> Result<Option<(u16, Vec<u8>, usize)>, GalapagosError> {
    let Some((msg_type, payload_len)) = galapagos_ipc::parse_header(buffer) else {
        // Trama incompleta (aún faltan bytes del encabezado)
        return Ok(None);
    };

    // Verificación de frontera: prohibir tramas gigantes que violen el presupuesto
    if payload_len > galapagos_ipc::MAX_PAYLOAD_BYTES {
        return Err(too_large());
    }

    let total_frame_len = IPC_HEADER_SIZE + payload_len;
    if buffer.len() < total_frame_len {
        // Trama incompleta (esperando el resto de la carga útil)
        return Ok(None);
    }

    let payload = buffer[IPC_HEADER_SIZE..total_frame_len].to_vec();
    Ok(Some((msg_type, payload, total_frame_len)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_encode_decode_roundtrip() {
        let payload = b"Hello from sandboxed Servo worker!";
        let msg_type = 42u16;

        let frame = encode_frame(msg_type, payload).expect("Codificación exitosa");
        assert_eq!(frame.len(), IPC_HEADER_SIZE + payload.len());

        let decoded = decode_frame(&frame)
            .expect("Decodificación sin errores")
            .expect("Trama completa");

        assert_eq!(decoded.0, msg_type);
        assert_eq!(&decoded.1, payload);
        assert_eq!(decoded.2, frame.len());
    }

    #[test]
    fn test_payload_exceeding_1mib_is_rejected() {
        // Carga de 1 MiB + 1 byte
        let oversized = vec![0u8; (MAX_ALLOWED_IPC_PAYLOAD_BYTES + 1) as usize];
        let res = encode_frame(1, &oversized);

        assert_eq!(
            res,
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id: TabId(0),
                reason: IpcViolationReason::PayloadTooLarge,
            })
        );
    }

    #[test]
    fn test_incomplete_frame_returns_none() {
        let partial_header = [0, 0, 0, 10]; // solo 4 bytes, faltan 2 del tipo
        let res = decode_frame(&partial_header).unwrap();
        assert_eq!(res, None);

        let complete_header_partial_body = [0, 0, 0, 10, 0, 1, 65, 66]; // cuerpo dice 10 pero sólo hay 2
        let res2 = decode_frame(&complete_header_partial_body).unwrap();
        assert_eq!(res2, None);
    }
}
