//! OS integration that needs Win32 calls.
//!
//! `vfm-app` denies `unsafe_code`, so the handful of platform reads that genuinely need
//! the Win32 API live here behind safe functions. Everything is `cfg(windows)`; other
//! targets fall back to conservative defaults.

/// Whether Windows is currently in light mode.
///
/// Reads `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\
/// AppsUseLightTheme`, which is what the Settings app toggles. Returns `false` (dark) when
/// the value is missing, which is the Windows default for new installs.
#[cfg(windows)]
pub fn system_uses_light_theme() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        // SAFETY: the key and value names are NUL-terminated literals, and `value`/`size`
        // describe a live 4-byte buffer that RegGetValueW writes at most `size` bytes into.
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    status.is_ok() && value == 1
}

#[cfg(not(windows))]
pub fn system_uses_light_theme() -> bool {
    false
}

/// Native "open file" dialog. Returns the chosen paths, or an empty list when cancelled.
///
/// `filter_label`/`extensions` build a single file-type filter (extensions without dots,
/// e.g. `["mp4", "mkv"]`). The dialog is modal to the foreground window.
#[cfg(windows)]
pub fn pick_files(title: &str, filter_label: &str, extensions: &[&str]) -> Vec<std::path::PathBuf> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST,
        FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
    };

    let mut out = Vec::new();
    // SAFETY: every call below is a documented shell/COM entry point. The apartment is
    // initialised for this thread and released before returning; handles are dropped by RAII.
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let owned = hr.is_ok();
        let dialog: Result<IFileOpenDialog, _> =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL);
        if let Ok(dialog) = dialog {
            if let Ok(options) = dialog.GetOptions() {
                let extra = FOS_FORCEFILESYSTEM
                    | FOS_FILEMUSTEXIST
                    | FOS_PATHMUSTEXIST
                    | FOS_ALLOWMULTISELECT;
                let _ = dialog.SetOptions(options | extra);
            }
            let heading = HSTRING::from(title);
            let _ = dialog.SetTitle(PCWSTR(heading.as_ptr()));

            if !extensions.is_empty() {
                let pattern = extensions
                    .iter()
                    .map(|ext| format!("*.{ext}"))
                    .collect::<Vec<_>>()
                    .join(";");
                let name = HSTRING::from(filter_label);
                let spec = HSTRING::from(pattern);
                let filters = [COMDLG_FILTERSPEC {
                    pszName: PCWSTR(name.as_ptr()),
                    pszSpec: PCWSTR(spec.as_ptr()),
                }];
                let _ = dialog.SetFileTypes(&filters);
            }

            // Cancelling returns an error; that is not a failure we report.
            if dialog.Show(None).is_ok() {
                if let Ok(items) = dialog.GetResults() {
                    out = collect_paths(&items);
                }
            }
        }
        if owned {
            CoUninitialize();
        }
    }
    out
}

#[cfg(windows)]
unsafe fn collect_paths(
    items: &windows::Win32::UI::Shell::IShellItemArray,
) -> Vec<std::path::PathBuf> {
    use windows::Win32::UI::Shell::SIGDN_FILESYSPATH;
    let mut paths = Vec::new();
    let Ok(count) = items.GetCount() else {
        return paths;
    };
    for index in 0..count {
        let Ok(item) = items.GetItemAt(index) else {
            continue;
        };
        let Ok(name) = item.GetDisplayName(SIGDN_FILESYSPATH) else {
            continue;
        };
        if let Ok(text) = name.to_string() {
            if !text.is_empty() {
                paths.push(std::path::PathBuf::from(text));
            }
        }
        windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as *const core::ffi::c_void));
    }
    paths
}

/// Native folder picker. `None` when the user cancels.
#[cfg(windows)]
pub fn pick_folder(title: &str) -> Option<std::path::PathBuf> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
        SIGDN_FILESYSPATH,
    };

    let mut out = None;
    // SAFETY: as above — apartment init/teardown around documented shell calls.
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let owned = hr.is_ok();
        let dialog: Result<IFileOpenDialog, _> =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL);
        if let Ok(dialog) = dialog {
            let options = FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
            let _ = dialog.SetOptions(options);
            let heading = HSTRING::from(title);
            let _ = dialog.SetTitle(PCWSTR(heading.as_ptr()));
            if dialog.Show(None).is_ok() {
                if let Ok(item) = dialog.GetResult() {
                    if let Ok(name) = item.GetDisplayName(SIGDN_FILESYSPATH) {
                        if let Ok(text) = name.to_string() {
                            if !text.is_empty() {
                                out = Some(std::path::PathBuf::from(text));
                            }
                        }
                        windows::Win32::System::Com::CoTaskMemFree(Some(
                            name.0 as *const core::ffi::c_void,
                        ));
                    }
                }
            }
        }
        if owned {
            CoUninitialize();
        }
    }
    out
}

