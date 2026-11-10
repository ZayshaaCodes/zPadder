//! Windows implementation.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, HANDLE, WIN32_ERROR};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, InternalGetWindowText,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use crate::security::{OwnedHandle, io_error};
use crate::{Autostart, Elevation, ForegroundWindow, RunningProgram, ThreadPriority};

const RUN_KEY: PCWSTR = windows::core::w!(r"Software\Microsoft\Windows\CurrentVersion\Run");

/// Starts this program again as administrator with `args` (Windows shows a UAC prompt). The
/// caller should quit afterwards.
pub fn restart_elevated(args: &str) -> io::Result<()> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let exe = std::env::current_exe()?;
    // SAFETY: all strings are valid for the duration of the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("runas"),
            &HSTRING::from(exe.as_os_str()),
            &HSTRING::from(args),
            None,
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success with a value above 32.
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(io::Error::other("the administrator restart was cancelled or failed"))
    }
}

/// Opens a program, shortcut, document or URL file the way Explorer would, with the shell's
/// "open" verb, in the file's own folder. Nothing in the path is interpreted by a command
/// interpreter. Blocks until the shell has started it (or shown why it cannot).
pub fn shell_open(path: &Path) -> io::Result<()> {
    let path = path.to_owned();
    let folder = path.parent().filter(|dir| !dir.as_os_str().is_empty()).map(Path::to_owned);
    shell_open_on_thread(path.into_os_string(), folder)
}

/// Opens a web page in the default browser. Only `https://` addresses are accepted, so nothing
/// but a web page can be opened this way.
pub fn open_url(url: &str) -> io::Result<()> {
    if !is_web_address(url) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not an https:// address"));
    }
    shell_open_on_thread(OsString::from(url), None)
}

/// Whether `url` is a plain `https://` address: no spaces, quotes or control characters.
fn is_web_address(url: &str) -> bool {
    url.len() > "https://".len()
        && url.get(..8).is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"')
}

/// Runs the shell's "open" on a thread of its own, so the COM apartment the shell needs does not
/// affect the caller's.
fn shell_open_on_thread(file: OsString, folder: Option<PathBuf>) -> io::Result<()> {
    std::thread::Builder::new()
        .name("zpadder-launch".into())
        .spawn(move || shell_open_here(&file, folder.as_deref()))?
        .join()
        .unwrap_or_else(|_| Err(io::Error::other("the launch thread panicked")))
}

