use std::process::ExitCode;
use std::sync::Mutex;

use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyNameTextW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
    LLKHF_INJECTED, MSG, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_SYSKEYDOWN,
};
use zpadder_output::EXTRA_INFO_TAG;

use crate::log::Log;

/// The hook procedure has no user-data pointer, so the log lives here.
static LOG: Mutex<Option<Log>> = Mutex::new(None);

fn log(msg: impl std::fmt::Display) {
    if let Some(log) = LOG.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        log.line(msg);
    }
}

pub(crate) fn run() -> ExitCode {
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(Log::create("keyhook"));
    log("Logging every key event Windows receives, from any window. Ctrl+C stops.");

    // SAFETY: a null module name returns this executable's handle.
    let module = match unsafe { GetModuleHandleW(None) } {
        Ok(module) => module,
        Err(e) => {
            log(format_args!("GetModuleHandleW failed: {e}"));
            return ExitCode::FAILURE;
        }
    };
    // SAFETY: `keyboard_proc` matches HOOKPROC and lives for the whole program.
    let hook = match unsafe {
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(HINSTANCE(module.0)), 0)
    } {
        Ok(hook) => hook,
        Err(e) => {
            log(format_args!("SetWindowsHookExW failed: {e}"));
            return ExitCode::FAILURE;
        }
    };

    // Low-level hooks are called on the installing thread, which must pump messages.
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid out-pointer.
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        // SAFETY: `msg` was filled in by GetMessageW.
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // SAFETY: `hook` was returned by SetWindowsHookExW and is unhooked once.
    let _ = unsafe { UnhookWindowsHookEx(hook) };
    ExitCode::SUCCESS
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let message = wparam.0 as u32;
        let action = if message == WM_KEYDOWN || message == WM_SYSKEYDOWN { "down" } else { "up" };
        let extended = info.flags.0 & LLKHF_EXTENDED.0 != 0;
        let source = if info.flags.0 & LLKHF_INJECTED.0 == 0 {
            "hardware"
        } else if info.dwExtraInfo == EXTRA_INFO_TAG {
            "ZPadder"
        } else {
            "another program"
        };
        log(format_args!(
            "{action:<4} vk={:#04x} scan={:#04x}{} {:<16} from {source}",
            info.vkCode,
            info.scanCode,
            if extended { " ext" } else { "    " },
            key_name(info.scanCode, extended),
        ));
    }
    // SAFETY: passes the hook arguments through unchanged.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// The key's name in the active keyboard layout, e.g. "Num Enter".
fn key_name(scan: u32, extended: bool) -> String {
    let lparam = (scan << 16) | (u32::from(extended) << 24);
    let mut buf = [0u16; 64];
    // SAFETY: `buf` is a writable buffer; GetKeyNameTextW never writes past its length.
    let len = unsafe { GetKeyNameTextW(lparam as i32, &mut buf) };
    String::from_utf16_lossy(&buf[..usize::try_from(len).unwrap_or(0)])
}
