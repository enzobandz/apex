//! Platform layer: real measurements and the system-change backend.

pub mod benchrunner;
pub mod metrics;
pub mod powercfg_parse;
pub mod startup_util;
#[cfg(windows)]
pub mod windows;

use apex_core::actions::{BackendError, Setting, SettingValue, SystemBackend};
use apex_core::model::{StartupEntry, SystemSnapshot};
use apex_core::storage;
use std::path::PathBuf;

pub use metrics::Monitor;

/// Backend for non-Windows builds (development/CI). It never pretends to change
/// anything: every system setting reports `Unsupported`.
pub struct UnsupportedBackend;

impl SystemBackend for UnsupportedBackend {
    fn read(&self, s: &Setting) -> Result<SettingValue, BackendError> {
        Err(BackendError::Unsupported(format!(
            "{s:?} is only available on Windows"
        )))
    }
    fn write(&self, s: &Setting, _v: &SettingValue) -> Result<(), BackendError> {
        Err(BackendError::Unsupported(format!(
            "{s:?} is only available on Windows"
        )))
    }
    fn temp_roots(&self) -> Vec<PathBuf> {
        vec![std::env::temp_dir()]
    }
}

pub fn backend() -> Box<dyn SystemBackend> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsBackend)
    }
    #[cfg(not(windows))]
    {
        Box::new(UnsupportedBackend)
    }
}

pub fn process_name(pid: u32) -> Option<String> {
    let mut sys = sysinfo::System::new();
    let p = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
    sys.process(p)
        .map(|p| p.name().to_string_lossy().into_owned())
}

pub fn startup_entries() -> (Vec<StartupEntry>, Vec<String>) {
    #[cfg(windows)]
    {
        windows::startup::enumerate()
    }
    #[cfg(not(windows))]
    {
        (
            vec![],
            vec!["Startup entry management is only available on Windows.".into()],
        )
    }
}

/// Full diagnostic snapshot: measured values only, with explicit limitations.
pub fn collect_snapshot(monitor: &mut Monitor, backend: &dyn SystemBackend) -> SystemSnapshot {
    let mut s = monitor.base_snapshot();
    let roots = backend.temp_roots();
    s.temp_cleanable_bytes = Some(storage::analyze_temp(&roots, 7).eligible_bytes);
    let (startup, problems) = startup_entries();
    s.startup = startup;
    s.limitations.extend(problems);

    #[cfg(windows)]
    {
        if let Some((name, display, build)) = windows::os_details() {
            s.os.name = name;
            if display.is_some() {
                s.os.version = display;
            }
            s.os.build = build;
        }
        s.gpus = windows::gpus();
        match windows::power_info() {
            Ok(p) => s.power = Some(p),
            Err(e) => s
                .limitations
                .push(format!("Power plan information unavailable: {e}")),
        }
        s.game_mode_enabled = match windows::game_mode() {
            Ok(SettingValue::Bool(b)) => Some(b),
            Ok(SettingValue::Absent) => Some(true), // Windows default is on
            _ => None,
        };
        s.limitations.push("Startup publisher names and digital signatures are not yet verified (planned: WinVerifyTrust).".into());
        s.limitations
            .push("Startup-folder shortcuts are not resolved to their target program.".into());
    }
    #[cfg(not(windows))]
    {
        s.limitations.push("Running outside Windows: power plans, Game Mode, GPUs and startup entries are not available, and APEX will not change system settings.".into());
    }
    if s.gpus.is_empty() {
        s.limitations.push("No GPU information available.".into());
    }
    s
}

/// Gaming-relevant status. Every field is measured; unknowns stay `None`.
pub fn gaming_status() -> apex_core::model::GamingStatus {
    #[cfg(windows)]
    {
        let mut limitations = vec![
            "Live FPS / frame-time capture is not available yet: it requires Event Tracing (PresentMon-style) and administrator or Performance Log Users rights.".to_string(),
        ];
        let power = match windows::power_info() {
            Ok(p) => Some(p),
            Err(e) => {
                limitations.push(format!("Power plan unavailable: {e}"));
                None
            }
        };
        apex_core::model::GamingStatus {
            game_mode_enabled: match windows::game_mode() {
                Ok(SettingValue::Bool(b)) => Some(b),
                Ok(SettingValue::Absent) => Some(true),
                _ => None,
            },
            hardware_gpu_scheduling: windows::display::hardware_gpu_scheduling(),
            displays: windows::display::displays(),
            power,
            limitations,
        }
    }
    #[cfg(not(windows))]
    {
        apex_core::model::GamingStatus {
            game_mode_enabled: None,
            hardware_gpu_scheduling: None,
            displays: vec![],
            power: None,
            limitations: vec!["Gaming diagnostics are only available on Windows.".into()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_windows_backend_never_claims_success() {
        let b = UnsupportedBackend;
        assert!(b.read(&Setting::GameMode).is_err());
        assert!(b
            .write(&Setting::GameMode, &SettingValue::Bool(true))
            .is_err());
    }

    #[test]
    fn snapshot_reports_limitations_honestly() {
        let mut m = Monitor::new();
        let b = backend();
        let s = collect_snapshot(&mut m, b.as_ref());
        assert!(s.temp_cleanable_bytes.is_some());
        #[cfg(not(windows))]
        assert!(s.limitations.iter().any(|l| l.contains("outside Windows")));
    }
}
