//! The system's light/dark preference, which picks a theme set's light twin.
//!
//! On Windows it is `AppsUseLightTheme` under the current user's `Personalize` key, the
//! value Windows Terminal follows when its schemes are set as a dark/light pair. Elsewhere
//! the preference is unknown and reads as dark.

use std::sync::mpsc::Sender;

use crate::source::Event;

/// Whether the system asks apps for a light theme; `false` where that cannot be read.
pub(crate) fn prefers_light() -> bool {
    imp::prefers_light()
}

/// Sends [`Event::Appearance`] whenever the preference changes, from a thread that blocks
/// on the OS's change notification and never polls. Starts nothing where there is none.
pub(crate) fn watch(events: Sender<Event>) {
    imp::watch(events);
}

#[cfg(windows)]
mod imp {
    use std::ptr;
    use std::sync::mpsc::Sender;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, RRF_RT_REG_DWORD,
        RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW,
    };

    use crate::source::Event;

    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    const VALUE: &str = "AppsUseLightTheme";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub(super) fn prefers_light() -> bool {
        let (key, value) = (wide(KEY), wide(VALUE));
        let mut data: u32 = 0;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: both names are NUL-terminated UTF-16 that outlive the call, and `data` is
        // a writable DWORD whose size `size` states; RRF_RT_REG_DWORD refuses any other type.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                ptr::null_mut(),
                (&raw mut data).cast(),
                &mut size,
            )
        };
        status == ERROR_SUCCESS && data == 1
    }

    pub(super) fn watch(events: Sender<Event>) {
        let name = wide(KEY);
        let mut key: HKEY = ptr::null_mut();
        // SAFETY: `name` is NUL-terminated UTF-16 and `key` a writable handle slot.
        let opened =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, name.as_ptr(), 0, KEY_NOTIFY, &mut key) };
        if opened != ERROR_SUCCESS {
            return;
        }
        // HKEY is a raw pointer, which is not Send; the handle is owned by this thread alone.
        let key = key as usize;
        std::thread::spawn(move || {
            let key = key as HKEY;
            let mut last = prefers_light();
            loop {
                // SAFETY: `key` is open with KEY_NOTIFY; a synchronous call (no event) blocks
                // until a value under the key is set.
                let status = unsafe {
                    RegNotifyChangeKeyValue(key, 0, REG_NOTIFY_CHANGE_LAST_SET, ptr::null_mut(), 0)
                };
                if status != ERROR_SUCCESS {
                    break;
                }
                let now = prefers_light();
                if now != last {
                    last = now;
                    if events.send(Event::Appearance(now)).is_err() {
                        break;
                    }
                }
            }
            // SAFETY: `key` was opened above and is closed once.
            unsafe { RegCloseKey(key) };
        });
    }
}

#[cfg(not(windows))]
mod imp {
    use std::sync::mpsc::Sender;

    use crate::source::Event;

    pub(super) fn prefers_light() -> bool {
        false
    }

    pub(super) fn watch(_events: Sender<Event>) {}
}
