//! Windows implementation of reads/writes. Everything here runs **without**
//! administrator rights except HKLM startup entries, which return
//! `BackendError::RequiresAdmin` instead of attempting elevation silently.

pub mod display;
pub mod startup;

use apex_core::actions::{BackendError, PriorityClass, Setting, SettingValue, SystemBackend};
use apex_core::model::{GpuInfo, PowerInfo, PowerScheme};
use std::ffi::c_void;
use std::io;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

use crate::powercfg_parse::{is_guid, parse_list};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const GAMEBAR: &str = r"Software\Microsoft\GameBar";
const GAME_MODE_VALUE: &str = "AutoGameModeEnabled";

pub struct WindowsBackend;

fn system32(exe: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(root).join("System32").join(exe)
}

fn powercfg(args: &[&str]) -> Result<String, BackendError> {
    let out = Command::new(system32("powercfg.exe"))
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| BackendError::Os(format!("could not run powercfg: {e}")))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(BackendError::Os(format!(
            "powercfg {} failed: {msg}",
            args.join(" ")
        )));
    }
    // powercfg writes in the console's OEM code page; GUIDs and '*' are ASCII so lossy UTF-8 is fine
    // for parsing, and non-ASCII scheme names may display imperfectly.
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn power_schemes() -> Result<(Vec<PowerScheme>, Option<String>), BackendError> {
    Ok(parse_list(&powercfg(&["/list"])?))
}

#[repr(C)]
#[allow(non_snake_case)]
struct SystemPowerStatus {
    ACLineStatus: u8,
    BatteryFlag: u8,
    BatteryLifePercent: u8,
    SystemStatusFlag: u8,
    BatteryLifeTime: u32,
    BatteryFullLifeTime: u32,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn CloseHandle(h: *mut c_void) -> i32;
    fn GetPriorityClass(h: *mut c_void) -> u32;
    fn SetPriorityClass(h: *mut c_void, class: u32) -> i32;
}

const PROCESS_SET_INFORMATION: u32 = 0x0200;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

fn class_to_raw(p: PriorityClass) -> u32 {
    match p {
        PriorityClass::Idle => 0x40,
        PriorityClass::BelowNormal => 0x4000,
        PriorityClass::Normal => 0x20,
        PriorityClass::AboveNormal => 0x8000,
        PriorityClass::High => 0x80,
    }
}

fn raw_to_class(r: u32) -> Option<PriorityClass> {
    Some(match r {
        0x40 => PriorityClass::Idle,
        0x4000 => PriorityClass::BelowNormal,
        0x20 => PriorityClass::Normal,
        0x8000 => PriorityClass::AboveNormal,
        0x80 => PriorityClass::High,
        _ => return None, // realtime (0x100) or unknown: never touched
    })
}

struct Handle(*mut c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn open_process(pid: u32, access: u32) -> Result<Handle, BackendError> {
    let h = unsafe { OpenProcess(access, 0, pid) };
    if h.is_null() {
        let e = io::Error::last_os_error();
        return Err(if e.raw_os_error() == Some(5) {
            BackendError::RequiresAdmin(format!(
                "process {pid} belongs to another user or is protected"
            ))
        } else {
            BackendError::NotFound(format!("process {pid}: {e}"))
        });
    }
    Ok(Handle(h))
}

pub fn get_priority(pid: u32) -> Result<PriorityClass, BackendError> {
    let h = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let raw = unsafe { GetPriorityClass(h.0) };
    if raw == 0 {
        return Err(BackendError::Os(io::Error::last_os_error().to_string()));
    }
    raw_to_class(raw).ok_or_else(|| {
        BackendError::Refused(format!(
            "process {pid} has an unsupported priority class (0x{raw:x})"
        ))
    })
}

fn set_priority(pid: u32, p: PriorityClass) -> Result<(), BackendError> {
    let h = open_process(
        pid,
        PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION,
    )?;
    if unsafe { SetPriorityClass(h.0, class_to_raw(p)) } == 0 {
        return Err(BackendError::Os(io::Error::last_os_error().to_string()));
    }
    Ok(())
}

pub fn power_info() -> Result<PowerInfo, BackendError> {
    let (available, active) = power_schemes()?;
    let active_guid =
        active.ok_or_else(|| BackendError::Os("powercfg reported no active scheme".into()))?;
    let active = available
        .iter()
        .find(|s| s.guid == active_guid)
        .cloned()
        .unwrap_or(PowerScheme {
            guid: active_guid.clone(),
            name: active_guid,
        });
    let mut st = SystemPowerStatus {
        ACLineStatus: 255,
        BatteryFlag: 255,
        BatteryLifePercent: 255,
        SystemStatusFlag: 0,
        BatteryLifeTime: 0,
        BatteryFullLifeTime: 0,
    };
    let ok = unsafe { GetSystemPowerStatus(&mut st) } != 0;
    let (on_battery, battery_present) = if ok {
        (
            match st.ACLineStatus {
                0 => Some(true),
                1 => Some(false),
                _ => None,
            },
            match st.BatteryFlag {
                128 => Some(false),
                255 => None,
                _ => Some(true),
            },
        )
    } else {
        (None, None)
    };
    Ok(PowerInfo {
        active,
        available,
        on_battery,
        battery_present,
    })
}

pub fn game_mode() -> Result<SettingValue, BackendError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hkcu.open_subkey_with_flags(GAMEBAR, KEY_READ) {
        Ok(k) => k,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(SettingValue::Absent),
        Err(e) => return Err(BackendError::Os(e.to_string())),
    };
    match key.get_value::<u32, _>(GAME_MODE_VALUE) {
        Ok(v) => Ok(SettingValue::Bool(v != 0)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(SettingValue::Absent),
        Err(e) => Err(BackendError::Os(e.to_string())),
    }
}

fn set_game_mode(v: &SettingValue) -> Result<(), BackendError> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    match v {
        SettingValue::Bool(b) => {
            let (key, _) = hkcu
                .create_subkey_with_flags(GAMEBAR, KEY_SET_VALUE)
                .map_err(|e| BackendError::Os(e.to_string()))?;
            key.set_value(GAME_MODE_VALUE, &(*b as u32))
                .map_err(|e| BackendError::Os(e.to_string()))
        }
        SettingValue::Absent => match hkcu.open_subkey_with_flags(GAMEBAR, KEY_SET_VALUE) {
            Ok(k) => match k.delete_value(GAME_MODE_VALUE) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => {
                    Err(BackendError::Os(e.to_string()))
                }
                _ => Ok(()),
            },
            Err(_) => Ok(()),
        },
        SettingValue::Text(_) => Err(BackendError::Refused("game mode must be a boolean".into())),
    }
}