fn shell_open_here(file: &std::ffi::OsStr, folder: Option<&Path>) -> io::Result<()> {
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    // SAFETY: initialises COM on this thread, which the shell may use to resolve shortcuts. It is
    // undone below if it succeeded.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let file = HSTRING::from(file);
    let folder = folder.map(HSTRING::from);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        // Finish before returning: the caller may exit right after.
        fMask: SEE_MASK_NOASYNC,
        lpVerb: windows::core::w!("open"),
        lpFile: PCWSTR(file.as_ptr()),
        lpDirectory: folder.as_ref().map_or(PCWSTR::null(), |dir| PCWSTR(dir.as_ptr())),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` is a SHELLEXECUTEINFOW with `cbSize` set; the strings it points to are
    // NUL-terminated and outlive the call.
    let result = unsafe { ShellExecuteExW(&mut info) }.map_err(io_error);
    if com.is_ok() {
        // SAFETY: balances the successful CoInitializeEx above, on the same thread.
        unsafe { CoUninitialize() };
    }
    result
}

/// Shows an error message box titled "ZPadder" and waits for the user to dismiss it. For errors
/// that stop ZPadder before (or instead of) its window opening.
pub fn error_box(text: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{
        MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MessageBoxW,
    };
    // SAFETY: both strings are NUL-terminated and outlive the call; there is no owner window.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(text),
            windows::core::w!("ZPadder"),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        )
    };
}

/// Makes DLLs load from System32 only (plus folders added explicitly), never from the current
/// or the executable's folder, so a planted DLL cannot be picked up. Call first thing in `main`.
pub fn restrict_dll_search() -> io::Result<()> {
    use windows::Win32::System::LibraryLoader::{
        LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories,
    };
    // SAFETY: changes only the process's default DLL search path; no pointers are involved.
    unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) }.map_err(io_error)
}

/// Keeps Windows honouring this process's timer resolution request (SDL asks for 1 ms) while its
/// window is minimised or hidden in the tray. Windows 11 otherwise ignores it then, which would
/// make turbo and mouse movement run on a coarse 15.6 ms tick.
pub fn keep_timer_resolution() -> io::Result<()> {
    use windows::Win32::System::Threading::{
        PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling, SetProcessInformation,
    };
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        // Take control of this throttling, and switch it off.
        ControlMask: PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        StateMask: 0,
    };
    // SAFETY: `state` is a PROCESS_POWER_THROTTLING_STATE of the size passed, as
    // ProcessPowerThrottling requires; the process pseudo-handle needs no closing.
    unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            (&raw const state).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    }
    .map_err(io_error)
}

/// Sets the calling thread's priority.
pub fn set_thread_priority(priority: ThreadPriority) -> io::Result<()> {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL,
        THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_NORMAL,
    };
    let value = match priority {
        ThreadPriority::Low => THREAD_PRIORITY_BELOW_NORMAL,
        ThreadPriority::Normal => THREAD_PRIORITY_NORMAL,
        ThreadPriority::AboveNormal => THREAD_PRIORITY_ABOVE_NORMAL,
    };
    // SAFETY: GetCurrentThread returns a pseudo-handle for the calling thread that needs no
    // closing.
    unsafe { SetThreadPriority(GetCurrentThread(), value) }.map_err(io_error)
}

/// Name of ZPadder's value under the Run key. A copy with its own `ZPADDER_INSTANCE` gets its own
/// value, so a development copy never touches the real entry.
fn run_value_name() -> HSTRING {
    HSTRING::from(format!("ZPadder{}", crate::instance::instance_suffix()))
}

/// Whether ZPadder starts with Windows, and whether the entry starts this copy.
pub fn autostart_status() -> Autostart {
    let command = match read_run_value(&run_value_name()) {
        Ok(Some(command)) => command,
        Ok(None) => return Autostart::Off,
        // There is a value, but not one ZPadder wrote (say, of another type).
        Err(_) => return Autostart::Elsewhere { command: String::new() },
    };
    let program = PathBuf::from(OsString::from_wide(command_program(&command)));
    match std::env::current_exe() {
        Ok(exe) if same_file(&program, &exe) => Autostart::On,
        _ => Autostart::Elsewhere { command: String::from_utf16_lossy(&command) },
    }
}

/// Whether ZPadder starts with Windows, as this copy. An entry that starts another copy counts as
/// off; see [`autostart_points_elsewhere`].
pub fn autostart_enabled() -> bool {
    autostart_status() == Autostart::On
}

/// Whether the autostart entry starts something other than this copy, typically a copy that has
/// since been moved. Turning autostart on repairs it.
pub fn autostart_points_elsewhere() -> bool {
    matches!(autostart_status(), Autostart::Elsewhere { .. })
}

/// Makes this copy of ZPadder start (minimized to the tray) with Windows, or stops ZPadder doing
/// so.
pub fn set_autostart(enabled: bool) -> io::Result<()> {
    use windows::Win32::System::Registry::{REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW};
    let name = run_value_name();
    if enabled {
        let command = run_command(&std::env::current_exe()?);
        // SAFETY: `command` is a NUL-terminated UTF-16 string of the given byte length.
        let result = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                &name,
                REG_SZ.0,
                Some(command.as_ptr().cast()),
                (command.len() * 2) as u32,
            )
        };
        win32(result)
    } else {
        // SAFETY: deletes a named value; no buffers involved.
        let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, &name) };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        win32(result)
    }
}

/// The Run command for `exe`: its path in quotes (Windows paths cannot contain quotes), then
/// `/M`, NUL-terminated. Built from the path's UTF-16 directly, so no path is mangled.
fn run_command(exe: &Path) -> Vec<u16> {
    let mut command: Vec<u16> = "\"".encode_utf16().collect();
    command.extend(exe.as_os_str().encode_wide());
    command.extend("\" /M".encode_utf16());
    command.push(0);
    command
}

/// The program a Run command starts: the part in quotes, or else everything before the first
/// space.
fn command_program(command: &[u16]) -> &[u16] {
    const QUOTE: u16 = b'"' as u16;
    const SPACE: u16 = b' ' as u16;
    let start = command.iter().position(|&c| c != SPACE).unwrap_or(command.len());
    let command = &command[start..];
    match command.split_first() {
        Some((&QUOTE, rest)) => rest.split(|&c| c == QUOTE).next().unwrap_or(rest),
        _ => command.split(|&c| c == SPACE).next().unwrap_or(command),
    }
}

/// Whether two paths name the same file, comparing them the way the file system does (resolved
/// and case-insensitive). A path that does not exist is compared as written.
fn same_file(a: &Path, b: &Path) -> bool {
    use windows::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
    let resolve = |path: &Path| -> Vec<u16> {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_owned())
            .as_os_str()
            .encode_wide()
            .collect()
    };
    let (a, b) = (resolve(a), resolve(b));
    // SAFETY: both slices are valid for their lengths; no NUL terminator is needed.
    let order = unsafe { CompareStringOrdinal(&a, &b, true) };
    order == CSTR_EQUAL
}

/// Reads a string value under the Run key, without its terminating NUL. `None` if there is none.
fn read_run_value(name: &HSTRING) -> io::Result<Option<Vec<u16>>> {
    // The value can change between asking for its size and reading it; try again then.
    for _ in 0..3 {
        let mut size = 0;
        // SAFETY: asks only for the size; no buffer is passed.
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                name,
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut size),
            )
        };
        if result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        win32(result)?;
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: `buffer` is writable for `size` bytes, which RegGetValueW never exceeds.
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                name,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        match result {
            ERROR_MORE_DATA => continue,
            ERROR_FILE_NOT_FOUND => return Ok(None),
            result => win32(result)?,
        }
        buffer.truncate(size as usize / 2);
        while buffer.last() == Some(&0) {
            buffer.pop();
        }
        return Ok(Some(buffer));
    }
    Err(io::Error::other("the autostart entry keeps changing"))
}

fn win32(result: WIN32_ERROR) -> io::Result<()> {
    if result.is_ok() { Ok(()) } else { Err(io::Error::from_raw_os_error(result.0 as i32)) }
}

/// Identifier of the foreground window. Cheap enough to call on every input tick.
pub fn foreground_window_id() -> Option<u64> {
    // SAFETY: no preconditions.
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.is_invalid()).then_some(hwnd.0 as usize as u64)
}

/// Details of the foreground window, including its process and elevation.
pub fn foreground_window() -> Option<ForegroundWindow> {
    // SAFETY: no preconditions.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return None;
    }
    let mut pid = 0;
    // SAFETY: `pid` is a valid out-pointer.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let mut title = [0u16; 512];
    // GetWindowTextW sends WM_GETTEXT to a window of this process and waits for its thread to
    // answer, so reading ZPadder's own title would hold up the caller (the input thread) behind a
    // busy UI thread. InternalGetWindowText reads the stored text without asking.
    let len = if pid == std::process::id() {
        // SAFETY: `title` is a writable buffer; the text is cut to its length.
        unsafe { InternalGetWindowText(hwnd, &mut title) }
    } else {
        // SAFETY: `title` is a writable buffer; GetWindowTextW never writes past its length.
        unsafe { GetWindowTextW(hwnd, &mut title) }
    };

    // SAFETY: OpenProcess has no memory-safety preconditions; failure is handled.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok();
    let (exe, elevation) = match process.map(OwnedHandle) {
        Some(process) => (image_path(process.0), token_elevation(process.0)),
        None => (None, Elevation::Unknown),
    };

    Some(ForegroundWindow {
        id: hwnd.0 as usize as u64,
        title: String::from_utf16_lossy(&title[..usize::try_from(len).unwrap_or(0)]),
        pid,
        exe,
        elevation,
    })
}

/// Every running process, by executable file name, e.g. `steam.exe`. Processes ZPadder cannot
/// open are included: the snapshot needs no access to them.
pub fn running_programs() -> io::Result<Vec<RunningProgram>> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    // SAFETY: takes a process snapshot, owned by the guard below; failure is handled.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.map_err(io_error)?;
    let snapshot = OwnedHandle(snapshot);
    let mut entry =
        PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut programs = Vec::new();
    // SAFETY: `entry` is a PROCESSENTRY32W with `dwSize` set, as Process32FirstW requires. The
    // walk ends with an error once there are no more processes.
    let mut found = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while found.is_ok() {
        let name = &entry.szExeFile;
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        programs.push(RunningProgram {
            pid: entry.th32ProcessID,
            exe: String::from_utf16_lossy(&name[..len]),
        });
        // SAFETY: as above.
        found = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    Ok(programs)
}

/// Whether this process runs elevated.
pub fn current_process_elevation() -> Elevation {
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing.
    token_elevation(unsafe { GetCurrentProcess() })
}

fn image_path(process: HANDLE) -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` u16s and QueryFullProcessImageNameW writes at most that many.
    unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
    }
    .ok()?;
    Some(OsString::from_wide(&buf[..len as usize]).into())
}

