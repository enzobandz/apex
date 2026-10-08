//! System-modifying actions and the backend trait that performs them.
//!
//! Every reversible action is expressed as "set setting X to value Y". The
//! executor reads X first, stores the original value in the ledger, writes Y,
//! then reads X again to verify. Undo writes the stored original value back.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

use crate::storage::CleanupReport;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    /// Activate an existing power scheme (`powercfg /setactive`).
    #[serde(rename_all = "camelCase")]
    SetPowerScheme { guid: String, name: String },
    /// Enable/disable a startup entry using the same `StartupApproved` mechanism as Task Manager.
    #[serde(rename_all = "camelCase")]
    SetStartupEntryEnabled {
        entry_id: String,
        name: String,
        enabled: bool,
    },
    /// HKCU\Software\Microsoft\GameBar\AutoGameModeEnabled
    #[serde(rename_all = "camelCase")]
    SetGameMode { enabled: bool },
    /// Change a running process's priority class. Reversible while the process runs;
    /// the change ends automatically when the process exits. Realtime is never allowed.
    #[serde(rename_all = "camelCase")]
    SetProcessPriority {
        pid: u32,
        name: String,
        priority: PriorityClass,
    },
    /// Delete files in the user's temp folder that are older than N days. NOT reversible.
    #[serde(rename_all = "camelCase")]
    CleanTempFiles { older_than_days: u32 },
}

/// A setting that can be read, written and restored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Setting {
    ActivePowerScheme,
    #[serde(rename_all = "camelCase")]
    StartupEntryEnabled {
        entry_id: String,
    },
    GameMode,
    #[serde(rename_all = "camelCase")]
    ProcessPriority {
        pid: u32,
    },
}

/// Windows priority classes APEX allows. `Realtime` is deliberately absent:
/// it can starve input and system threads and freeze the machine.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PriorityClass {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
}

impl PriorityClass {
    pub fn as_str(self) -> &'static str {
        match self {
            PriorityClass::Idle => "idle",
            PriorityClass::BelowNormal => "belowNormal",
            PriorityClass::Normal => "normal",
            PriorityClass::AboveNormal => "aboveNormal",
            PriorityClass::High => "high",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "idle" => PriorityClass::Idle,
            "belowNormal" => PriorityClass::BelowNormal,
            "normal" => PriorityClass::Normal,
            "aboveNormal" => PriorityClass::AboveNormal,
            "high" => PriorityClass::High,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum SettingValue {
    Text(String),
    Bool(bool),
    /// The setting did not exist (e.g. a registry value that was never written).
    /// Restoring `Absent` deletes the value again so the system is left exactly as found.
    Absent,
}

impl Action {
    /// The setting this action changes and the value it sets, for reversible actions.
    pub fn target(&self) -> Option<(Setting, SettingValue)> {
        match self {
            Action::SetPowerScheme { guid, .. } => Some((
                Setting::ActivePowerScheme,
                SettingValue::Text(guid.to_ascii_lowercase()),
            )),
            Action::SetStartupEntryEnabled {
                entry_id, enabled, ..
            } => Some((
                Setting::StartupEntryEnabled {
                    entry_id: entry_id.clone(),
                },
                SettingValue::Bool(*enabled),
            )),
            Action::SetGameMode { enabled } => {
                Some((Setting::GameMode, SettingValue::Bool(*enabled)))
            }
            Action::SetProcessPriority { pid, priority, .. } => Some((
                Setting::ProcessPriority { pid: *pid },
                SettingValue::Text(priority.as_str().into()),
            )),
            Action::CleanTempFiles { .. } => None,
        }
    }

    pub fn is_reversible(&self) -> bool {
        self.target().is_some()
    }

    pub fn describe(&self) -> String {
        match self {
            Action::SetPowerScheme { name, .. } => format!("Switch power plan to \"{name}\""),
            Action::SetStartupEntryEnabled { name, enabled, .. } => {
                if *enabled {
                    format!("Enable startup entry \"{name}\"")
                } else {
                    format!("Disable startup entry \"{name}\"")
                }
            }
            Action::SetGameMode { enabled } => {
                format!(
                    "Turn Windows Game Mode {}",
                    if *enabled { "on" } else { "off" }
                )
            }
            Action::SetProcessPriority {
                pid,
                name,
                priority,
            } => {
                format!(
                    "Set priority of {name} (PID {pid}) to {}",
                    priority.as_str()
                )
            }
            Action::CleanTempFiles { older_than_days } => {
                format!("Delete temporary files older than {older_than_days} days from your user Temp folder")
            }
        }
    }
}

#[derive(Debug, Error, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum BackendError {
    #[error("not supported on this system: {0}")]
    Unsupported(String),
    #[error("administrator rights are required: {0}")]
    RequiresAdmin(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("refused for safety: {0}")]
    Refused(String),
    #[error("operating-system error: {0}")]
    Os(String),
}

/// Platform implementation of system reads/writes. Windows: `apex_platform::windows::WindowsBackend`.
pub trait SystemBackend: Send + Sync {
    fn read(&self, setting: &Setting) -> Result<SettingValue, BackendError>;
    fn write(&self, setting: &Setting, value: &SettingValue) -> Result<(), BackendError>;
    /// Temp directories that `CleanTempFiles` is allowed to touch.
    fn temp_roots(&self) -> Vec<PathBuf>;
    fn clean_temp(&self, older_than_days: u32) -> Result<CleanupReport, BackendError> {
        let roots = self.temp_roots();
        if roots.is_empty() {
            return Err(BackendError::Unsupported("no temp directory found".into()));
        }
        crate::storage::clean_temp(&roots, older_than_days)
            .map_err(|e| BackendError::Refused(e.to_string()))
    }
}
