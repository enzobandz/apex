//! Startup entries from the Run keys and Startup folders, enabled/disabled through
//! `Explorer\StartupApproved` exactly like Task Manager › Startup apps does.
//!
//! StartupApproved values are 12-byte REG_BINARY. The first byte's low bit marks
//! the entry disabled (0x03/0x07 = disabled, 0x02/0x06 = enabled); bytes 4..12 hold
//! a FILETIME of when it was disabled. A missing value means enabled.

use apex_core::actions::{BackendError, SettingValue};
use apex_core::model::{StartupEntry, StartupLocation};
use std::borrow::Cow;
use std::io;
use std::path::{Path, PathBuf};
use winreg::enums::*;
use winreg::types::FromRegValue;
use winreg::{RegKey, RegValue};

use crate::startup_util::{executable_from_command, make_id, parse_id};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN32: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

/// (hive, StartupApproved subkey) for a location.
fn approved_key(l: StartupLocation) -> (RegKey, String) {
    let (hive, sub) = match l {
        StartupLocation::UserRun => (HKEY_CURRENT_USER, "Run"),
        StartupLocation::MachineRun => (HKEY_LOCAL_MACHINE, "Run"),
        StartupLocation::MachineRun32 => (HKEY_LOCAL_MACHINE, "Run32"),
        StartupLocation::UserStartupFolder => (HKEY_CURRENT_USER, "StartupFolder"),
        StartupLocation::CommonStartupFolder => (HKEY_LOCAL_MACHINE, "StartupFolder"),
    };
    (RegKey::predef(hive), format!(r"{APPROVED}\{sub}"))
}

fn map_io(e: io::Error, what: &str) -> BackendError {
    match e.kind() {
        io::ErrorKind::PermissionDenied => BackendError::RequiresAdmin(what.into()),
        io::ErrorKind::NotFound => BackendError::NotFound(what.into()),
        _ => BackendError::Os(format!("{what}: {e}")),
    }
}

