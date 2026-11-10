//! Windows pseudo consoles (ConPTY): runs a program on a console that this
//! process owns instead of the user's terminal.
//!
//! `hearth run` puts `podman exec -it` on one so it can keep the terminal
//! to itself for a moment and show the gateway's questions; tests use it as
//! a scriptable terminal for `hearth` itself. The program sees a real
//! console, so it switches to raw mode and reports size changes as usual.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Command;
use std::ptr;
use std::time::Duration;

use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject,
};

/// Size of a console in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
}

impl Size {
    fn coord(self) -> COORD {
        // COORD is signed; clamping keeps absurd sizes from wrapping.
        COORD {
            X: self.cols.clamp(1, i16::MAX as u16) as i16,
            Y: self.rows.clamp(1, i16::MAX as u16) as i16,
        }
    }
}

/// A process attached to its own pseudo console.
pub struct PtyProcess {
    console: HPCON,
    process: OwnedHandle,
    input: Option<File>,
    output: Option<File>,
}

// The console handle is only an opaque token for kernel32 calls, which may
// come from any thread.
unsafe impl Send for PtyProcess {}
unsafe impl Sync for PtyProcess {}

fn last_error() -> io::Error {
    io::Error::last_os_error()
}

fn hresult(code: i32) -> io::Result<()> {
    if code >= 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}

fn pipe() -> io::Result<(File, File)> {
    let (mut read, mut write): (HANDLE, HANDLE) = (ptr::null_mut(), ptr::null_mut());
    // SAFETY: both out pointers are valid; no security attributes, so the
    // handles are not inheritable.
    if unsafe { CreatePipe(&mut read, &mut write, ptr::null(), 0) } == 0 {
        return Err(last_error());
    }
    // SAFETY: CreatePipe returned two fresh handles that nothing else owns.
    Ok(unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) })
}

impl PtyProcess {
    /// Starts `command` (program, arguments, environment changes and working
    /// directory; its stdio settings are ignored) on a new pseudo console.
    pub fn spawn(command: &Command, size: Size) -> io::Result<PtyProcess> {
        let (input_read, input_write) = pipe()?;
        let (output_read, output_write) = pipe()?;
        let mut console: HPCON = 0;
        // SAFETY: the pipe handles are valid; the console duplicates them.
        hresult(unsafe {
            CreatePseudoConsole(
                size.coord(),
                input_read.as_raw_handle(),
                output_write.as_raw_handle(),
                0,
                &mut console,
            )
        })?;
        // The console holds its own copies; ours would keep the pipes open
        // after it closes, and the output reader would never see the end.
        drop(input_read);
        drop(output_write);

        match spawn_on(console, command) {
            Ok(process) => Ok(PtyProcess {
                console,
                process,
                input: Some(input_write),
                output: Some(output_read),
            }),
            Err(error) => {
                // SAFETY: the console was created above and is not used after.
                unsafe { ClosePseudoConsole(console) };
                Err(error)
            }
        }
    }

    /// What the program writes, as the console renders it (VT sequences).
    /// Must be read continuously, otherwise the program blocks. Ends after
    /// [`PtyProcess::close`].
    pub fn take_output(&mut self) -> Option<File> {
        self.output.take()
    }

    /// Keyboard input for the program, as VT sequences.
    pub fn take_input(&mut self) -> Option<File> {
        self.input.take()
    }

    /// Changes the console size; the program sees it like a window resize.
    pub fn resize(&self, size: Size) -> io::Result<()> {
        // SAFETY: the console stays open while self lives.
        hresult(unsafe { ResizePseudoConsole(self.console, size.coord()) })
    }

    /// Waits for the program to end and returns its exit code, or `None`
    /// if `timeout` passed first. `None` as timeout waits forever.
    pub fn wait(&self, timeout: Option<Duration>) -> io::Result<Option<u32>> {
        let millis = timeout.map_or(INFINITE, |t| {
            t.as_millis().min(u128::from(INFINITE - 1)) as u32
        });
        // SAFETY: the process handle is owned by self.
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), millis) } {
            WAIT_OBJECT_0 => {
                let mut code = 0u32;
                // SAFETY: as above; `code` is a valid out pointer.
                if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
                    return Err(last_error());
                }
                Ok(Some(code))
            }
            WAIT_TIMEOUT => Ok(None),
            _ => Err(last_error()),
        }
    }

    /// Ends the program forcibly.
    pub fn kill(&self) -> io::Result<()> {
        // SAFETY: the process handle is owned by self.
        if unsafe { TerminateProcess(self.process.as_raw_handle(), 1) } == 0 {
            return Err(last_error());
        }
        Ok(())
    }

    /// Closes the console, which ends the output once everything rendered
    /// so far has been read. The output must still be drained meanwhile:
    /// on older Windows versions closing waits for that.
    pub fn close(mut self) {
        self.close_console();
    }

    fn close_console(&mut self) {
        if self.console != 0 {
            // Closing the input first lets the console wind down.
            self.input = None;
            // SAFETY: the console is open and closed exactly once.
            unsafe { ClosePseudoConsole(self.console) };
            self.console = 0;
        }
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        self.close_console();
    }
}

