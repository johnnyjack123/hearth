//! The VM service `hearth-vmd` (docs/spikes/phase-0.md §12.3): under
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

use crate::lang::tr;
use crate::{Error, Result, tr};

/// Name of the Windows service.
pub const SERVICE_NAME: &str = "hearth-vmd";

/// The service's pipe; its DACL admits SYSTEM, administrators and the user
/// the service was installed for.
pub const PIPE_NAME: &str = r"\\.\pipe\hearth-vmd";

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
    /// Give the space of deleted files back to Windows: shrink the disk file
    /// while the machine is off, then start it again if it ran. The caller
    /// trims inside the VM first (`PodmanProvider::trim`); services before
    /// 0.1.5 tried that themselves and failed.
    Compact,
    /// Stop the idle machine, then shrink its disk file if it has grown
    /// since the last time. The caller trims first, as for `Compact`.
    /// Services before 0.1.5 answer [`UNKNOWN_REQUEST`].
    #[serde(rename = "stop-idle")]
    StopIdle,
}

/// The service's message for a line it cannot read, e.g. a request newer
/// than the service. Always German: the CLI recognizes services before
/// 0.1.5 by exactly this text.
pub const UNKNOWN_REQUEST: &str = "unverständliche Anfrage";

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
    /// What happened, or why it failed (in the service's language, shown to
    /// the user).
    #[serde(default)]
    pub message: String,
}

/// `%ProgramData%\hearth\vmd`: configuration, log and podman's
/// environment of the service; writable for administrators only.
pub fn service_dir() -> PathBuf {
    // ProgramData of the system drive rather than %ProgramData%: the service
    // hands podman a ProgramData of its own, and service, installer and CLI
    // must agree on the location.
    let base = env::var_os("SystemDrive")
        .map(|drive| PathBuf::from(format!("{}\\", drive.to_string_lossy())))
        .unwrap_or_else(|| PathBuf::from("C:\\"));
    base.join("ProgramData").join("hearth").join("vmd")
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
        .map_err(|e| Error::Vmd(tr!("Anfrage kodieren: {e}", "encoding the request: {e}")))?;
    line.push('\n');
    pipe.write_all(line.as_bytes())
        .and_then(|()| pipe.flush())
        .map_err(|e| Error::Vmd(tr!("Anfrage senden: {e}", "sending the request: {e}")))?;
    let mut answer = String::new();
    BufReader::new(pipe)
        .take(64 * 1024)
        .read_line(&mut answer)
        .map_err(|e| Error::Vmd(tr!("Antwort lesen: {e}", "reading the answer: {e}")))?;
    serde_json::from_str(answer.trim()).map_err(|e| {
        Error::Vmd(tr!(
            "unverständliche Antwort ({e}): {}",
            "answer not understood ({e}): {}",
            answer.trim()
        ))
    })
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
                    tr(
                        "nicht installiert (einmalig: hearth vmd install)",
                        "not installed (once: hearth vmd install)",
                    )
                    .to_owned(),
                ));
            }
            Err(e) => {
                return Err(Error::Vmd(tr!(
                    "keine Verbindung: {e}",
                    "no connection: {e}"
                )));
            }
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
        return Err(failed(tr(
            "Prozess-Token öffnen",
            "opening the process token",
        )));
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
        return Err(failed(tr(
            "Nutzer des Tokens lesen",
            "reading the token's user",
        )));
    }
    // SAFETY: GetTokenInformation filled a TOKEN_USER at the buffer's start.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: valid SID; the string is allocated by Windows.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(failed(tr("SID umwandeln", "converting the SID")));
    }
    // SAFETY: NUL-terminated string from ConvertSidToStringSidW.
    let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    let result = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    // SAFETY: allocated with LocalAlloc by the conversion.
    unsafe { LocalFree(text.cast()) };
    Ok(result)
}

/// SID of the owner of a file or folder, e.g. `S-1-5-32-544`.
#[cfg(windows)]
pub fn owner_sid(path: &std::path::Path) -> Result<String> {
    use std::os::windows::ffi::OsStrExt as _;

    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut owner: PSID = std::ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: NUL-terminated path and valid out pointers; the owner points
    // into the descriptor, which is freed below.
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(Error::Vmd(tr!(
            "Besitzer von {} lesen: {}",
            "Reading the owner of {}: {}",
            path.display(),
            std::io::Error::from_raw_os_error(status as i32)
        )));
    }
    let mut text: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: valid SID; the string is allocated by Windows.
    let converted = unsafe { ConvertSidToStringSidW(owner, &mut text) } != 0;
    let result = if converted {
        // SAFETY: NUL-terminated string from ConvertSidToStringSidW.
        let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
        let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
        // SAFETY: allocated with LocalAlloc by the conversion.
        unsafe { LocalFree(text.cast()) };
        Ok(sid)
    } else {
        Err(Error::Vmd(format!(
            "SID: {}",
            std::io::Error::last_os_error()
        )))
    };
    // SAFETY: allocated by GetNamedSecurityInfoW.
    unsafe { LocalFree(descriptor.cast()) };
    result
}

