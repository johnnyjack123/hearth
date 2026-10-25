//! The VM service `harness-vmd` (docs/spikes/phase-0.md §12.3): under
//! Hyper-V every `podman machine` command needs admin rights, so a Windows
//! service starts and stops the machine for the one user it was installed
//! for, who talks to it over a named pipe.
//!
//! The service runs podman as SYSTEM with an environment of its own below
//! [`service_dir`], which only administrators can write. podman must not
//! read anything a user process can change there: `containers.conf` alone
//! can name the helper programs podman starts, which would run as SYSTEM.

use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Name of the Windows service.
pub const SERVICE_NAME: &str = "llm-harness-vmd";

/// The service's pipe; its DACL admits SYSTEM, administrators and the user
/// the service was installed for.
pub const PIPE_NAME: &str = r"\\.\pipe\llm-harness-vmd";

/// A request to the service, one JSON object per line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Request {
    /// State of the machine.
    Status,
    /// Start the machine unless it answers SSH already.
    Start,
    /// Stop the machine.
    Stop,
    /// Stop and start, e.g. when SSH stopped answering.
    Restart,
    /// Give the space of deleted files back to Windows: TRIM inside the VM,
    /// then shrink its disk file while it is off. Starts it again if it ran.
    Compact,
}

/// State of the machine as the service sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VmState {
    /// Running and answering SSH.
    Running,
    /// Running, but SSH does not answer (e.g. after standby).
    Unresponsive,
    /// Not running.
    Stopped,
    /// Unknown, e.g. the machine does not exist.
    Unknown,
}

/// The service's answer to a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    /// Whether the request succeeded.
    pub ok: bool,
    /// State after the request.
    pub state: VmState,
    /// What happened, or why it failed (German, shown to the user).
    #[serde(default)]
    pub message: String,
}

/// `%ProgramData%\llm-harness\vmd`: configuration, log and podman's
/// environment of the service; writable for administrators only.
pub fn service_dir() -> PathBuf {
    // ProgramData of the system drive rather than %ProgramData%: the service
    // hands podman a ProgramData of its own, and service, installer and CLI
    // must agree on the location.
    let base = env::var_os("SystemDrive")
        .map(|drive| PathBuf::from(format!("{}\\", drive.to_string_lossy())))
        .unwrap_or_else(|| PathBuf::from("C:\\"));
    base.join("ProgramData").join("llm-harness").join("vmd")
}

/// HOME and USERPROFILE of podman inside the service.
pub fn podman_home() -> PathBuf {
    service_dir().join("home")
}

/// Machine JSON of a machine the service owns.
pub fn machine_json(name: &str) -> PathBuf {
    podman_home()
        .join(".config")
        .join("containers")
        .join("podman")
        .join("machine")
        .join("hyperv")
        .join(format!("{name}.json"))
}

/// How long [`ssh_answers`] waits for the server's greeting.
pub const SSH_BANNER_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether an SSH server answers on `127.0.0.1:port`. A TCP connection is
/// not enough: gvproxy accepts connections even when the VM's sshd does not
/// answer, as after standby on 14.10.2026.
pub fn ssh_answers(port: u16, timeout: Duration) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let mut greeting = [0u8; 4];
    stream.read_exact(&mut greeting).is_ok() && &greeting == b"SSH-"
}

/// Sends one request to the service and waits for its answer, which for a
/// start can take minutes.
pub fn request(request: Request) -> Result<Response> {
    let mut pipe = open_pipe()?;
    let mut line = serde_json::to_string(&request)
        .map_err(|e| Error::Vmd(format!("Anfrage kodieren: {e}")))?;
    line.push('\n');
    pipe.write_all(line.as_bytes())
        .and_then(|()| pipe.flush())
        .map_err(|e| Error::Vmd(format!("Anfrage senden: {e}")))?;
    let mut answer = String::new();
    BufReader::new(pipe)
        .take(64 * 1024)
        .read_line(&mut answer)
        .map_err(|e| Error::Vmd(format!("Antwort lesen: {e}")))?;
    serde_json::from_str(answer.trim())
        .map_err(|e| Error::Vmd(format!("unverständliche Antwort ({e}): {}", answer.trim())))
}

/// Whether the service is installed and listening.
pub fn available() -> bool {
    open_pipe().is_ok()
}