/// Current approval state: `Absent` (never toggled → enabled) or `Bool(enabled)`.
pub fn read_approved(entry_id: &str) -> Result<SettingValue, BackendError> {
    let (loc, name) = parse_id(entry_id)?;
    let (hive, path) = approved_key(loc);
    let key = match hive.open_subkey_with_flags(&path, KEY_READ) {
        Ok(k) => k,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(SettingValue::Absent),
        Err(e) => return Err(map_io(e, &path)),
    };
    match key.get_raw_value(&name) {
        Ok(v) => Ok(SettingValue::Bool(
            v.bytes.first().map(|b| b & 1 == 0).unwrap_or(true),
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(SettingValue::Absent),
        Err(e) => Err(map_io(e, &name)),
    }
}

fn filetime_now() -> u64 {
    // 100 ns intervals since 1601-01-01.
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (unix.as_nanos() / 100) as u64 + 116_444_736_000_000_000
}

pub fn write_approved(entry_id: &str, value: &SettingValue) -> Result<(), BackendError> {
    let (loc, name) = parse_id(entry_id)?;
    // Only toggle entries that actually exist, so we never create orphan values.
    if !entry_exists(loc, &name) && !matches!(value, SettingValue::Absent) {
        return Err(BackendError::NotFound(format!(
            "startup entry {name} no longer exists"
        )));
    }
    let (hive, path) = approved_key(loc);
    match value {
        SettingValue::Absent => {
            let key = match hive.open_subkey_with_flags(&path, KEY_SET_VALUE) {
                Ok(k) => k,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(map_io(e, &path)),
            };
            match key.delete_value(&name) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(map_io(e, &name)),
            }
        }
        SettingValue::Bool(enabled) => {
            let (key, _) = hive
                .create_subkey_with_flags(&path, KEY_SET_VALUE | KEY_QUERY_VALUE)
                .map_err(|e| map_io(e, &path))?;
            let mut bytes = vec![0u8; 12];
            if *enabled {
                bytes[0] = 0x02;
            } else {
                bytes[0] = 0x03;
                bytes[4..12].copy_from_slice(&filetime_now().to_le_bytes());
            }
            key.set_raw_value(
                &name,
                &RegValue {
                    bytes: Cow::Owned(bytes),
                    vtype: REG_BINARY,
                },
            )
            .map_err(|e| map_io(e, &name))
        }
        SettingValue::Text(_) => Err(BackendError::Refused(
            "startup state must be a boolean".into(),
        )),
    }
}

fn startup_folder(loc: StartupLocation) -> Option<PathBuf> {
    let base = match loc {
        StartupLocation::UserStartupFolder => std::env::var_os("APPDATA")?,
        StartupLocation::CommonStartupFolder => std::env::var_os("ProgramData")?,
        _ => return None,
    };
    Some(Path::new(&base).join(r"Microsoft\Windows\Start Menu\Programs\Startup"))
}

fn run_key(loc: StartupLocation) -> Option<(RegKey, &'static str)> {
    match loc {
        StartupLocation::UserRun => Some((RegKey::predef(HKEY_CURRENT_USER), RUN)),
        StartupLocation::MachineRun => Some((RegKey::predef(HKEY_LOCAL_MACHINE), RUN)),
        StartupLocation::MachineRun32 => Some((RegKey::predef(HKEY_LOCAL_MACHINE), RUN32)),
        _ => None,
    }
}

fn entry_exists(loc: StartupLocation, name: &str) -> bool {
    if let Some((hive, path)) = run_key(loc) {
        return hive
            .open_subkey_with_flags(path, KEY_READ)
            .and_then(|k| k.get_raw_value(name))
            .is_ok();
    }
    startup_folder(loc)
        .map(|d| d.join(name).is_file())
        .unwrap_or(false)
}

pub fn enumerate() -> (Vec<StartupEntry>, Vec<String>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for loc in [
        StartupLocation::UserRun,
        StartupLocation::MachineRun,
        StartupLocation::MachineRun32,
    ] {
        let (hive, path) = run_key(loc).expect("registry location");
        let key = match hive.open_subkey_with_flags(path, KEY_READ) {
            Ok(k) => k,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => {
                problems.push(format!("Could not read {}: {e}", loc.describe()));
                continue;
            }
        };
        for item in key.enum_values() {
            let Ok((name, val)) = item else { continue };
            if name.is_empty() {
                continue;
            }
            let command = String::from_reg_value(&val).unwrap_or_default();
            let id = make_id(loc, &name);
            let enabled = !matches!(read_approved(&id), Ok(SettingValue::Bool(false)));
            let exe = executable_from_command(&command);
            out.push(StartupEntry {
                executable_exists: exe.as_ref().map(|p| Path::new(p).exists()),
                executable_path: exe,
                id,
                name,
                command,
                location: loc,
                enabled,
                publisher: None,
            });
        }
    }
    for loc in [
        StartupLocation::UserStartupFolder,
        StartupLocation::CommonStartupFolder,
    ] {
        let Some(dir) = startup_folder(loc) else {
            continue;
        };
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for f in rd.flatten() {
            let file_name = f.file_name().to_string_lossy().into_owned();
            if file_name.eq_ignore_ascii_case("desktop.ini") || !f.path().is_file() {
                continue;
            }
            let id = make_id(loc, &file_name);
            let enabled = !matches!(read_approved(&id), Ok(SettingValue::Bool(false)));
            let is_lnk = file_name.to_ascii_lowercase().ends_with(".lnk");
            out.push(StartupEntry {
                id,
                name: file_name
                    .trim_end_matches(".lnk")
                    .trim_end_matches(".LNK")
                    .to_string(),
                command: f.path().display().to_string(),
                location: loc,
                enabled,
                // Shortcut targets need the Shell COM API to resolve; not resolved in this version.
                executable_path: if is_lnk {
                    None
                } else {
                    Some(f.path().display().to_string())
                },
                executable_exists: if is_lnk { None } else { Some(true) },
                publisher: None,
            });
        }
    }
    (out, problems)
}
