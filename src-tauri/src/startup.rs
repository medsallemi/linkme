use crate::storage::AppError;

#[cfg(windows)]
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS},
        System::Registry::{
            HKEY_CURRENT_USER, REG_DWORD, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
            RegSetKeyValueW,
        },
    },
    core::{PCWSTR, w},
};

#[cfg(windows)]
const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
#[cfg(windows)]
const RUN_NAME: PCWSTR = w!("Linkme");
#[cfg(windows)]
const MAX_COMMAND_CHARS: usize = 260;
#[cfg(windows)]
const PREFERENCES_KEY: PCWSTR = w!(r"Software\Linkme");
#[cfg(windows)]
const STARTUP_INITIALIZED: PCWSTR = w!("StartupDefaultApplied");
#[cfg(windows)]
const STARTUP_ENABLED: PCWSTR = w!("StartupEnabled");

#[cfg(windows)]
fn startup_error() -> AppError {
    AppError::new(
        "native_action",
        "Windows could not update the Linkme startup setting.",
    )
}

#[cfg(windows)]
fn mark_initialized() -> Result<(), AppError> {
    write_preference(STARTUP_INITIALIZED, 1)
}

#[cfg(windows)]
fn write_preference(name: PCWSTR, value: u32) -> Result<(), AppError> {
    // A separate marker preserves an explicit opt-out after the Run value is removed.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PREFERENCES_KEY,
            name,
            REG_DWORD.0,
            Some((&value as *const u32).cast()),
            size_of::<u32>() as u32,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(startup_error())
    }
}

#[cfg(all(windows, not(debug_assertions)))]
pub fn initialize_default() -> Result<(), AppError> {
    fn exists(key: PCWSTR, name: PCWSTR) -> Result<bool, AppError> {
        let mut size = 0;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key,
                name,
                windows::Win32::System::Registry::RRF_RT_ANY,
                None,
                None,
                Some(&mut size),
            )
        };
        match status {
            ERROR_SUCCESS => Ok(true),
            ERROR_FILE_NOT_FOUND => Ok(false),
            _ => Err(startup_error()),
        }
    }
    if exists(PREFERENCES_KEY, STARTUP_INITIALIZED)? {
        return Ok(());
    }
    // If an earlier release registered a path, leave that entry and any Windows
    // Startup Apps policy alone. Never reset policy or install a dev executable.
    if !exists(RUN_KEY, RUN_NAME)? {
        windows_preference(Some(true))?;
    }
    mark_initialized()
}

#[tauri::command]
pub fn startup_preference(enabled: Option<bool>) -> Result<bool, AppError> {
    #[cfg(windows)]
    {
        if let Some(enabled) = enabled {
            mark_initialized()?;
            let actual = windows_preference(Some(enabled))?;
            write_preference(STARTUP_ENABLED, if actual { 1 } else { 0 })?;
            return Ok(actual);
        }
        windows_preference(enabled)
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err(AppError::new(
            "native_action",
            "Startup is available on Windows.",
        ))
    }
}

#[cfg(windows)]
fn windows_preference(enabled: Option<bool>) -> Result<bool, AppError> {
    let path = std::env::current_exe().map_err(|_| startup_error())?;
    let expected = startup_command(&path);
    let expected_bytes = (expected.len() * size_of::<u16>()) as u32;
    // Windows documents a 260-character limit for commands in this Run key.
    if enabled == Some(true) && expected.len().saturating_sub(1) > MAX_COMMAND_CHARS {
        return Err(AppError::new(
            "native_action",
            "Move Linkme to a shorter path before enabling startup.",
        ));
    }
    match enabled {
        Some(true) => {
            // SAFETY: expected is a NUL-terminated UTF-16 registry string.
            let result = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    RUN_KEY,
                    RUN_NAME,
                    REG_SZ.0,
                    Some(expected.as_ptr().cast()),
                    expected_bytes,
                )
            };
            if result != ERROR_SUCCESS || !registration_matches(RUN_KEY, RUN_NAME, &expected)? {
                return Err(startup_error());
            }
            Ok(true)
        }
        Some(false) => {
            let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_NAME) };
            (result == ERROR_SUCCESS || result == ERROR_FILE_NOT_FOUND)
                .then_some(false)
                .ok_or_else(startup_error)
        }
        None => registration_matches(RUN_KEY, RUN_NAME, &expected),
    }
}

#[cfg(windows)]
fn registration_matches(key: PCWSTR, name: PCWSTR, expected: &[u16]) -> Result<bool, AppError> {
    // Allow room for the terminator and the extra UTF-16 unit Windows may
    // reserve for a string read. Compare only the bytes actually returned.
    let mut registered = [0u16; MAX_COMMAND_CHARS + 2];
    let mut bytes = size_of_val(&registered) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            RRF_RT_REG_SZ,
            None,
            Some(registered.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    match result {
        ERROR_SUCCESS => {
            let length = bytes as usize;
            Ok(length == size_of_val(expected)
                && registered.get(..expected.len()) == Some(expected))
        }
        ERROR_FILE_NOT_FOUND | ERROR_MORE_DATA => Ok(false),
        _ => Err(startup_error()),
    }
}

// Store a fully qualified, quoted path and an argument that identifies Windows logon.
#[cfg(windows)]
fn startup_command(path: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::iter::once(u16::from(b'"'))
        .chain(path.as_os_str().encode_wide())
        .chain(r#"" --windows-startup"#.encode_utf16())
        .chain([0])
        .collect()
}

#[cfg(all(windows, test))]
#[test]
fn quotes_the_full_path_and_identifies_startup() {
    let path = std::path::Path::new(r"C:\Program Files\Linkme\linkme.exe");
    let command = startup_command(path);
    assert_eq!(command.last(), Some(&0));
    assert_eq!(
        String::from_utf16(&command[..command.len() - 1]).unwrap(),
        r#""C:\Program Files\Linkme\linkme.exe" --windows-startup"#
    );
}

#[cfg(all(windows, test))]
#[test]
fn reads_back_an_enabled_registry_string_and_rejects_a_stale_path() {
    let name =
        windows::core::HSTRING::from(format!("LinkmeStartupReadbackTest-{}", std::process::id()));
    let name = PCWSTR(name.as_ptr());
    let expected = startup_command(std::path::Path::new(r"C:\Example Folder\linkme.exe"));
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PREFERENCES_KEY,
            name,
            REG_SZ.0,
            Some(expected.as_ptr().cast()),
            size_of_val(expected.as_slice()) as u32,
        )
    };
    assert_eq!(status, ERROR_SUCCESS);
    let actual = registration_matches(PREFERENCES_KEY, name, &expected);
    let stale = startup_command(std::path::Path::new(r"C:\Another Folder\linkme.exe"));
    let moved = registration_matches(PREFERENCES_KEY, name, &stale);
    let removed = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PREFERENCES_KEY, name) };
    assert_eq!(removed, ERROR_SUCCESS);
    assert!(actual.unwrap());
    assert!(!moved.unwrap());
    assert!(!registration_matches(PREFERENCES_KEY, name, &expected).unwrap());
}
