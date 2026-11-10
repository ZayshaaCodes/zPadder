//! Whether ZPadder's input can reach the user's desktop. It cannot while Windows is locked or
//! shows a secure screen (a UAC prompt, the Ctrl+Alt+Del screen), and it should not be holding
//! keys down while the computer goes to sleep.

use std::ffi::c_void;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS, HANDLE};
use windows::Win32::System::Power::{
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, PowerRegisterSuspendResumeNotification,
};
use windows::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK, WTSFreeMemory,
    WTSINFOEXW, WTSQuerySessionInformationW, WTSSessionInfoEx,
};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, GetUserObjectInformationW,
    OpenInputDesktop, UOI_NAME,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND,
};
use windows::core::PWSTR;

use crate::security::is;

/// Whether Windows is locked or showing a secure screen, so input sent now would not reach the
/// user's programs. Cheap enough to call a few times a second.
pub fn desktop_locked() -> bool {
    !input_desktop_is_default() || session_locked()
}

/// Whether the desktop receiving input is the user's ("Default"), rather than the secure one
/// ("Winlogon") that the lock screen, UAC prompts and the Ctrl+Alt+Del screen use.
fn input_desktop_is_default() -> bool {
    // SAFETY: opens the desktop that receives input; the handle is closed below.
    let desktop =
        match unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) } {
            Ok(desktop) => desktop,
            // The secure desktop is closed to the user's programs.
            Err(e) if is(&e, ERROR_ACCESS_DENIED) => return false,
            // Cannot tell; assume all is well rather than stop mapping for no reason.
            Err(_) => return true,
        };
    let mut name = [0u16; 256];
    // SAFETY: `name` is writable for the byte length passed; the desktop handle is open.
    let read = unsafe {
        GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr().cast()),
            size_of_val(&name) as u32,
            None,
        )
    };
    // SAFETY: the handle came from OpenInputDesktop and is closed exactly once.
    let _ = unsafe { CloseDesktop(desktop) };
    if read.is_err() {
        return true;
    }
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    String::from_utf16_lossy(&name[..len]).eq_ignore_ascii_case("Default")
}

/// Whether this session is locked, as Remote Desktop Services sees it.
fn session_locked() -> bool {
    let mut buffer = PWSTR::null();
    let mut bytes = 0;
    // SAFETY: Windows allocates the buffer and reports its size in bytes; it is freed below.
    let queried = unsafe {
        WTSQuerySessionInformationW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut bytes,
        )
    };
    if queried.is_err() || buffer.is_null() {
        return false;
    }
    let locked = if bytes as usize >= size_of::<WTSINFOEXW>() {
        // SAFETY: for WTSSessionInfoEx the buffer holds a WTSINFOEXW, and it is at least that
        // large. It is read unaligned, which is always sound for plain data.
        let info = unsafe { buffer.0.cast::<WTSINFOEXW>().read_unaligned() };
        // SAFETY: level 1 is the union's only member.
        let flags = unsafe { info.Data.WTSInfoExLevel1.SessionFlags };
        info.Level == 1 && flags == WTS_SESSIONSTATE_LOCK as i32
    } else {
        false
    };
    // SAFETY: frees the buffer WTSQuerySessionInformationW allocated, exactly once.
    unsafe { WTSFreeMemory(buffer.0.cast()) };
    locked
}

/// Set from Windows' suspend notification until the system resumes.
static SUSPENDING: AtomicBool = AtomicBool::new(false);
/// Set once [`system_suspending`] has told a caller about the suspend.
static SEEN: AtomicBool = AtomicBool::new(false);
static WATCH: Once = Once::new();
/// How long a suspend waits for the mapper to notice it...
const SUSPEND_NOTICE: Duration = Duration::from_millis(300);
/// ...and then for it to let go of everything.
const SUSPEND_RELEASE: Duration = Duration::from_millis(50);

/// Whether the computer is going to sleep (or hibernate) and has not resumed yet. The first call
/// starts watching; from then on a suspend waits briefly (a few hundred milliseconds at most)
/// for a caller to notice, so the mapper can let go of every key before the system sleeps.
pub fn system_suspending() -> bool {
    WATCH.call_once(watch_suspend);
    let suspending = SUSPENDING.load(Ordering::SeqCst);
    if suspending {
        SEEN.store(true, Ordering::SeqCst);
    }
    suspending
}

fn watch_suspend() {
    // Windows keeps the pointer for as long as the registration lasts, which is the rest of the
    // process, so the parameters are leaked.
    let parameters = Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
        Callback: Some(on_power_event),
        Context: std::ptr::null_mut(),
    }));
    let mut registration = std::ptr::null_mut();
    // SAFETY: with DEVICE_NOTIFY_CALLBACK the recipient is a DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS,
    // which lives for the rest of the process (above); `registration` is a valid out-pointer.
    let result = unsafe {
        PowerRegisterSuspendResumeNotification(
            DEVICE_NOTIFY_CALLBACK,
            HANDLE((&raw mut *parameters).cast()),
            &mut registration,
        )
    };
    if result != ERROR_SUCCESS {
        tracing::debug!(
            "Cannot watch for sleep and resume: {}",
            std::io::Error::from_raw_os_error(result.0 as i32)
        );
    }
}

/// Called by Windows, on a thread of its own, when the system suspends or resumes.
unsafe extern "system" fn on_power_event(
    _context: *const c_void,
    kind: u32,
    _setting: *const c_void,
) -> u32 {
    match kind {
        PBT_APMSUSPEND => {
            SEEN.store(false, Ordering::SeqCst);
            SUSPENDING.store(true, Ordering::SeqCst);
            let deadline = Instant::now() + SUSPEND_NOTICE;
            while !SEEN.load(Ordering::SeqCst) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if SEEN.load(Ordering::SeqCst) {
                std::thread::sleep(SUSPEND_RELEASE);
            }
        }
        PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => SUSPENDING.store(false, Ordering::SeqCst),
        _ => {}
    }
    ERROR_SUCCESS.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checks_run() {
        // Whatever the answers, the calls must work: the tests may run on a locked machine.
        let _ = desktop_locked();
        assert!(!system_suspending());
    }
}