/// Owners the service's data folder may have: SYSTEM and Administrators.
pub const TRUSTED_OWNERS: [&str; 2] = ["S-1-5-18", "S-1-5-32-544"];

/// Checks the service's data folder (`C:\ProgramData\hearth`) before
/// anything runs as SYSTEM from it. Any user may create folders in
/// ProgramData: another account of a shared computer could prepare it
/// before the first install, with a junction inside that sends what the
/// installer writes, or what the service reads, elsewhere (security review
/// M8, 3.11.). A folder that does not exist yet passes.
///
/// `also` is an owner trusted besides [`TRUSTED_OWNERS`] (the installing
/// user, whose own old folder is taken over). Reparse points (junctions,
/// symlinks) fail anywhere in the tree with `deep`, else only on the way to
/// what the service reads (the service starts with that: after the install
/// only administrators can add anything).
#[cfg(windows)]
pub fn check_service_root(root: &std::path::Path, also: Option<&str>, deep: bool) -> Result<()> {
    use std::os::windows::fs::MetadataExt as _;

    const REPARSE_POINT: u32 = 0x400;
    let Ok(meta) = std::fs::symlink_metadata(root) else {
        return Ok(());
    };
    let owner = owner_sid(root)?;
    if !TRUSTED_OWNERS.contains(&owner.as_str()) && Some(owner.as_str()) != also {
        return Err(Error::Vmd(tr!(
            "{} gehört einem anderen Konto ({owner}). Bitte prüfen und als Administrator \
             löschen, dann neu einrichten.",
            "{} belongs to another account ({owner}). Please check it and delete it as \
             administrator, then set up again.",
            root.display()
        )));
    }
    let mut pending = vec![(root.to_path_buf(), meta)];
    if !deep {
        for path in [service_dir(), service_dir().join("vmd.toml")] {
            if path.starts_with(root)
                && let Ok(meta) = std::fs::symlink_metadata(&path)
            {
                pending.push((path, meta));
            }
        }
    }
    while let Some((path, meta)) = pending.pop() {
        if meta.file_attributes() & REPARSE_POINT != 0 {
            return Err(Error::Vmd(tr!(
                "{} ist eine Verknüpfung (Junction oder Symlink); im Datenordner des Dienstes \
                 darf keine sein. Bitte prüfen und als Administrator löschen.",
                "{} is a link (junction or symlink); the service's data folder must not hold \
                 one. Please check it and delete it as administrator.",
                path.display()
            )));
        }
        if meta.is_dir() && deep {
            let entries = std::fs::read_dir(&path)
                .map_err(|error| Error::Vmd(format!("{}: {error}", path.display())))?;
            for entry in entries.flatten() {
                if let Ok(meta) = std::fs::symlink_metadata(entry.path()) {
                    pending.push((entry.path(), meta));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn service_roots_with_links_or_foreign_owners_fail() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("hearth");
        // Missing: the installer creates it.
        check_service_root(&root, None, true).unwrap();
        std::fs::create_dir_all(root.join("vmd")).unwrap();
        // Created by this user: only trusted when named.
        let me = current_user_sid().unwrap();
        assert!(check_service_root(&root, None, true).is_err());
        check_service_root(&root, Some(&me), true).unwrap();
        // A junction inside, as `mklink /J` makes it without admin rights.
        let target = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&target).unwrap();
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("vmd").join("home"))
            .arg(&target)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
        let error = check_service_root(&root, Some(&me), true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Verknüpfung"), "{error}");
        // Not looked for when the service starts: only administrators can
        // add anything after the install.
        check_service_root(&root, Some(&me), false).unwrap();
    }

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
        assert_eq!(
            serde_json::to_string(&Request::StopIdle).unwrap(),
            r#"{"op":"stop-idle"}"#
        );
        let answer: Response =
            serde_json::from_str(r#"{"ok":true,"state":"unresponsive"}"#).unwrap();
        assert_eq!(answer.state, VmState::Unresponsive);
        assert!(answer.message.is_empty());
        assert!(serde_json::from_str::<Request>(r#"{"op":"rm"}"#).is_err());
    }

    #[test]
    fn service_paths_are_fixed() {
        let json = machine_json("hearth");
        assert!(json.starts_with(service_dir()));
        assert!(json.ends_with("hyperv\\hearth.json") || json.ends_with("hyperv/hearth.json"));
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