/// Display adapters from the device class key (readable without admin).
pub fn gpus() -> Vec<GpuInfo> {
    const CLASS: &str =
        r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(class) = hklm.open_subkey_with_flags(CLASS, KEY_READ) else {
        return vec![];
    };
    let mut out = Vec::new();
    for sub in class.enum_keys().flatten() {
        if !sub.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(k) = class.open_subkey_with_flags(&sub, KEY_READ) else {
            continue;
        };
        let Ok(name) = k.get_value::<String, _>("DriverDesc") else {
            continue;
        };
        let mem = k
            .get_value::<u64, _>("HardwareInformation.qwMemorySize")
            .ok()
            .or_else(|| {
                k.get_raw_value("HardwareInformation.MemorySize")
                    .ok()
                    .and_then(|v| match v.bytes.len() {
                        4 => Some(u32::from_le_bytes(v.bytes[..4].try_into().ok()?) as u64),
                        8 => Some(u64::from_le_bytes(v.bytes[..8].try_into().ok()?)),
                        _ => None,
                    })
            })
            .filter(|m| *m > 0);
        // Microsoft Basic Display / Remote Display adapters are not real GPUs.
        if name.contains("Basic Display") || name.contains("Remote Display") {
            continue;
        }
        if out.iter().any(|g: &GpuInfo| g.name == name) {
            continue;
        }
        out.push(GpuInfo {
            name,
            driver_version: k.get_value::<String, _>("DriverVersion").ok(),
            dedicated_memory_bytes: mem,
        });
    }
    out
}

/// (product name, display version e.g. "23H2", build number)
pub fn os_details() -> Option<(String, Option<String>, Option<u32>)> {
    let k = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", KEY_READ)
        .ok()?;
    let build: Option<u32> = k
        .get_value::<String, _>("CurrentBuildNumber")
        .ok()
        .and_then(|b| b.parse().ok());
    let mut product: String = k
        .get_value("ProductName")
        .unwrap_or_else(|_| "Windows".into());
    // ProductName still says "Windows 10" on Windows 11; build 22000+ is Windows 11.
    if build.map(|b| b >= 22000).unwrap_or(false) {
        product = product.replace("Windows 10", "Windows 11");
    }
    Some((
        product,
        k.get_value::<String, _>("DisplayVersion").ok(),
        build,
    ))
}

impl SystemBackend for WindowsBackend {
    fn read(&self, setting: &Setting) -> Result<SettingValue, BackendError> {
        match setting {
            Setting::ActivePowerScheme => {
                let (_, active) = power_schemes()?;
                active
                    .map(SettingValue::Text)
                    .ok_or_else(|| BackendError::Os("no active power scheme".into()))
            }
            Setting::StartupEntryEnabled { entry_id } => startup::read_approved(entry_id),
            Setting::GameMode => game_mode(),
            Setting::ProcessPriority { pid } => {
                Ok(SettingValue::Text(get_priority(*pid)?.as_str().into()))
            }
        }
    }

    fn write(&self, setting: &Setting, value: &SettingValue) -> Result<(), BackendError> {
        match (setting, value) {
            (Setting::ActivePowerScheme, SettingValue::Text(guid)) => {
                if !is_guid(guid) {
                    return Err(BackendError::Refused(format!(
                        "{guid} is not a power scheme GUID"
                    )));
                }
                let (schemes, _) = power_schemes()?;
                if !schemes.iter().any(|s| s.guid.eq_ignore_ascii_case(guid)) {
                    return Err(BackendError::NotFound(format!(
                        "power scheme {guid} is not installed"
                    )));
                }
                powercfg(&["/setactive", guid]).map(|_| ())
            }
            (Setting::StartupEntryEnabled { entry_id }, v) => startup::write_approved(entry_id, v),
            (Setting::GameMode, v) => set_game_mode(v),
            (Setting::ProcessPriority { pid }, SettingValue::Text(p)) => {
                let class = PriorityClass::parse(p)
                    .ok_or_else(|| BackendError::Refused(format!("priority {p} not allowed")))?;
                let name = crate::process_name(*pid).unwrap_or_default();
                if apex_core::safety::is_critical_process(&name, *pid) {
                    return Err(BackendError::Refused(format!(
                        "{name} is a protected system process"
                    )));
                }
                set_priority(*pid, class)
            }
            (s, v) => Err(BackendError::Refused(format!("cannot set {s:?} to {v:?}"))),
        }
    }

    fn temp_roots(&self) -> Vec<PathBuf> {
        // %TEMP% for the current user (e.g. C:\Users\<you>\AppData\Local\Temp).
        // C:\Windows\Temp requires admin and is left to Windows' own Storage Sense.
        vec![std::env::temp_dir()]
    }
}