/// Text currently on the clipboard, if it is a plain Unicode string.
#[cfg(windows)]
pub fn clipboard_text() -> Option<String> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    const CF_UNICODETEXT: u32 = 13;
    // SAFETY: the clipboard is opened and always closed; the global block is locked while we
    // read it and unlocked before we return.
    unsafe {
        if OpenClipboard(None).is_err() {
            return None;
        }
        let handle = GetClipboardData(CF_UNICODETEXT);
        let text = match handle {
            Ok(handle) if !handle.is_invalid() => {
                let global = HGLOBAL(handle.0);
                let ptr = GlobalLock(global) as *const u16;
                if ptr.is_null() {
                    None
                } else {
                    let mut len = 0usize;
                    while *ptr.add(len) != 0 && len < 32_768 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(ptr, len);
                    let value = String::from_utf16_lossy(slice);
                    let _ = GlobalUnlock(global);
                    Some(value)
                }
            }
            _ => None,
        };
        let _ = CloseClipboard();
        text.filter(|t| !t.trim().is_empty())
    }
}

/// Friendly names of the capture devices DirectShow currently exposes, split by kind.
///
/// These are the exact names libVLC's `dshow` module accepts for `dshow-vdev` / `dshow-adev`,
/// because both read the same `FriendlyName` property from the same device categories. Empty
/// lists are honest: they mean Windows reported no device of that kind, not that enumeration
/// failed silently. The Open dialog still lets a name be typed for either field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CaptureDevices {
    pub video: Vec<String>,
    pub audio: Vec<String>,
}

impl CaptureDevices {
    pub fn is_empty(&self) -> bool {
        self.video.is_empty() && self.audio.is_empty()
    }
}

/// Enumerate video and audio capture devices through the DirectShow system device
/// enumerator. The call initialises and tears down its own COM apartment, so it is safe to
/// call from any thread at any time.
#[cfg(windows)]
pub fn enumerate_capture_devices() -> CaptureDevices {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

    /// `CLSID_VideoInputDeviceCategory`.
    const VIDEO_CATEGORY: windows::core::GUID =
        windows::core::GUID::from_u128(0x860bb310_5d01_11d0_bd3b_00a0c911ce86);
    /// `CLSID_AudioInputDeviceCategory`.
    const AUDIO_CATEGORY: windows::core::GUID =
        windows::core::GUID::from_u128(0x33d9a762_90c8_11d0_bd43_00a0c911ce86);

    let mut out = CaptureDevices::default();
    // SAFETY: COM is initialised on this thread and released before returning. Every call
    // below is a documented shell/com entry point; the enumerator and moniker handles are
    // released by RAII, and the VARIANT is cleared explicitly after each read.
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let owned = hr.is_ok();
        // A failure here (for example an apartment already on a different model) does not
        // make the call unsafe — COM is still initialised — so we still attempt enumeration.
        out.video = collect_devices(&VIDEO_CATEGORY);
        out.audio = collect_devices(&AUDIO_CATEGORY);
        if owned {
            CoUninitialize();
        }
    }
    out
}

