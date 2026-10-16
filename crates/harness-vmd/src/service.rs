//! Talking to the Windows service control manager: register, report
//! "running", and end the process on stop or shutdown.

use std::ffi::c_void;
use std::sync::OnceLock;

use harness_core::vmd::SERVICE_NAME;
use windows_sys::Win32::Foundation::{ERROR_CALL_NOT_IMPLEMENTED, NO_ERROR};
use windows_sys::Win32::System::Services::{
    RegisterServiceCtrlHandlerExW, SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP,
    SERVICE_CONTROL_INTERROGATE, SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_RUNNING,
    SERVICE_START_PENDING, SERVICE_STATUS, SERVICE_STATUS_CURRENT_STATE, SERVICE_STATUS_HANDLE,
    SERVICE_STOPPED, SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS, SetServiceStatus,
    StartServiceCtrlDispatcherW,
};
use windows_sys::core::PWSTR;

use crate::log;

/// The status handle, stored as an address because raw pointers are not
/// `Sync`; it is only ever passed back to `SetServiceStatus`.
static HANDLE: OnceLock<usize> = OnceLock::new();

/// What the service does once registered; returns only on failure.
static BODY: OnceLock<fn() -> Result<(), String>> = OnceLock::new();

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn report(state: SERVICE_STATUS_CURRENT_STATE, exit_code: u32) {
    let Some(&handle) = HANDLE.get() else {
        return;
    };
    let accepted = if state == SERVICE_RUNNING {
        SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
    } else {
        0
    };
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: accepted,
        dwWin32ExitCode: exit_code,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: if state == SERVICE_START_PENDING {
            10_000
        } else {
            0
        },
    };
    // SAFETY: the handle came from RegisterServiceCtrlHandlerExW.
    unsafe { SetServiceStatus(handle as SERVICE_STATUS_HANDLE, &status) };
}

unsafe extern "system" fn control(
    control: u32,
    _event_type: u32,
    _event_data: *mut c_void,
    _context: *mut c_void,
) -> u32 {
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            // The VM is left alone: Hyper-V shuts it down with the host
            // (AutomaticStopAction), and a service restart must not stop
            // the user's work.
            log::line("Dienst wird beendet");
            report(SERVICE_STOPPED, NO_ERROR);
            std::process::exit(0);
        }
        SERVICE_CONTROL_INTERROGATE => NO_ERROR,
        _ => ERROR_CALL_NOT_IMPLEMENTED,
    }
}

unsafe extern "system" fn service_main(_argc: u32, _argv: *mut PWSTR) {
    let name = wide(SERVICE_NAME);
    // SAFETY: valid name; `control` lives for the whole process.
    let handle =
        unsafe { RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(control), std::ptr::null()) };
    if handle.is_null() {
        return;
    }
    let _ = HANDLE.set(handle as usize);
    report(SERVICE_START_PENDING, NO_ERROR);
    report(SERVICE_RUNNING, NO_ERROR);
    let body = BODY.get().copied();
    let result = body.map_or(Err("kein Dienstprogramm".to_owned()), |body| body());
    if let Err(error) = result {
        log::line(&format!("Dienst beendet sich mit Fehler: {error}"));
        // ERROR_SERVICE_SPECIFIC_ERROR would need a code; 1 is enough for
        // the service manager's restart policy.
        report(SERVICE_STOPPED, 1);
    }
}

/// Hands the process to the service control manager, which then runs
/// `body` on its own thread. Fails when not started as a service.
pub fn run(body: fn() -> Result<(), String>) -> Result<(), String> {
    let _ = BODY.set(body);
    let mut name = wide(SERVICE_NAME);
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_mut_ptr(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: std::ptr::null_mut(),
            lpServiceProc: None,
        },
    ];
    // SAFETY: a NULL-terminated table whose strings outlive the call, which
    // returns when the service stops.
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        return Err(format!(
            "nicht als Dienst gestartet ({}). Zum Testen: harness-vmd console",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
