//! The tray icon of `harnessd` (ROADMAP Phase 3): shows whether the
//! service is connected and what the VM does, opens the web app and the
//! log, stops the VM, ends the service, and points at what waits for the
//! user (questions of a gateway, merges held for approval).
//!
//! Plain Win32 on the main thread; the service runs as a child process
//! that a second thread watches. The tray reads what the service writes to
//! `daemon-status.json` and the stored merge requests; it asks the service
//! for something only through named events, like `harness daemon stop`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, EVENT_MODIFY_STATE, OpenEventW, SetEvent,
};
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONUSERCLICK, NIN_SELECT, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, Shell_NotifyIconW, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
    DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, HICON, HMENU,
    MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SM_CXSMICON, SW_SHOWNORMAL, SetForegroundWindow, SetTimer,
    TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP,
    WM_CONTEXTMENU, WM_DESTROY, WM_NULL, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};

use crate::icon::{self, AMBER, BRAND, GREY, Look};
use crate::menu::{Action, Entry, Language, View};
use crate::{object_suffix, supervise};

/// Messages of the tray icon.
const WM_TRAY: u32 = WM_APP + 1;
/// The service has ended for good.
const WM_SERVICE_ENDED: u32 = WM_APP + 2;
const TIMER_ID: usize = 1;
/// The status file is read this often.
const REFRESH_MS: u32 = 2_000;
/// Clipboard format for UTF-16 text (`CF_UNICODETEXT`).
const CF_UNICODETEXT: u32 = 13;
/// The icon chosen with the keyboard (`NIN_SELECT | NINF_KEY`).
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;

/// `TaskbarCreated`: Explorer restarted and lost all icons.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain([0]).collect()
}

/// Copies `text` into a fixed field of a Win32 struct, cut and terminated.
fn fill(field: &mut [u16], text: &str) {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.truncate(field.len().saturating_sub(1));
    field[..units.len()].copy_from_slice(&units);
    field[units.len()] = 0;
}

