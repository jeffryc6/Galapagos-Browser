//! src/ipc/channel.rs
//! Canal de Comunicación IPC sobre Win32 Named Pipes con Aislamiento Restringido (Fase F5)
//! Gobernado bajo estándar ZNVE v2.4.0 (Axiomas 1 y 2, ARC-01, ARC-02, SEC-01).
//!
//! Responsabilidades:
//! 1. Transporte síncrono de alto rendimiento mediante `CreateNamedPipeW` y `ConnectNamedPipe`.
//! 2. Descriptor de seguridad explícito (SYSTEM + propietario), instancia única, sin clientes
//!    remotos y nombre no predecible (INC-20261025-CORE-DEBLOAT T-03).
//! 3. Enmarcado binario inmutable: longitud (4 bytes) + tipo (2 bytes) + payload (<= 1 MiB).
//! 4. Timeout estricto de 5 000 ms conforme a `MAX_ALLOWED_IPC_TIMEOUT_MS`.

use crate::galapagos_core_v1::{
    GalapagosError, IpcViolationReason, TabId, WindowsSandboxLevel, MAX_ALLOWED_IPC_PAYLOAD_BYTES,
    MAX_ALLOWED_IPC_TIMEOUT_MS,
};
use crate::ipc::framer::{decode_frame, encode_frame, IPC_HEADER_SIZE};

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_MORE_DATA, ERROR_PIPE_CONNECTED, GENERIC_READ,
    GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId, PeekNamedPipe,
    PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PIPE_WAIT,
};

/// T-03: DACL protegida solo para SYSTEM y el propietario del pipe (el usuario del Coordinador).
/// Sustituye a la DACL nula anterior, que daba acceso total a cualquier proceso local.
const PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;OW)";
const SDDL_REVISION_1: u32 = 1;
const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;

/// P-01: la mayoría de tramas (FrameReady, handshake, entrada) cabe aquí sin reservar memoria;
/// antes cada lectura reservaba y ponía a cero 64 KiB.
const SMALL_FRAME_BYTES: usize = 256;

// `Win32_Security_Authorization` y bcrypt no están habilitados en windows-sys: FFI mínima a mano
#[link(name = "advapi32")]
extern "system" {
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        sddl: *const u16,
        revision: u32,
        security_descriptor: *mut PSECURITY_DESCRIPTOR,
        size: *mut u32,
    ) -> i32;
}

#[link(name = "bcrypt")]
extern "system" {
    fn BCryptGenRandom(algorithm: *mut std::ffi::c_void, buffer: *mut u8, len: u32, flags: u32) -> i32;
}

/// DELTA_04 L-I3: `ERROR_BROKEN_PIPE`(109), `ERROR_NO_DATA`(232) y `ERROR_PIPE_NOT_CONNECTED`(233)
/// son un canal caído (`ChannelTimeout`); el resto, un mensaje inválido (`MalformedMessage`).
fn classify_pipe_error(win32_code: u32) -> IpcViolationReason {
    match win32_code {
        109 | 232 | 233 => IpcViolationReason::ChannelTimeout,
        _ => IpcViolationReason::MalformedMessage,
    }
}

/// Último código de error de pipe ya registrado: evita inundar el log (p. ej. un `MouseMove`
/// por evento con el worker caído) registrando una vez por código hasta que haya un éxito.
static LAST_LOGGED_PIPE_ERROR: AtomicU32 = AtomicU32::new(0);

fn pipe_failure(tab_id: TabId, operation: &'static str) -> GalapagosError {
    let code = unsafe { GetLastError() };
    if LAST_LOGGED_PIPE_ERROR.swap(code, Ordering::Relaxed) != code {
        crate::diagnostics::log_error(
            crate::diagnostics::DiagnosticCategory::Ipc,
            "PIPE_IO_FAILED",
            format!("{} falló en la pestaña {:?} (Win32 {})", operation, tab_id, code),
            Some(code),
        );
    }
    GalapagosError::IpcBoundaryViolation { tab_id, reason: classify_pipe_error(code) }
}

