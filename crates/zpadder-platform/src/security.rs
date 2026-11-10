//! Handles, the current user's SID and session, and security descriptors for ZPadder's own named
//! objects.

use std::io;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, HLOCAL, LocalFree, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HSTRING, PWSTR};

/// A kernel handle, closed when dropped.
#[derive(Debug)]
pub(crate) struct OwnedHandle(pub(crate) HANDLE);

// SAFETY: a kernel handle is a process-wide value that any thread may use, and this is its only
// owner, so moving it to another thread moves the responsibility for closing it too.
unsafe impl Send for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the handle is owned and closed exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// A handle borrowed by another thread.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SendHandle(pub(crate) HANDLE);

// SAFETY: a kernel handle is a process-wide value that any thread may use. Whoever hands one to
// another thread keeps it open until that thread is done with it.
unsafe impl Send for SendHandle {}

/// Turns a Windows error into an `io::Error` carrying the plain Win32 code, so that
/// `io::ErrorKind` and `raw_os_error` work as they do for std's own errors.
pub(crate) fn io_error(error: windows::core::Error) -> io::Error {
    let code = error.code().0 as u32;
    // HRESULT_FROM_WIN32: severity 1, facility 7, the Win32 code below.
    if code & 0xFFFF_0000 == 0x8007_0000 {
        io::Error::from_raw_os_error((code & 0xFFFF) as i32)
    } else {
        io::Error::other(error)
    }
}

/// Whether `error` is the Win32 error `code`.
pub(crate) fn is(error: &windows::core::Error, code: WIN32_ERROR) -> bool {
    error.code() == code.to_hresult()
}

/// The current user's SID in string form, e.g. `S-1-5-21-…-1001`. An elevated process of the
/// same account has the same SID.
pub(crate) fn current_user_sid() -> io::Result<String> {
    let mut token = HANDLE::default();
    // SAFETY: GetCurrentProcess returns a pseudo-handle; `token` is a valid out-pointer and the
    // token it receives is owned by the guard below.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(io_error)?;
    let token = OwnedHandle(token);
    let mut len = 0;
    // SAFETY: asks only for the size needed; no buffer is passed.
    match unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut len) } {
        Err(e) if is(&e, ERROR_INSUFFICIENT_BUFFER) => {}
        Err(e) => return Err(io_error(e)),
        Ok(()) => return Err(io::Error::other("the token has no user")),
    }
    // u64s, so the buffer is aligned for TOKEN_USER and the SID that follows it.
    let mut buffer = vec![0u64; (len as usize).div_ceil(size_of::<u64>())];
    // SAFETY: `buffer` is writable for at least `len` bytes.
    unsafe {
        GetTokenInformation(token.0, TokenUser, Some(buffer.as_mut_ptr().cast()), len, &mut len)
    }
    .map_err(io_error)?;
    // SAFETY: GetTokenInformation filled the start of `buffer`, which is suitably aligned and at
    // least as large as a TOKEN_USER, with one; its SID points into `buffer`, which outlives it.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    // SAFETY: the SID is valid (see above); `text` receives a LocalAlloc'd string, freed below.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }.map_err(io_error)?;
    // SAFETY: ConvertSidToStringSidW returned a NUL-terminated string.
    let sid = unsafe { text.to_string() };
    // SAFETY: frees the string ConvertSidToStringSidW allocated, once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    sid.map_err(|_| io::Error::other("the user's SID is not valid text"))
}

/// The Remote Desktop session this process runs in (the console session is usually 1).
pub(crate) fn current_session_id() -> io::Result<u32> {
    let mut session = 0;
    // SAFETY: `session` is a valid out-pointer.
    unsafe { ProcessIdToSessionId(std::process::id(), &mut session) }.map_err(io_error)?;
    Ok(session)
}

/// A security descriptor built from SDDL, freed when dropped.
#[derive(Debug)]
pub(crate) struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    /// Full access for the current user and for SYSTEM, nobody else, with a medium integrity
    /// label. The label is what lets a ZPadder that is not elevated open an object created by one
    /// that is: without it, the object would carry the creator's high label, which bars writes
    /// from lower integrity levels whatever the access list says.
    pub(crate) fn for_current_user() -> io::Result<Self> {
        let sid = current_user_sid()?;
        Self::from_sddl(&format!("D:P(A;;GA;;;{sid})(A;;GA;;;SY)S:(ML;;NW;;;ME)"))
    }

    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the SDDL string is NUL-terminated and outlives the call; `descriptor` receives a
        // LocalAlloc'd descriptor owned by the returned value.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(io_error)?;
        Ok(Self(descriptor))
    }

    /// Attributes for creating an object with this descriptor; the handle is not inherited. The
    /// result points into `self`, which must outlive its use.
    pub(crate) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW
        // and is freed exactly once.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_user_has_a_sid_and_a_session() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
        current_session_id().unwrap();
    }

    #[test]
    fn the_descriptor_for_zpadder_objects_is_valid() {
        let descriptor = SecurityDescriptor::for_current_user().unwrap();
        let attributes = descriptor.attributes();
        assert!(!attributes.lpSecurityDescriptor.is_null());
        assert!(SecurityDescriptor::from_sddl("not sddl").is_err());
    }

    #[test]
    fn windows_errors_keep_their_code() {
        let error = windows::core::Error::from(ERROR_INSUFFICIENT_BUFFER.to_hresult());
        assert_eq!(io_error(error).raw_os_error(), Some(ERROR_INSUFFICIENT_BUFFER.0 as i32));
    }
}
