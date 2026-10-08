//! Data model shared by the engine, the platform layer and the UI.
//!
//! Every field that a platform cannot measure reliably is an `Option`, so the
//! UI can show "not available" instead of a made-up number.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OsInfo {
    pub name: String,
    pub version: Option<String>,
    /// Windows build number (e.g. 22631) when known.
    pub build: Option<u32>,
    pub kernel: Option<String>,
    pub hostname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    pub brand: String,
    pub vendor: String,
    pub arch: String,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    /// Current reported frequency of the first core in MHz.
    pub frequency_mhz: Option<u64>,
    /// System-wide utilisation, averaged over the sampling window.
    pub usage_percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

impl MemoryInfo {
    pub fn used_fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        1.0 - (self.available_bytes as f64 / self.total_bytes as f64)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DiskKind {
    Ssd,
    Hdd,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub kind: DiskKind,
    pub file_system: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub removable: bool,
    /// True for the volume that hosts the operating system.
    pub is_system: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub driver_version: Option<String>,
    pub dedicated_memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PowerScheme {
    pub guid: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerInfo {
    pub active: PowerScheme,
    pub available: Vec<PowerScheme>,
    /// `Some(true)` when running on battery, `None` when unknown / desktop.
    pub on_battery: Option<bool>,
    pub battery_present: Option<bool>,
}

/// Well-known built-in Windows power scheme GUIDs (documented by Microsoft, `powercfg /list`).
pub mod power_guids {
    pub const POWER_SAVER: &str = "a1841308-3541-4fab-bc81-f71556f20b4a";
    pub const BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
    pub const HIGH_PERFORMANCE: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
    pub const ULTIMATE_PERFORMANCE: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum StartupLocation {
    /// HKCU\Software\Microsoft\Windows\CurrentVersion\Run
    UserRun,
    /// HKLM\Software\Microsoft\Windows\CurrentVersion\Run
    MachineRun,
    /// HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run
    MachineRun32,
    /// %APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup
    UserStartupFolder,
    /// %ProgramData%\Microsoft\Windows\Start Menu\Programs\Startup
    CommonStartupFolder,
}

impl StartupLocation {
    pub fn requires_admin(self) -> bool {
        matches!(
            self,
            StartupLocation::MachineRun
                | StartupLocation::MachineRun32
                | StartupLocation::CommonStartupFolder
        )
    }

    pub fn describe(self) -> &'static str {
        match self {
            StartupLocation::UserRun => r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            StartupLocation::MachineRun => r"HKLM\Software\Microsoft\Windows\CurrentVersion\Run",
            StartupLocation::MachineRun32 => {
                r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run"
            }
            StartupLocation::UserStartupFolder => {
                r"%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup"
            }
            StartupLocation::CommonStartupFolder => {
                r"%ProgramData%\Microsoft\Windows\Start Menu\Programs\Startup"
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupEntry {
    /// Stable identifier: `<location>:<value or file name>`.
    pub id: String,
    pub name: String,
    pub command: String,
    pub location: StartupLocation,
    pub enabled: bool,
    pub executable_path: Option<String>,
    pub executable_exists: Option<bool>,
    pub publisher: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub name: String,
    pub exe: Option<String>,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub virtual_memory_bytes: u64,
    pub disk_read_bytes_per_sec: u64,
    pub disk_write_bytes_per_sec: u64,
    pub status: String,
    pub run_time_secs: u64,
    pub critical: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemSnapshot {
    pub captured_at: DateTime<Utc>,
    pub os: OsInfo,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub disks: Vec<DiskInfo>,
    pub gpus: Vec<GpuInfo>,
    pub power: Option<PowerInfo>,
    pub game_mode_enabled: Option<bool>,
    pub startup: Vec<StartupEntry>,
    pub top_processes: Vec<ProcessInfo>,
    /// Bytes in the user's temp folder that are eligible for cleanup (older than the default age).
    pub temp_cleanable_bytes: Option<u64>,
    /// Facts the platform layer could not determine, surfaced to the user verbatim.
    pub limitations: Vec<String>,
}

impl SystemSnapshot {
    pub fn system_disk(&self) -> Option<&DiskInfo> {
        self.disks.iter().find(|d| d.is_system)
    }
}

/// Lightweight sample used by the live dashboard (cheap to collect every second).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSample {
    pub timestamp_ms: i64,
    pub cpu_percent: f32,
    pub per_core_percent: Vec<f32>,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub net_rx_bytes_per_sec: u64,
    pub net_tx_bytes_per_sec: u64,
    pub disk_read_bytes_per_sec: u64,
    pub disk_write_bytes_per_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub device: String,
    pub adapter: String,
    pub primary: bool,
    pub width: u32,
    pub height: u32,
    pub current_hz: u32,
    /// Highest refresh rate the driver offers at the current resolution.
    pub max_hz_at_current_resolution: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamingStatus {
    pub game_mode_enabled: Option<bool>,
    /// Hardware-accelerated GPU scheduling (HwSchMode: 2 = on, 1 = off).
    pub hardware_gpu_scheduling: Option<bool>,
    pub displays: Vec<DisplayInfo>,
    pub power: Option<PowerInfo>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetInterface {
    pub name: String,
    pub mac: String,
    pub addresses: Vec<String>,
    pub mtu: u64,
    pub state: String,
    pub total_received_bytes: u64,
    pub total_transmitted_bytes: u64,
    pub total_errors: u64,
}