fn pipe_io_ok() {
    LAST_LOGGED_PIPE_ERROR.store(0, Ordering::Relaxed);
}

fn violation(tab_id: TabId, reason: IpcViolationReason) -> GalapagosError {
    GalapagosError::IpcBoundaryViolation { tab_id, reason }
}

fn sandbox_failure(win32_error_code: u32) -> GalapagosError {
    GalapagosError::SandboxEnforcementFailed {
        win32_error_code,
        target_level: WindowsSandboxLevel::AppContainerStrict,
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// T-03: 128 bits del RNG del sistema en hexadecimal, para que el nombre del pipe no sea predecible
fn random_pipe_suffix() -> Result<String, GalapagosError> {
    let mut bytes = [0u8; 16];
    let status = unsafe {
        BCryptGenRandom(null_mut(), bytes.as_mut_ptr(), bytes.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG)
    };
    if status != 0 {
        // NTSTATUS de BCrypt, conservado tal cual en el campo de código del error
        return Err(sandbox_failure(status as u32));
    }
    Ok(bytes.iter().map(|b| format!("{:02x}", b)).collect())
}

/// Descriptor de seguridad construido desde `PIPE_SDDL`; se libera con `LocalFree`
struct PipeSecurityDescriptor(PSECURITY_DESCRIPTOR);

impl PipeSecurityDescriptor {
    fn new() -> Result<Self, GalapagosError> {
        let sddl = to_wide(PIPE_SDDL);
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(), SDDL_REVISION_1, &mut sd, null_mut())
        };
        if ok == 0 {
            return Err(sandbox_failure(unsafe { GetLastError() }));
        }
        Ok(Self(sd))
    }
}

impl Drop for PipeSecurityDescriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// Escribe una trama enmarcada (modo mensaje: un `WriteFile` es un mensaje)
fn write_frame(handle: HANDLE, tab_id: TabId, msg_type: u16, payload: &[u8]) -> Result<(), GalapagosError> {
    let frame = encode_frame(msg_type, payload)?;
    let mut bytes_written: u32 = 0;
    let res = unsafe { WriteFile(handle, frame.as_ptr(), frame.len() as u32, &mut bytes_written, null_mut()) };
    if res == 0 || bytes_written != frame.len() as u32 {
        return Err(pipe_failure(tab_id, "WriteFile"));
    }
    pipe_io_ok();
    Ok(())
}

/// Un mensaje debe contener exactamente una trama completa
fn decode_message(bytes: &[u8], tab_id: TabId) -> Result<(u16, Vec<u8>), GalapagosError> {
    match decode_frame(bytes)? {
        Some((msg_type, payload, consumed)) if consumed == bytes.len() => Ok((msg_type, payload)),
        _ => Err(violation(tab_id, IpcViolationReason::MalformedMessage)),
    }
}

/// Lee un mensaje completo. Las tramas pequeñas se leen en la pila; si el mensaje es mayor
/// (`ERROR_MORE_DATA`), se reserva exactamente la longitud declarada, acotada a 1 MiB de payload.
fn read_frame(handle: HANDLE, tab_id: TabId) -> Result<(u16, Vec<u8>), GalapagosError> {
    let mut head = [0u8; SMALL_FRAME_BYTES];
    let mut read: u32 = 0;
    let ok = unsafe { ReadFile(handle, head.as_mut_ptr(), head.len() as u32, &mut read, null_mut()) };
    let read = read as usize;
    if ok != 0 {
        if read == 0 {
            return Err(violation(tab_id, IpcViolationReason::MalformedMessage));
        }
        return decode_message(&head[..read], tab_id);
    }
    if unsafe { GetLastError() } != ERROR_MORE_DATA {
        return Err(pipe_failure(tab_id, "ReadFile"));
    }

    // Mensaje mayor que SMALL_FRAME_BYTES: la cabecera ya está en `head`
    let declared = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if declared > MAX_ALLOWED_IPC_PAYLOAD_BYTES as usize {
        return Err(violation(tab_id, IpcViolationReason::PayloadTooLarge));
    }
    let total = IPC_HEADER_SIZE + declared;
    if total <= read {
        return Err(violation(tab_id, IpcViolationReason::MalformedMessage));
    }
    let mut buf = vec![0u8; total];
    buf[..read].copy_from_slice(&head[..read]);
    let mut filled = read;
    while filled < total {
        let mut n: u32 = 0;
        let ok = unsafe {
            ReadFile(handle, buf[filled..].as_mut_ptr(), (total - filled) as u32, &mut n, null_mut())
        };
        filled += n as usize;
        if ok != 0 {
            break; // fin del mensaje; si es más corto que lo declarado, `decode_message` lo rechaza
        }
        if unsafe { GetLastError() } != ERROR_MORE_DATA {
            return Err(pipe_failure(tab_id, "ReadFile"));
        }
        if filled >= total {
            // El mensaje es más largo que la longitud declarada en su cabecera
            return Err(violation(tab_id, IpcViolationReason::MalformedMessage));
        }
    }
    decode_message(&buf[..filled], tab_id)
}

