//! Whether a path is owned by exactly the current user: the ownership test git
//! applies before it trusts a repository. The path's own entry is judged, not a
//! junction or symlink target, so callers canonicalise first.
//!
//! `Ok(false)` also covers every case this module declines to judge: a network
//! or device path on Windows, a group owner such as Administrators, and a
//! process running as root on Unix. Missing or unreadable paths are `Err`.

use std::io;
use std::path::Path;

#[cfg(unix)]
pub(crate) fn is_owned_by_current_user(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let uid = std::fs::symlink_metadata(path)?.uid();
    // SAFETY: `geteuid` takes no arguments, touches no memory and cannot fail.
    let euid = unsafe { libc::geteuid() };
    Ok(euid != 0 && uid == euid)
}

#[cfg(windows)]
pub(crate) fn is_owned_by_current_user(path: &Path) -> io::Result<bool> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        EqualSid, IsValidSid, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    };

    if !is_local_disk_path(path) {
        return Ok(false);
    }
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains a NUL character",
        ));
    }
    wide.push(0);

    let user = win::CurrentUser::query()?;
    let mut owner: PSID = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: `wide` is NUL-terminated and outlives the call; the owner and
    // descriptor out-pointers are valid locals, and the three unwanted outputs
    // are null, which the API documents as "not requested".
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _descriptor = win::LocalAllocation(descriptor);

    // SAFETY: `owner` is null or points into `descriptor`, which stays alive
    // until `_descriptor` drops; `user.sid()` points into `user`'s buffer.
    let owned =
        unsafe { !owner.is_null() && IsValidSid(owner) != 0 && EqualSid(owner, user.sid()) != 0 };
    Ok(owned)
}

/// A drive-letter path, plain or `\\?\`-prefixed. UNC, device and
/// prefix-less paths are left to git.
#[cfg(windows)]
fn is_local_disk_path(path: &Path) -> bool {
    use std::path::{Component, Prefix};

    matches!(
        path.components().next(),
        Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
    )
}

#[cfg(windows)]
mod win {
    use std::io;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, HLOCAL, LocalFree,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// A buffer `LocalAlloc`ed by the system on our behalf, freed on drop.
    pub(super) struct LocalAllocation(pub(super) HLOCAL);

    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the pointer came from an API that documents
                // `LocalFree` as its release, and is freed exactly once here.
                unsafe { LocalFree(self.0) };
            }
        }
    }

    struct Token(HANDLE);

    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: the handle was opened by `OpenProcessToken` and is closed
            // exactly once here.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// The process token's `TOKEN_USER`, held in a `u64` buffer so the struct
    /// at its start is suitably aligned; the SID it names lives in the buffer.
    pub(super) struct CurrentUser(Vec<u64>);

    impl CurrentUser {
        pub(super) fn query() -> io::Result<Self> {
            let mut handle: HANDLE = null_mut();
            // SAFETY: `GetCurrentProcess` returns a pseudo-handle that needs no
            // closing; `handle` is a valid out-pointer.
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = Token(handle);

            let mut len = 0u32;
            // SAFETY: a null buffer of length zero is the documented sizing
            // call; only `len` is written.
            if unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut len) } == 0 {
                let err = io::Error::last_os_error();
                if err.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
                    return Err(err);
                }
            }
            if (len as usize) < size_of::<TOKEN_USER>() {
                return Err(io::Error::other("token user information is truncated"));
            }

            let mut buf = vec![0u64; (len as usize).div_ceil(size_of::<u64>())];
            // SAFETY: `buf` holds at least `len` writable bytes and is 8-byte
            // aligned, which covers `TOKEN_USER`'s alignment.
            if unsafe {
                GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(buf))
        }

        pub(super) fn sid(&self) -> PSID {
            // SAFETY: `query` filled the buffer with a `TOKEN_USER` at offset
            // zero, and the buffer is at least that large and aligned for it.
            unsafe { (*self.0.as_ptr().cast::<TOKEN_USER>()).User.Sid }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_owned_by_current_user;
    use std::path::Path;

    #[test]
    fn a_directory_this_test_created_is_owned() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(is_owned_by_current_user(tmp.path()).expect("owner"));
        let canonical = tmp.path().canonicalize().expect("canonicalize");
        assert!(is_owned_by_current_user(&canonical).expect("owner"));
    }

    #[test]
    fn a_missing_path_is_an_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(is_owned_by_current_user(&tmp.path().join("missing")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_directory_is_not_owned() {
        assert!(!is_owned_by_current_user(Path::new(r"C:\Windows")).expect("owner"));
    }

    #[cfg(windows)]
    #[test]
    fn only_drive_letter_paths_are_judged() {
        for p in [
            r"\\?\UNC\localhost\c$\Windows",
            r"\\localhost\c$\Windows",
            r"\\.\C:\Windows",
            r"Windows",
        ] {
            assert!(
                !is_owned_by_current_user(Path::new(p)).expect("owner"),
                "{p}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_filesystem_root_is_not_owned() {
        assert!(!is_owned_by_current_user(Path::new("/")).expect("owner"));
    }
}