fn open_pipe() -> Result<std::fs::File> {
    // All instances can be busy for a moment while the service accepts
    // another client.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(PIPE_NAME)
        {
            Ok(pipe) => return Ok(pipe),
            // ERROR_PIPE_BUSY
            Err(e) if e.raw_os_error() == Some(231) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::Vmd(
                    "nicht installiert (einmalig: harness vmd install)".to_owned(),
                ));
            }
            Err(e) => return Err(Error::Vmd(format!("keine Verbindung: {e}"))),
        }
    }
}

/// `S-1-5-21-<a>-<b>-<c>-<rid>`: a local or domain account. Anything else is
/// refused, so the value cannot inject entries into the pipe's SDDL.
pub fn is_user_sid(sid: &str) -> bool {
    let Some(rest) = sid.strip_prefix("S-1-5-21-") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('-').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 10 && p.chars().all(|c| c.is_ascii_digit()))
}

/// Whether `path` is absolute and inside `C:\Program Files` (or the
/// Program Files of the system drive), compared without regard to case.
pub fn below_program_files(path: &std::path::Path) -> bool {
    let text = path.to_string_lossy().to_lowercase().replace('/', "\\");
    let drive = std::env::var("SystemDrive")
        .unwrap_or_else(|_| "C:".to_owned())
        .to_lowercase();
    let root = format!("{drive}\\program files\\");
    text.starts_with(&root) && !text.split('\\').any(|part| part == "..")
}

/// SID of the user running this process, e.g. `S-1-5-21-…-1001`.
#[cfg(windows)]
pub fn current_user_sid() -> Result<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let failed = |what: &str| Error::Vmd(format!("{what}: {}", std::io::Error::last_os_error()));
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: pseudo handle of this process and a valid out pointer.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(failed("Prozess-Token öffnen"));
    }
    let mut buffer = vec![0u64; 64];
    let mut needed = 0u32;
    // SAFETY: the buffer is 512 bytes, u64-aligned, enough for TOKEN_USER.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            &mut needed,
        )
    };
    // SAFETY: opened above.
    unsafe { CloseHandle(token) };
    if ok == 0 {
        return Err(failed("Nutzer des Tokens lesen"));
    }
    // SAFETY: GetTokenInformation filled a TOKEN_USER at the buffer's start.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: valid SID; the string is allocated by Windows.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(failed("SID umwandeln"));
    }
    // SAFETY: NUL-terminated string from ConvertSidToStringSidW.
    let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    let result = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    // SAFETY: allocated with LocalAlloc by the conversion.
    unsafe { LocalFree(text.cast()) };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn current_user_has_a_sid() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-5-"), "{sid}");
    }

    #[test]
    fn checks_of_what_the_service_trusts() {
        assert!(is_user_sid("S-1-5-21-1004336348-1177238915-682003330-1001"));
        assert!(!is_user_sid("S-1-5-18"));
        assert!(!is_user_sid("S-1-5-21-1-2-3-4)(A;;GA;;;WD"));
        assert!(below_program_files(std::path::Path::new(
            r"C:\Program Files\RedHat\Podman\podman.exe"
        )));
        assert!(!below_program_files(std::path::Path::new(
            r"C:\Program Files\..\Users\x\podman.exe"
        )));
        assert!(!below_program_files(std::path::Path::new(
            r"C:\Users\x\podman.exe"
        )));
    }

    #[test]
    fn protocol_lines() {
        assert_eq!(
            serde_json::to_string(&Request::Restart).unwrap(),
            r#"{"op":"restart"}"#
        );
        let answer: Response =
            serde_json::from_str(r#"{"ok":true,"state":"unresponsive"}"#).unwrap();
        assert_eq!(answer.state, VmState::Unresponsive);
        assert!(answer.message.is_empty());
        assert!(serde_json::from_str::<Request>(r#"{"op":"rm"}"#).is_err());
    }

    #[test]
    fn service_paths_are_fixed() {
        let json = machine_json("harness");
        assert!(json.starts_with(service_dir()));
        assert!(json.ends_with("hyperv\\harness.json") || json.ends_with("hyperv/harness.json"));
    }

    #[test]
    fn ssh_greeting_is_required() {
        use std::net::TcpListener;
        // Accepts but never greets, like gvproxy in front of a hung VM.
        let silent = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = silent.local_addr().unwrap().port();
        let hold = std::thread::spawn(move || silent.accept().map(|(s, _)| s));
        assert!(!ssh_answers(port, Duration::from_millis(300)));
        drop(hold.join());

        let greeting = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = greeting.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = greeting.accept() {
                let _ = stream.write_all(b"SSH-2.0-OpenSSH_9.9\r\n");
            }
        });
        assert!(ssh_answers(port, Duration::from_secs(2)));
    }
}