fn signal(name: &str) -> bool {
    let name = wide(name);
    // SAFETY: a named event handle, closed right after use.
    unsafe {
        let handle = OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr());
        if handle.is_null() {
            return false;
        }
        let ok = SetEvent(handle) != 0;
        CloseHandle(handle);
        ok
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

struct Tray {
    hwnd: HWND,
    root: PathBuf,
    suffix: String,
    pid: Arc<AtomicU32>,
    quit: Arc<AtomicBool>,
    language: Language,
    /// Connected, not connected, and both with a dot that asks for attention.
    icons: [HICON; 4],
    /// Notifications already shown.
    announced: HashSet<String>,
    /// Where a click on the last notification leads.
    notice_target: Option<String>,
    /// Actions of the open menu; command id n is index n − 1.
    actions: Vec<Action>,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

/// Runs `f` on the tray unless it is busy (a nested message during a call
/// that pumps messages); then the message is simply skipped.
fn with_tray<T>(f: impl FnOnce(&mut Tray) -> T) -> Option<T> {
    TRAY.with(|cell| cell.try_borrow_mut().ok()?.as_mut().map(f))
}

impl Tray {
    fn data(&self) -> NOTIFYICONDATAW {
        // SAFETY: a plain C struct; zero is a valid start for every field.
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = 1;
        data
    }

    fn view(&self) -> View {
        let mut view = View::read(&self.root, self.pid.load(Ordering::SeqCst));
        view.quitting = self.quit.load(Ordering::SeqCst);
        view
    }

    fn add(&self) {
        let mut data = self.data();
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = self.icons[1];
        fill(
            &mut data.szTip,
            &self.language.tooltip(&View::default(), now()),
        );
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        // SAFETY: valid data for this window's icon.
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &data);
            Shell_NotifyIconW(NIM_SETVERSION, &data);
        }
    }

    fn remove(&self) {
        let data = self.data();
        // SAFETY: as in `add`.
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &data);
        }
    }

    fn refresh(&mut self) {
        let view = self.view();
        let icon = match (view.connected(), view.attention()) {
            (true, false) => self.icons[0],
            (false, false) => self.icons[1],
            (true, true) => self.icons[2],
            (false, true) => self.icons[3],
        };
        let mut data = self.data();
        data.uFlags = NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        data.hIcon = icon;
        fill(&mut data.szTip, &self.language.tooltip(&view, now()));
        // SAFETY: as in `add`.
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &data);
        }
        for notice in self.language.notices(&view) {
            if self.announced.insert(notice.key.clone()) {
                self.notify(&notice.text, notice.warning, notice.target);
            }
        }
    }

    fn notify(&mut self, text: &str, warning: bool, target: Option<String>) {
        // Test runs (scripts/smoke-connect.sh) start their own tray; its
        // questions and held merges would pop up on the user's desktop.
        if std::env::var_os("HARNESS_TRAY_QUIET").is_some() {
            return;
        }
        let mut data = self.data();
        data.uFlags = NIF_INFO;
        fill(&mut data.szInfoTitle, "LLM Harness");
        fill(&mut data.szInfo, text);
        data.dwInfoFlags = if warning { NIIF_WARNING } else { NIIF_INFO };
        // SAFETY: as in `add`.
        unsafe {
            Shell_NotifyIconW(NIM_MODIFY, &data);
        }
        self.notice_target = target;
    }

    /// The menu for the current view; its actions are kept by command id.
    fn build_menu(&mut self) -> Option<HMENU> {
        let entries = self
            .language
            .menu(&self.view(), &self.root.join("daemon.log"), now());
        self.actions.clear();
        // SAFETY: a fresh menu, filled with copies of the strings.
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return None;
            }
            for entry in entries {
                match entry {
                    Entry::Separator => {
                        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                    }
                    Entry::Item(label, None) => {
                        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide(&label).as_ptr());
                    }
                    Entry::Item(label, Some(action)) => {
                        self.actions.push(action);
                        AppendMenuW(menu, MF_STRING, self.actions.len(), wide(&label).as_ptr());
                    }
                }
            }
            Some(menu)
        }
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Open(target) => open(self.hwnd, &target),
            Action::Copy(text) => copy(self.hwnd, &text),
            Action::StopVm => {
                signal(&format!(r"Local\llm-harness-daemon-vmstop-{}", self.suffix));
            }
            Action::Quit => {
                self.quit.store(true, Ordering::SeqCst);
                signal(&format!(r"Local\llm-harness-daemon-stop-{}", self.suffix));
                self.refresh();
            }
        }
    }
}

