//! src/ipc/mod.rs
//! Canal IPC Bidireccional & Aislamiento AppContainer (Fase F5)
//! Gobernado bajo estándar ZNVE v2.4.0 (Axiomas 1 y 2, ARC-01, ARC-02, SEC-01).

pub mod channel;
pub mod dispatcher;
pub mod framer;

pub use channel::{RestrictedNamedPipeClient, RestrictedNamedPipeServer};
pub use dispatcher::{
    decode_content_ipc, encode_content_ipc, IpcDispatcher, MSG_TYPE_FRAME_READY,
    MSG_TYPE_REQUEST_CAPABILITY, MSG_TYPE_REQUEST_NETWORK_FETCH, MSG_TYPE_TELEMETRY_SAMPLE,
};
pub use framer::{decode_frame, encode_frame, IPC_HEADER_SIZE};