fn spawn_on(console: HPCON, command: &Command) -> io::Result<OwnedHandle> {
    let mut size = 0usize;
    // SAFETY: the first call only reports the size needed; it fails by design.
    unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) };
    // usize elements keep the list pointer-aligned.
    let mut buffer = vec![0usize; size.div_ceil(mem::size_of::<usize>())];
    let list = buffer.as_mut_ptr().cast::<std::ffi::c_void>() as LPPROC_THREAD_ATTRIBUTE_LIST;
    // SAFETY: the buffer has the size the first call asked for.
    if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut size) } == 0 {
        return Err(last_error());
    }
    let result = (|| {
        // SAFETY: for this attribute the value is the console handle itself.
        let ok = unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console as *const std::ffi::c_void,
                mem::size_of::<HPCON>(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        if ok == 0 {
            return Err(last_error());
        }
        let mut startup: STARTUPINFOEXW = unsafe { mem::zeroed() };
        startup.StartupInfo.cb = mem::size_of::<STARTUPINFOEXW>() as u32;
        // Without explicit (invalid) std handles, a child of a process whose
        // stdio is redirected would write there instead of to the console.
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
        startup.lpAttributeList = list;

        let mut command_line = command_line(command);
        let environment = environment_block(command);
        let directory: Option<Vec<u16>> =
            command.get_current_dir().map(|dir| wide(dir.as_os_str()));
        let mut info: PROCESS_INFORMATION = unsafe { mem::zeroed() };
        // SAFETY: all buffers are NUL-terminated and outlive the call; the
        // command line buffer is mutable as CreateProcessW requires.
        let ok = unsafe {
            CreateProcessW(
                ptr::null(),
                command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                directory.as_ref().map_or(ptr::null(), |d| d.as_ptr()),
                &startup.StartupInfo,
                &mut info,
            )
        };
        if ok == 0 {
            return Err(last_error());
        }
        // SAFETY: CreateProcessW returned both handles; we own them now.
        unsafe {
            drop(OwnedHandle::from_raw_handle(info.hThread));
            Ok(OwnedHandle::from_raw_handle(info.hProcess))
        }
    })();
    // SAFETY: the list was initialized above and is no longer used.
    unsafe { DeleteProcThreadAttributeList(list) };
    result
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain([0]).collect()
}

/// The command line for `CreateProcessW`, quoted so that the C runtime's and
/// Go's argument parsing (podman) give back exactly the original arguments.
pub fn command_line(command: &Command) -> Vec<u16> {
    let mut line: Vec<u16> = Vec::new();
    let program = command.get_program();
    for (i, arg) in std::iter::once(program)
        .chain(command.get_args())
        .enumerate()
    {
        if i > 0 {
            line.push(u16::from(b' '));
        }
        quote_into(&mut line, arg);
    }
    line.push(0);
    line
}

fn quote_into(line: &mut Vec<u16>, arg: &OsStr) {
    let units: Vec<u16> = arg.encode_wide().collect();
    let plain = !units.is_empty()
        && !units.iter().any(|&u| {
            u == u16::from(b' ')
                || u == u16::from(b'\t')
                || u == u16::from(b'"')
                || u == u16::from(b'\n')
        });
    if plain {
        line.extend(units);
        return;
    }
    line.push(u16::from(b'"'));
    let mut backslashes = 0usize;
    for &unit in &units {
        if unit == u16::from(b'\\') {
            backslashes += 1;
            continue;
        }
        // Backslashes are literal unless they precede a quote; then each one
        // needs escaping, and the quote too.
        let repeat = if unit == u16::from(b'"') {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        line.extend(std::iter::repeat_n(u16::from(b'\\'), repeat));
        line.push(unit);
        backslashes = 0;
    }
    // Before the closing quote, trailing backslashes double.
    line.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes * 2));
    line.push(u16::from(b'"'));
}