/// Extremo del Servidor (Trusted Coordinator) para un Named Pipe restringido
pub struct RestrictedNamedPipeServer {
    pipe_name: String,
    handle: HANDLE,
    tab_id: TabId,
}

// HANDLE en este modelo de arquitectura pertenece a un único hilo coordinador
unsafe impl Send for RestrictedNamedPipeServer {}
unsafe impl Sync for RestrictedNamedPipeServer {}

impl RestrictedNamedPipeServer {
    /// Crea el Named Pipe de una pestaña con nombre no predecible (`galapagos-ipc-tab-<id>-<128 bits>`)
    pub fn create_for_tab(tab_id: TabId) -> Result<Self, GalapagosError> {
        let pipe_name = format!(r"\\.\pipe\galapagos-ipc-tab-{}-{}", tab_id.0, random_pipe_suffix()?);
        Self::create_named(tab_id, pipe_name)
    }

    /// T-03: `FILE_FLAG_FIRST_PIPE_INSTANCE` hace fallar la creación si otro proceso ya tiene
    /// el nombre (pipe squatting) en lugar de compartirlo; `PIPE_REJECT_REMOTE_CLIENTS` corta la red.
    fn create_named(tab_id: TabId, pipe_name: String) -> Result<Self, GalapagosError> {
        let wide_name = to_wide(&pipe_name);
        let sd = PipeSecurityDescriptor::new()?;
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };

        // Búferes circulares de 64 KiB para transporte eficiente
        let buffer_size: u32 = 64 * 1024;

        let handle = unsafe {
            CreateNamedPipeW(
                wide_name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1, // Instancia exclusiva de un solo worker
                buffer_size,
                buffer_size,
                MAX_ALLOWED_IPC_TIMEOUT_MS,
                &mut sa,
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            return Err(sandbox_failure(unsafe { GetLastError() }));
        }

        Ok(Self {
            pipe_name,
            handle,
            tab_id,
        })
    }

    /// Retorna el nombre de la tubería
    pub fn pipe_name(&self) -> &str {
        &self.pipe_name
    }

    /// Espera de forma síncrona la conexión del proceso worker de contenido
    pub fn wait_for_connection(&self) -> Result<(), GalapagosError> {
        let res = unsafe { ConnectNamedPipe(self.handle, null_mut()) };
        if res == 0 {
            let err = unsafe { GetLastError() };
            if err != ERROR_PIPE_CONNECTED {
                return Err(GalapagosError::IpcBoundaryViolation {
                    tab_id: self.tab_id,
                    reason: IpcViolationReason::ChannelTimeout,
                });
            }
        }
        Ok(())
    }

    /// PID del proceso conectado según el kernel (no según lo que declare el propio cliente)
    pub fn client_process_id(&self) -> Result<u32, GalapagosError> {
        let mut pid: u32 = 0;
        if unsafe { GetNamedPipeClientProcessId(self.handle, &mut pid) } == 0 {
            return Err(pipe_failure(self.tab_id, "GetNamedPipeClientProcessId"));
        }
        Ok(pid)
    }

