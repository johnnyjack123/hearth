//! The user's Windows console in raw mode: keys arrive as VT sequences
//! (Ctrl+C included, it is the agent's to handle), output understands VT.

use std::io;
use std::time::Duration;

use harness_core::pty::Size;
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{
    CONSOLE_SCREEN_BUFFER_INFO, DISABLE_NEWLINE_AUTO_RETURN, ENABLE_PROCESSED_OUTPUT,
    ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, ENABLE_WINDOW_INPUT,
    GetConsoleMode, GetConsoleScreenBufferInfo, GetNumberOfConsoleInputEvents, GetStdHandle,
    INPUT_RECORD, KEY_EVENT, ReadConsoleInputW, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    SetConsoleMode,
};
use windows_sys::Win32::System::Threading::WaitForSingleObject;

/// Raw mode for as long as it lives; the previous modes come back on drop.
pub struct RawConsole {
    input: HANDLE,
    output: HANDLE,
    input_mode: u32,
    output_mode: u32,
    /// A high surrogate whose low half has not been read yet.
    pending: Option<u16>,
}

fn std_handle(which: u32) -> io::Result<HANDLE> {
    // SAFETY: GetStdHandle has no preconditions.
    let handle = unsafe { GetStdHandle(which) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::other(harness_core::lang::tr(
            "keine Konsole",
            "no console",
        )));
    }
    Ok(handle)
}

fn mode(handle: HANDLE) -> io::Result<u32> {
    let mut mode = 0;
    // SAFETY: valid handle and out pointer.
    if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(mode)
}

fn set_mode(handle: HANDLE, mode: u32) -> io::Result<()> {
    // SAFETY: valid handle.
    if unsafe { SetConsoleMode(handle, mode) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

impl RawConsole {
    /// Switches stdin and stdout to raw VT mode; fails if they are no console.
    pub fn enable() -> io::Result<RawConsole> {
        let input = std_handle(STD_INPUT_HANDLE)?;
        let output = std_handle(STD_OUTPUT_HANDLE)?;
        let input_mode = mode(input)?;
        let output_mode = mode(output)?;
        let console = RawConsole {
            input,
            output,
            input_mode,
            output_mode,
            pending: None,
        };
        // No line editing, echo or Ctrl+C handling: every key goes to the
        // agent. Quick-edit settings stay as they are, since
        // ENABLE_EXTENDED_FLAGS is not part of the new mode.
        set_mode(input, ENABLE_VIRTUAL_TERMINAL_INPUT | ENABLE_WINDOW_INPUT)?;
        set_mode(
            output,
            output_mode
                | ENABLE_PROCESSED_OUTPUT
                | ENABLE_VIRTUAL_TERMINAL_PROCESSING
                | DISABLE_NEWLINE_AUTO_RETURN,
        )?;
        Ok(console)
    }

    /// The visible window in character cells.
    pub fn size(&self) -> io::Result<Size> {
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = unsafe { std::mem::zeroed() };
        // SAFETY: valid handle and out pointer.
        if unsafe { GetConsoleScreenBufferInfo(self.output, &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let window = info.srWindow;
        Ok(Size {
            cols: (window.Right - window.Left + 1).max(1) as u16,
            rows: (window.Bottom - window.Top + 1).max(1) as u16,
        })
    }

    /// Typed text (VT sequences included), waiting at most `timeout`.
    pub fn read(&mut self, timeout: Duration) -> io::Result<String> {
        let millis = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        // SAFETY: valid handle.
        if unsafe { WaitForSingleObject(self.input, millis) } != WAIT_OBJECT_0 {
            return Ok(String::new());
        }
        let mut count = 0u32;
        // SAFETY: valid handle and out pointer.
        if unsafe { GetNumberOfConsoleInputEvents(self.input, &mut count) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if count == 0 {
            return Ok(String::new());
        }
        let mut records = vec![INPUT_RECORD::default(); count.min(256) as usize];
        let mut read = 0u32;
        // SAFETY: the buffer holds `records.len()` records; events are waiting,
        // so the call does not block.
        if unsafe {
            ReadConsoleInputW(
                self.input,
                records.as_mut_ptr(),
                records.len() as u32,
                &mut read,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut units: Vec<u16> = self.pending.take().into_iter().collect();
        for record in &records[..read as usize] {
            if u32::from(record.EventType) != KEY_EVENT {
                continue;
            }
            // SAFETY: EventType says this is a key event.
            let key = unsafe { record.Event.KeyEvent };
            // SAFETY: both union fields are plain integers.
            let unit = unsafe { key.uChar.UnicodeChar };
            if key.bKeyDown != 0 && unit != 0 {
                units.extend(std::iter::repeat_n(
                    unit,
                    usize::from(key.wRepeatCount.max(1)),
                ));
            }
        }
        if units.last().is_some_and(|u| (0xD800..0xDC00).contains(u)) {
            self.pending = units.pop();
        }
        Ok(String::from_utf16_lossy(&units))
    }
}

impl Drop for RawConsole {
    fn drop(&mut self) {
        let _ = set_mode(self.input, self.input_mode);
        let _ = set_mode(self.output, self.output_mode);
    }
}

// Nothing else touches these handles concurrently; they are process-wide.
unsafe impl Send for RawConsole {}