fn token_elevation(process: HANDLE) -> Elevation {
    let mut token = HANDLE::default();
    // SAFETY: `token` is a valid out-pointer; failure is handled.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.is_err() {
        return Elevation::Unknown;
    }
    let token = OwnedHandle(token);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut len = 0;
    // SAFETY: the buffer is a TOKEN_ELEVATION of the size passed in.
    let read = unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
    };
    match (read.is_ok(), elevation.TokenIsElevated != 0) {
        (false, _) => Elevation::Unknown,
        (true, true) => Elevation::Elevated,
        (true, false) => Elevation::NotElevated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn running_programs_include_this_one() {
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_name().unwrap().to_string_lossy();
        let programs = running_programs().unwrap();
        let this = programs.iter().find(|p| p.pid == std::process::id());
        assert!(this.is_some_and(|p| p.exe.eq_ignore_ascii_case(&name)), "{name} in {programs:?}");
    }

    #[test]
    fn the_run_command_quotes_the_path() {
        let command = run_command(Path::new(r"C:\Program Files\Z & Co\zpadder.exe"));
        assert_eq!(command.last(), Some(&0));
        let text = String::from_utf16(&command[..command.len() - 1]).unwrap();
        assert_eq!(text, r#""C:\Program Files\Z & Co\zpadder.exe" /M"#);
        assert_eq!(
            command_program(&command[..command.len() - 1]),
            wide(r"C:\Program Files\Z & Co\zpadder.exe")
        );
    }

    #[test]
    fn the_program_is_found_in_quoted_and_bare_commands() {
        assert_eq!(command_program(&wide(r#"  "D:\a b\z.exe" /M"#)), wide(r"D:\a b\z.exe"));
        assert_eq!(command_program(&wide(r"D:\z.exe /M")), wide(r"D:\z.exe"));
        assert_eq!(command_program(&wide(r#""D:\unterminated"#)), wide(r"D:\unterminated"));
        assert_eq!(command_program(&wide("")), wide(""));
    }

    #[test]
    fn only_plain_https_addresses_open() {
        assert!(is_web_address("https://github.com/nefarius/ViGEmBus/releases"));
        assert!(is_web_address("HTTPS://example.com"));
        assert!(!is_web_address("https://"));
        assert!(!is_web_address("http://example.com"));
        assert!(!is_web_address(r"C:\Windows\System32\cmd.exe"));
        assert!(!is_web_address("file:///C:/x"));
        assert!(!is_web_address("https://example.com\" calc"));
        assert!(!is_web_address("https://example.com/a b"));
        assert!(open_url("calc.exe").is_err());
    }

    #[test]
    fn the_same_file_matches_in_any_case() {
        let exe = std::env::current_exe().unwrap();
        let shouted = PathBuf::from(exe.to_string_lossy().to_uppercase());
        assert!(same_file(&exe, &shouted));
        assert!(!same_file(&exe, &exe.with_file_name("not-zpadder.exe")));
    }
}