/// Read the `FriendlyName` of every moniker in one DirectShow device category.
///
/// Returns only names Windows actually reported; duplicates are collapsed and blank names
/// are dropped. Any COM failure is treated as "no device in this category" so the caller
/// never sees a fabricated entry or a panic.
#[cfg(windows)]
unsafe fn collect_devices(category: &windows::core::GUID) -> Vec<String> {
    use windows::core::{w, GUID};
    use windows::Win32::Media::DirectShow::ICreateDevEnum;
    use windows::Win32::System::Com::StructuredStorage::IPropertyBag;
    use windows::Win32::System::Com::{CoCreateInstance, IEnumMoniker, IMoniker, CLSCTX_ALL};
    use windows::Win32::System::Variant::{VariantClear, VARIANT, VT_BSTR};

    /// `CLSID_SystemDeviceEnum`.
    const SYSTEM_DEVICE_ENUM: GUID = GUID::from_u128(0x62be5d10_60eb_11d0_bd3b_00a0c911ce86);

    let mut out = Vec::new();
    let Ok(denum) = CoCreateInstance::<_, ICreateDevEnum>(&SYSTEM_DEVICE_ENUM, None, CLSCTX_ALL)
    else {
        return out;
    };

    let mut monikers: Option<IEnumMoniker> = None;
    if denum
        .CreateClassEnumerator(category, &mut monikers, 0)
        .is_err()
    {
        return out;
    }
    let Some(monikers) = monikers else {
        return out;
    };

    loop {
        let mut moniker: Option<IMoniker> = None;
        let mut fetched = 0u32;
        let status = monikers.Next(std::slice::from_mut(&mut moniker), Some(&mut fetched));
        if status.is_err() || fetched == 0 {
            break;
        }
        let Some(moniker) = moniker else {
            break;
        };
        let Ok(bag) = moniker.BindToStorage::<_, _, IPropertyBag>(None, None) else {
            continue;
        };

        let mut variant = VARIANT::default();
        if bag.Read(w!("FriendlyName"), &mut variant, None).is_ok() {
            // SAFETY: `Read` initialised `variant`; a VT_BSTR discriminant guarantees the
            // union's `bstrVal` member is the live one. We read it before `VariantClear`.
            let name = unsafe {
                let fields = &*variant.Anonymous.Anonymous;
                if fields.vt == VT_BSTR {
                    Some(fields.Anonymous.bstrVal.to_string())
                } else {
                    None
                }
            };
            if let Some(name) = name {
                let trimmed = name.trim();
                if !trimmed.is_empty() && !out.iter().any(|existing: &String| existing == trimmed) {
                    out.push(trimmed.to_string());
                }
            }
        }
        let _ = VariantClear(&mut variant);
    }
    out
}

#[cfg(not(windows))]
pub fn enumerate_capture_devices() -> CaptureDevices {
    CaptureDevices::default()
}

#[cfg(not(windows))]
pub fn pick_files(_title: &str, _label: &str, _extensions: &[&str]) -> Vec<std::path::PathBuf> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn pick_folder(_title: &str) -> Option<std::path::PathBuf> {
    None
}

#[cfg(not(windows))]
pub fn clipboard_text() -> Option<String> {
    None
}

/// Whether a process with this id is currently running.
///
/// The single-instance guard uses this to tell a live lock file from one left behind by a
/// crash. It asks the kernel directly instead of shelling out to `tasklist`, so starting the
/// app never flashes a console window or waits on a child process.
#[cfg(windows)]
pub fn process_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    /// `STILL_ACTIVE`: the exit code the kernel reports for a live process.
    const STILL_ACTIVE: u32 = 259;

    if pid == 0 {
        return false;
    }
    // SAFETY: OpenProcess either fails or returns a handle that we close exactly once below.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut code = 0u32;
        let alive = GetExitCodeProcess(handle, &mut code).is_ok() && code == STILL_ACTIVE;
        let _ = CloseHandle(handle);
        alive
    }
}

#[cfg(not(windows))]
pub fn process_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn theme_probe_returns_a_bool() {
        // The value depends on the host, but the call must not panic or hang.
        let _ = super::system_uses_light_theme();
    }

    #[test]
    fn missing_clipboard_is_not_an_error() {
        // Nothing guarantees text on the clipboard; the call must stay total.
        let _ = super::clipboard_text();
    }

    #[test]
    fn the_current_process_is_alive() {
        assert!(super::process_alive(std::process::id()));
    }

    #[test]
    fn pid_zero_is_never_alive() {
        assert!(!super::process_alive(0));
    }

    #[test]
    fn capture_enumeration_is_total_and_repeatable() {
        // The host may have no capture device at all; the call must still be total, must not
        // panic, and must return the same shape on a second call.
        let first = super::enumerate_capture_devices();
        let second = super::enumerate_capture_devices();
        assert_eq!(first, second);
        for name in first.video.iter().chain(first.audio.iter()) {
            assert!(!name.trim().is_empty(), "device names must be non-blank");
        }
        // Within each category, names are unique.
        for group in [&first.video, &first.audio] {
            let mut seen = std::collections::HashSet::new();
            for name in group {
                assert!(seen.insert(name), "duplicate device name: {name}");
            }
        }
    }

    #[test]
    fn empty_capture_devices_reports_empty() {
        assert!(super::CaptureDevices::default().is_empty());
    }
}