    /// Envía una trama binaria enmarcada hacia el proceso de contenido
    pub fn send_frame(&self, msg_type: u16, payload: &[u8]) -> Result<(), GalapagosError> {
        write_frame(self.handle, self.tab_id, msg_type, payload)
    }

    /// Lee una trama binaria enmarcada desde el proceso de contenido
    pub fn receive_frame(&self) -> Result<(u16, Vec<u8>), GalapagosError> {
        read_frame(self.handle, self.tab_id)
    }

    /// Lectura NO bloqueante (N-I5): `Ok(None)` si no hay un mensaje pendiente en el pipe.
    /// Un pipe roto (worker terminado) se reporta como `ChannelTimeout`.
    pub fn try_receive_frame(&self) -> Result<Option<(u16, Vec<u8>)>, GalapagosError> {
        let mut bytes_avail: u32 = 0;
        let ok = unsafe {
            PeekNamedPipe(self.handle, null_mut(), 0, null_mut(), &mut bytes_avail, null_mut())
        };
        if ok == 0 {
            return Err(pipe_failure(self.tab_id, "PeekNamedPipe"));
        }
        if bytes_avail == 0 {
            return Ok(None);
        }
        // Hay al menos un mensaje completo (pipe en modo mensaje): la lectura no espera
        self.receive_frame().map(Some)
    }

    /// Desconecta el extremo del cliente
    pub fn disconnect_client(&self) {
        unsafe {
            let _ = DisconnectNamedPipe(self.handle);
        }
    }
}

impl Drop for RestrictedNamedPipeServer {
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE && self.handle != 0 {
            unsafe {
                let _ = DisconnectNamedPipe(self.handle);
                let _ = CloseHandle(self.handle);
            }
            self.handle = INVALID_HANDLE_VALUE;
        }
    }
}

/// Extremo del Cliente (Sandboxed Servo Content Process)
pub struct RestrictedNamedPipeClient {
    handle: HANDLE,
    tab_id: TabId,
}

unsafe impl Send for RestrictedNamedPipeClient {}
unsafe impl Sync for RestrictedNamedPipeClient {}

