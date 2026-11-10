//! The service's named pipe: one JSON request per connection, one answer.
//!
//! Its DACL admits SYSTEM, administrators and the one configured user, and
//! remote clients are rejected. The first instance is created with
//! `FILE_FLAG_FIRST_PIPE_INSTANCE`, so the service fails rather than serving
//! behind another process that grabbed the name first.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::windows::io::FromRawHandle;
use std::ptr;

use hearth_core::vmd::{PIPE_NAME, Request, Response, UNKNOWN_REQUEST, VmState};
use windows_sys::Win32::Foundation::{
    ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FlushFileBuffers, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

/// SDDL of the pipe: full access for SYSTEM and administrators, read and
/// write for the configured user, nobody else. `P` keeps inherited entries
/// out. The SID is validated by `config::is_user_sid` before.
pub fn sddl(user_sid: &str) -> String {
    format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{user_sid})")
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// A security descriptor made from SDDL, freed on drop.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn new(sddl: &str) -> io::Result<Descriptor> {
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        let text = wide(sddl);
        // SAFETY: valid NUL-terminated string and out pointer; revision 1.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Descriptor(descriptor))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: allocated by the conversion above with LocalAlloc.
        unsafe { LocalFree(self.0) };
    }
}

/// Answers clients until an error occurs. Each client gets its own thread,
/// so a long start does not keep `status` from answering.
pub fn serve<H>(name: &str, user_sid: &str, handler: H) -> io::Result<()>
where
    H: Fn(Request) -> Response + Send + Sync + 'static,
{
    let descriptor = Descriptor::new(&sddl(user_sid))?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let handler = std::sync::Arc::new(handler);
    let name = wide(name);
    let mut first = true;
    loop {
        let flags = PIPE_ACCESS_DUPLEX
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        // SAFETY: valid name and security attributes that outlive the call.
        let pipe = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                flags,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                &attributes,
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        first = false;
        // SAFETY: a fresh pipe handle; blocks until a client connects.
        let connected = unsafe { ConnectNamedPipe(pipe, ptr::null_mut()) } != 0
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        // SAFETY: we own the handle; File closes it.
        let file = unsafe { File::from_raw_handle(pipe) };
        if !connected {
            continue;
        }
        let handler = std::sync::Arc::clone(&handler);
        std::thread::spawn(move || answer(file, &*handler));
    }
}

fn answer(file: File, handler: &dyn Fn(Request) -> Response) {
    let mut line = String::new();
    let Ok(mut writer) = file.try_clone() else {
        return;
    };
    let read = BufReader::new(file).take(4096).read_line(&mut line);
    let response = match read
        .ok()
        .and_then(|_| serde_json::from_str::<Request>(line.trim()).ok())
    {
        Some(request) => handler(request),
        None => Response {
            ok: false,
            state: VmState::Unknown,
            message: UNKNOWN_REQUEST.to_owned(),
        },
    };
    let mut text = serde_json::to_string(&response).unwrap_or_default();
    text.push('\n');
    let _ = writer.write_all(text.as_bytes());
    // SAFETY: valid handle; lets the client read everything before close.
    unsafe { FlushFileBuffers(std::os::windows::io::AsRawHandle::as_raw_handle(&writer)) };
}

/// The pipe the CLI talks to.
pub fn default_name() -> &'static str {
    PIPE_NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_requests_over_the_pipe() {
        let name = format!(r"\\.\pipe\hearth-vmd-test-{}", std::process::id());
        let sid = hearth_core::vmd::current_user_sid().unwrap();
        let server_name = name.clone();
        std::thread::spawn(move || {
            serve(&server_name, &sid, |request| Response {
                ok: true,
                state: VmState::Stopped,
                message: format!("{request:?}"),
            })
        });
        let ask = |line: &str| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut pipe = loop {
                match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&name)
                {
                    Ok(pipe) => break pipe,
                    Err(_) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            pipe.write_all(line.as_bytes()).unwrap();
            let mut answer = String::new();
            BufReader::new(pipe).read_line(&mut answer).unwrap();
            serde_json::from_str::<Response>(answer.trim()).unwrap()
        };
        let answer = ask("{\"op\":\"start\"}\n");
        assert!(answer.ok);
        assert_eq!(answer.message, "Start");
        // Several clients one after the other, and nonsense is refused.
        assert_eq!(ask("{\"op\":\"status\"}\n").message, "Status");
        assert!(!ask("rm -rf /\n").ok);
    }

    #[test]
    fn sddl_admits_only_the_configured_user() {
        assert_eq!(
            sddl("S-1-5-21-1-2-3-1001"),
            "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;S-1-5-21-1-2-3-1001)"
        );
        assert!(Descriptor::new(&sddl("S-1-5-21-1-2-3-1001")).is_ok());
    }
}