/// This process's environment with the command's changes, as a sorted
/// `KEY=value\0…\0\0` block.
pub fn environment_block(command: &Command) -> Vec<u16> {
    let mut vars: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    for (key, value) in command.get_envs() {
        let upper = key.to_ascii_uppercase();
        vars.retain(|(k, _)| k.to_ascii_uppercase() != upper);
        if let Some(value) = value {
            vars.push((key.to_owned(), value.to_owned()));
        }
    }
    // Windows expects the block sorted by name, ignoring case.
    vars.sort_by_key(|(key, _)| key.to_ascii_uppercase());
    let mut block = Vec::new();
    for (key, value) in vars {
        block.extend(key.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// Splits a command line the way the C runtime does; for tests.
#[cfg(test)]
fn split(line: &[u16]) -> Vec<OsString> {
    use std::os::windows::ffi::OsStringExt;

    let line = &line[..line.iter().position(|&u| u == 0).unwrap_or(line.len())];
    let mut args = Vec::new();
    let mut current: Vec<u16> = Vec::new();
    let mut in_quotes = false;
    let mut have = false;
    let mut i = 0;
    while i < line.len() {
        let unit = line[i];
        if unit == u16::from(b'\\') {
            let start = i;
            while i < line.len() && line[i] == u16::from(b'\\') {
                i += 1;
            }
            let count = i - start;
            if i < line.len() && line[i] == u16::from(b'"') {
                current.extend(std::iter::repeat_n(u16::from(b'\\'), count / 2));
                if count % 2 == 1 {
                    current.push(u16::from(b'"'));
                    i += 1;
                }
            } else {
                current.extend(std::iter::repeat_n(u16::from(b'\\'), count));
            }
            have = true;
            continue;
        }
        if unit == u16::from(b'"') {
            in_quotes = !in_quotes;
            have = true;
        } else if (unit == u16::from(b' ') || unit == u16::from(b'\t')) && !in_quotes {
            if have {
                args.push(OsString::from_wide(&current));
                current.clear();
                have = false;
            }
        } else {
            current.push(unit);
            have = true;
        }
        i += 1;
    }
    if have {
        args.push(OsString::from_wide(&current));
    }
    args
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn arguments_survive_quoting() {
        let args = [
            "plain",
            "",
            "with space",
            "quote\"inside",
            "trailing\\",
            "trailing space\\",
            "back\\\\\"slash",
            "multi\nline \"text\" with `ticks` and $vars",
            "C:\\Program Files\\x\\",
        ];
        let mut command = Command::new("C:\\Program Files\\RedHat\\Podman\\podman.exe");
        command.args(args);
        let parsed = split(&command_line(&command));
        assert_eq!(parsed[0], "C:\\Program Files\\RedHat\\Podman\\podman.exe");
        assert_eq!(parsed[1..], args.map(OsString::from));
    }

    #[test]
    fn environment_changes_apply() {
        let mut command = Command::new("x");
        command
            .env("HEARTH_PTY_TEST", "a=b")
            .env_remove("PATH")
            .env("path_like", "1");
        let block = environment_block(&command);
        let text = String::from_utf16(&block).unwrap();
        let entries: Vec<&str> = text.split('\0').filter(|e| !e.is_empty()).collect();
        assert!(entries.contains(&"HEARTH_PTY_TEST=a=b"));
        assert!(
            !entries
                .iter()
                .any(|e| e.to_ascii_uppercase().starts_with("PATH="))
        );
        let keys: Vec<String> = entries
            .iter()
            .map(|e| e.split('=').next().unwrap().to_ascii_uppercase())
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert!(block.ends_with(&[0, 0]));
    }

    #[test]
    fn runs_a_program_on_its_own_console() {
        let mut command = Command::new("cmd.exe");
        command
            .args(["/d", "/c", "echo %HEARTH_PTY_TEST%& exit 7"])
            .env("HEARTH_PTY_TEST", "hello from conpty");
        let mut pty = PtyProcess::spawn(&command, Size { cols: 80, rows: 25 }).unwrap();
        let mut output = pty.take_output().unwrap();
        let reader = std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = output.read_to_end(&mut text);
            String::from_utf8_lossy(&text).into_owned()
        });
        pty.resize(Size {
            cols: 100,
            rows: 30,
        })
        .unwrap();
        assert_eq!(pty.wait(Some(Duration::from_secs(20))).unwrap(), Some(7));
        pty.close();
        let text = reader.join().unwrap();
        assert!(text.contains("hello from conpty"), "{text:?}");
    }
}