impl RestrictedNamedPipeClient {
    /// Conecta con el Named Pipe expuesto por el Trusted Coordinator
    pub fn connect(pipe_name: &str, tab_id: TabId) -> Result<Self, GalapagosError> {
        let wide_name = to_wide(pipe_name);

        let handle = unsafe {
            CreateFileW(
                wide_name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0, // Sin compartición
                null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                0,
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            return Err(GalapagosError::IpcBoundaryViolation {
                tab_id,
                reason: IpcViolationReason::ChannelTimeout,
            });
        }

        Ok(Self { handle, tab_id })
    }

    /// Envía una trama binaria enmarcada hacia el Trusted Coordinator
    pub fn send_frame(&self, msg_type: u16, payload: &[u8]) -> Result<(), GalapagosError> {
        write_frame(self.handle, self.tab_id, msg_type, payload)
    }

    /// Lee una trama binaria enmarcada desde el Trusted Coordinator
    pub fn receive_frame(&self) -> Result<(u16, Vec<u8>), GalapagosError> {
        read_frame(self.handle, self.tab_id)
    }
}

impl Drop for RestrictedNamedPipeClient {
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE && self.handle != 0 {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
            self.handle = INVALID_HANDLE_VALUE;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_named_pipe_restricted_roundtrip() {
        let tab_id = TabId(888);
        let server = RestrictedNamedPipeServer::create_for_tab(tab_id)
            .expect("Creación exitosa del pipe server");

        let pipe_name = server.pipe_name().to_string();

        let client_handle = thread::spawn(move || {
            let client = RestrictedNamedPipeClient::connect(&pipe_name, tab_id)
                .expect("Conexión del cliente");

            // Enviar mensaje 101 desde el cliente al servidor
            client
                .send_frame(101, b"Payload from AppContainer client")
                .expect("Envío cliente");

            // Recibir respuesta 202 desde el servidor
            let (resp_type, resp_body) = client.receive_frame().expect("Recepción cliente");
            assert_eq!(resp_type, 202);
            assert_eq!(&resp_body, b"Ack from Coordinator");
        });

        server.wait_for_connection().expect("Conexión aceptada");

        // Servidor lee mensaje del cliente
        let (msg_type, payload) = server.receive_frame().expect("Recepción servidor");
        assert_eq!(msg_type, 101);
        assert_eq!(&payload, b"Payload from AppContainer client");

        // Servidor responde
        server
            .send_frame(202, b"Ack from Coordinator")
            .expect("Envío servidor");

        client_handle.join().expect("Hilo cliente completado con éxito");
    }

    /// INC-20261025-CORE-DEBLOAT T-03: DACL presente, no nula y con exactamente 2 ACE (SYSTEM y propietario)
    #[test]
    fn test_pipe_dacl_is_explicit_owner_only_inc20261025() {
        use windows_sys::Win32::Security::{
            AclSizeInformation, GetAclInformation, GetKernelObjectSecurity, GetSecurityDescriptorDacl, ACL,
            ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION,
        };
        let server = RestrictedNamedPipeServer::create_for_tab(TabId(890)).expect("pipe server");
        unsafe {
            let mut needed = 0u32;
            GetKernelObjectSecurity(server.handle, DACL_SECURITY_INFORMATION, null_mut(), 0, &mut needed);
            assert!(needed > 0, "tamaño del descriptor");
            let mut sd = vec![0u8; needed as usize];
            assert_ne!(
                GetKernelObjectSecurity(server.handle, DACL_SECURITY_INFORMATION, sd.as_mut_ptr().cast(), needed, &mut needed),
                0
            );
            let (mut present, mut defaulted) = (0i32, 0i32);
            let mut dacl: *mut ACL = null_mut();
            assert_ne!(GetSecurityDescriptorDacl(sd.as_mut_ptr().cast(), &mut present, &mut dacl, &mut defaulted), 0);
            assert_eq!(present, 1, "DACL presente");
            assert!(!dacl.is_null(), "una DACL nula concede acceso total a todos");
            let mut info: ACL_SIZE_INFORMATION = std::mem::zeroed();
            assert_ne!(
                GetAclInformation(
                    dacl,
                    (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                    std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation
                ),
                0
            );
            assert_eq!(info.AceCount, 2);
        }
    }

    /// T-03: el nombre lleva 128 bits aleatorios; dos pipes de la misma pestaña no colisionan
    #[test]
    fn test_pipe_names_are_unpredictable_inc20261025() {
        let a = RestrictedNamedPipeServer::create_for_tab(TabId(7)).expect("pipe a");
        let b = RestrictedNamedPipeServer::create_for_tab(TabId(7)).expect("pipe b");
        assert_ne!(a.pipe_name(), b.pipe_name());
        for name in [a.pipe_name(), b.pipe_name()] {
            let suffix = name.strip_prefix(r"\\.\pipe\galapagos-ipc-tab-7-").expect("prefijo");
            assert_eq!(suffix.len(), 32);
            assert!(suffix.bytes().all(|c| c.is_ascii_hexdigit()));
        }
    }

    /// T-03: si otro proceso ya creó el nombre (pipe squatting), la creación falla en vez de compartirlo
    #[test]
    fn test_squatted_pipe_name_is_refused_inc20261025() {
        let name = format!(r"\\.\pipe\galapagos-squat-test-{}", std::process::id());
        let wide = to_wide(&name);
        let squatter = unsafe {
            CreateNamedPipeW(wide.as_ptr(), PIPE_ACCESS_DUPLEX, PIPE_TYPE_MESSAGE | PIPE_WAIT, 255, 4096, 4096, 0, null())
        };
        assert_ne!(squatter, INVALID_HANDLE_VALUE, "pipe del atacante");
        let res = RestrictedNamedPipeServer::create_named(TabId(891), name);
        unsafe { CloseHandle(squatter) };
        assert!(matches!(res, Err(GalapagosError::SandboxEnforcementFailed { .. })));
    }

    /// P-01: tramas mayores que el búfer de 64 KiB del pipe se reciben completas (antes: MalformedMessage)
    #[test]
    fn test_frame_larger_than_64kib_roundtrip_inc20261025() {
        let server = RestrictedNamedPipeServer::create_for_tab(TabId(892)).expect("pipe server");
        let pipe_name = server.pipe_name().to_string();
        let payload: Vec<u8> = (0..200 * 1024).map(|i| (i % 251) as u8).collect();
        let expected = payload.clone();
        let client = thread::spawn(move || {
            let client = RestrictedNamedPipeClient::connect(&pipe_name, TabId(892)).expect("cliente");
            client.send_frame(9, &payload).expect("envío");
        });
        server.wait_for_connection().expect("conexión");
        assert_eq!(server.receive_frame(), Ok((9, expected)));
        client.join().unwrap();
    }

    /// P-01: una cabecera que declara más de 1 MiB se rechaza sin reservar esa memoria
    #[test]
    fn test_declared_oversize_frame_rejected_inc20261025() {
        let server = RestrictedNamedPipeServer::create_for_tab(TabId(893)).expect("pipe server");
        let pipe_name = server.pipe_name().to_string();
        let client = thread::spawn(move || {
            let client = RestrictedNamedPipeClient::connect(&pipe_name, TabId(893)).expect("cliente");
            let mut raw = (MAX_ALLOWED_IPC_PAYLOAD_BYTES + 1).to_be_bytes().to_vec();
            raw.extend_from_slice(&1u16.to_be_bytes());
            raw.extend_from_slice(&[0u8; 300]);
            let mut written = 0u32;
            unsafe { WriteFile(client.handle, raw.as_ptr(), raw.len() as u32, &mut written, null_mut()) };
            written
        });
        server.wait_for_connection().expect("conexión");
        assert_eq!(
            server.receive_frame(),
            Err(GalapagosError::IpcBoundaryViolation { tab_id: TabId(893), reason: IpcViolationReason::PayloadTooLarge })
        );
        assert_eq!(client.join().unwrap(), 306);
    }

    /// DELTA_04 L-I3: tabla de mapeo Win32 -> motivo de violación
    #[test]
    fn test_pipe_error_classification_delta04() {
        for code in [109u32, 232, 233] {
            assert_eq!(classify_pipe_error(code), IpcViolationReason::ChannelTimeout, "{}", code);
        }
        for code in [0u32, 5, 234, 997, 1234] {
            assert_eq!(classify_pipe_error(code), IpcViolationReason::MalformedMessage, "{}", code);
        }
    }

    /// DELTA_02 N-I5: la lectura no bloquea sin datos, entrega un mensaje por llamada y detecta pipe roto
    #[test]
    fn test_try_receive_frame_is_nonblocking_delta02() {
        let tab_id = TabId(889);
        let server = RestrictedNamedPipeServer::create_for_tab(tab_id).expect("pipe server");
        let pipe_name = server.pipe_name().to_string();
        let (to_client, from_server) = std::sync::mpsc::channel::<()>();
        let (to_server, from_client) = std::sync::mpsc::channel::<()>();

        let client = thread::spawn(move || {
            let client = RestrictedNamedPipeClient::connect(&pipe_name, tab_id).expect("cliente");
            to_server.send(()).unwrap(); // conectado
            from_server.recv().unwrap(); // el servidor ya comprobó "sin datos"
            client.send_frame(7, b"uno").unwrap();
            client.send_frame(8, b"dos").unwrap();
            to_server.send(()).unwrap(); // enviado; el cliente se cierra al salir
        });

        server.wait_for_connection().expect("conexión");
        from_client.recv().unwrap();

        assert_eq!(server.try_receive_frame(), Ok(None));
        to_client.send(()).unwrap();
        from_client.recv().unwrap();
        client.join().unwrap();

        assert_eq!(server.try_receive_frame(), Ok(Some((7, b"uno".to_vec()))));
        assert_eq!(server.try_receive_frame(), Ok(Some((8, b"dos".to_vec()))));
        // Cliente cerrado y pipe vacío: se reporta el canal roto, no una espera
        assert_eq!(
            server.try_receive_frame(),
            Err(GalapagosError::IpcBoundaryViolation {
                tab_id,
                reason: IpcViolationReason::ChannelTimeout,
            })
        );
    }
}