fn open(hwnd: HWND, target: &str) {
    let verb = wide("open");
    let target = wide(target);
    // SAFETY: valid strings; Windows picks the program for the URL or file.
    unsafe {
        ShellExecuteW(
            hwnd,
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

fn copy(hwnd: HWND, text: &str) {
    let units = wide(text);
    let bytes = units.len() * 2;
    // SAFETY: the memory is written within its size; the clipboard owns it
    // once SetClipboardData succeeded.
    unsafe {
        if OpenClipboard(hwnd) == 0 {
            return;
        }
        EmptyClipboard();
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if !memory.is_null() {
            let target = GlobalLock(memory);
            if !target.is_null() {
                std::ptr::copy_nonoverlapping(units.as_ptr().cast::<u8>(), target.cast(), bytes);
                GlobalUnlock(memory);
                SetClipboardData(CF_UNICODETEXT, memory);
            }
        }
        CloseClipboard();
    }
}

/// Shows the menu at the pointer and runs what the user picks. The tray is
/// not borrowed while the menu is open: the menu runs its own message loop.
fn show_menu(hwnd: HWND) {
    let Some(Some(menu)) = with_tray(Tray::build_menu) else {
        return;
    };
    // SAFETY: the menu belongs to this thread; SetForegroundWindow before
    // and WM_NULL after make it close when the user clicks elsewhere.
    let chosen = unsafe {
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        chosen
    };
    let Ok(id) = usize::try_from(chosen) else {
        return;
    };
    with_tray(|tray| {
        if let Some(action) = id.checked_sub(1).and_then(|i| tray.actions.get(i)).cloned() {
            tray.run(action);
        }
    });
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAY => {
            // NOTIFYICON_VERSION_4: the event is in the low word.
            match (lparam & 0xffff) as u32 {
                WM_CONTEXTMENU | NIN_SELECT | NIN_KEYSELECT => show_menu(hwnd),
                NIN_BALLOONUSERCLICK => {
                    if let Some(Some(target)) = with_tray(|t| t.notice_target.clone()) {
                        open(hwnd, &target);
                    }
                }
                _ => {}
            }
            0
        }
        WM_TIMER => {
            with_tray(Tray::refresh);
            0
        }
        WM_SERVICE_ENDED => {
            with_tray(|tray| tray.remove());
            // SAFETY: our own window.
            unsafe {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            0
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::SeqCst) => {
            with_tray(|tray| {
                tray.add();
                tray.refresh();
            });
            0
        }
        // SAFETY: default handling of everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// A hidden window for the icon's messages; a message-only window would
/// miss the TaskbarCreated broadcast.
fn create_window() -> HWND {
    // SAFETY: class name and window procedure live for the whole program.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide("llm-harness-tray");
        let mut window_class: WNDCLASSW = std::mem::zeroed();
        window_class.lpfnWndProc = Some(window_proc);
        window_class.hInstance = instance;
        window_class.lpszClassName = class.as_ptr();
        RegisterClassW(&window_class);
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            Ordering::SeqCst,
        );
        CreateWindowExW(
            0,
            class.as_ptr(),
            wide("LLM Harness").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    }
}

/// Runs the service with a tray icon until it ends or the user quits.
pub fn run(harness: &Path, root: &Path) {
    let suffix = object_suffix(root);
    let name = wide(&format!(r"Local\llm-harness-tray-{suffix}"));
    // SAFETY: the mutex stays until the process ends; a second launcher
    // for the same configuration has nothing to do.
    unsafe {
        let mutex = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
        if mutex.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
        // Sharp icons on scaled displays.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let pid = Arc::new(AtomicU32::new(0));
    let quit = Arc::new(AtomicBool::new(false));
    let hwnd = create_window();
    if hwnd.is_null() {
        // Without a window the service still runs, only without an icon.
        supervise(harness, root, &quit, &pid);
        return;
    }
    // SAFETY: system metrics and the UI language.
    let (size, german) = unsafe {
        (
            GetSystemMetrics(SM_CXSMICON),
            GetUserDefaultUILanguage() & 0x3ff == 0x07,
        )
    };
    let look = |fill, dot| icon::create(size, Look { fill, dot });
    let tray = Tray {
        hwnd,
        root: root.to_owned(),
        suffix,
        pid: Arc::clone(&pid),
        quit: Arc::clone(&quit),
        language: Language { german },
        icons: [
            look(BRAND, None),
            look(GREY, None),
            look(BRAND, Some(AMBER)),
            look(GREY, Some(AMBER)),
        ],
        announced: HashSet::new(),
        notice_target: None,
        actions: Vec::new(),
    };
    tray.add();
    TRAY.with(|cell| *cell.borrow_mut() = Some(tray));

    let watcher = {
        let harness = harness.to_owned();
        let root = root.to_owned();
        // A window handle is a pointer; posting to it works from any thread.
        let window = hwnd as usize;
        std::thread::spawn(move || {
            supervise(&harness, &root, &quit, &pid);
            // SAFETY: posting to a window of this process.
            unsafe {
                PostMessageW(window as HWND, WM_SERVICE_ENDED, 0, 0);
            }
        })
    };

    // SAFETY: the standard message loop of this thread.
    unsafe {
        SetTimer(hwnd, TIMER_ID, REFRESH_MS, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    let _ = watcher.join();
    TRAY.with(|cell| {
        if let Some(tray) = cell.borrow_mut().take() {
            for icon in tray.icons {
                // SAFETY: icons created above and no longer shown.
                unsafe {
                    DestroyIcon(icon);
                }
            }
        }
    });
}
