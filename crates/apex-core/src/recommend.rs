//! Deterministic, hardware-aware recommendation engine.
//!
//! Rules only fire on measured facts from the snapshot. Each recommendation states
//! its category, confidence, risk, expected benefit, downsides and rollback path.
//! Expected benefits are worded as *expected*, never as measured: measurement is
//! the Benchmark Lab's job.

use serde::{Deserialize, Serialize};

use crate::actions::Action;
use crate::model::{power_guids, DiskKind, SystemSnapshot};
use crate::safety;
use crate::storage::human_bytes;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Balanced,
    Gaming,
    MaximumPerformance,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    /// A measured problem (e.g. system drive nearly full).
    VerifiedProblem,
    /// Measured signal that is likely, but not certainly, hurting performance.
    LikelyBottleneck,
    /// Reasonable change whose value depends on how the user uses the PC.
    Preference,
    /// Plausible but weakly evidenced; only shown in Maximum Performance mode.
    Experimental,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub id: String,
    pub title: String,
    pub category: Category,
    /// 0–100: how confident APEX is that acting on this helps *this* PC.
    pub confidence: u8,
    pub risk: Risk,
    pub expected_benefit: String,
    pub downsides: String,
    /// The measured facts this recommendation is based on.
    pub evidence: Vec<String>,
    pub requires_admin: bool,
    pub reversible: bool,
    pub rollback: String,
    /// `None` = advice only (APEX will not change anything for this item).
    pub action: Option<Action>,
    /// Must be explicitly ticked by the user; never pre-selected.
    pub requires_confirmation: bool,
    /// Pre-selected in the one-click plan.
    pub selected_by_default: bool,
}

const GIB: u64 = 1024 * 1024 * 1024;

pub fn recommend(s: &SystemSnapshot, mode: Mode) -> Vec<Recommendation> {
    let mut out = Vec::new();
    disk_rules(s, &mut out);
    memory_rules(s, &mut out);
    cpu_rules(s, &mut out);
    power_rules(s, mode, &mut out);
    game_mode_rules(s, mode, &mut out);
    startup_rules(s, mode, &mut out);

    if mode != Mode::MaximumPerformance {
        out.retain(|r| r.category != Category::Experimental);
    }
    out.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then(b.confidence.cmp(&a.confidence))
            .then(a.risk.cmp(&b.risk))
    });
    out
}

fn disk_rules(s: &SystemSnapshot, out: &mut Vec<Recommendation>) {
    let Some(sys) = s.system_disk() else { return };
    if sys.total_bytes == 0 {
        return;
    }
    let free_frac = sys.available_bytes as f64 / sys.total_bytes as f64;
    let low = free_frac < 0.10 || sys.available_bytes < 15 * GIB;
    if low {
        out.push(Recommendation {
            id: "disk.system-low-space".into(),
            title: format!("System drive {} is low on free space", sys.mount_point),
            category: Category::VerifiedProblem,
            confidence: 90,
            risk: Risk::Low,
            expected_benefit: "Windows needs free space for updates, the page file, hibernation and app caches. Very low free space causes failed updates and can slow the system.".into(),
            downsides: "Freeing space requires removing files; review Storage Cleaner before deleting anything personal.".into(),
            evidence: vec![format!(
                "{} free of {} ({:.1}%)",
                human_bytes(sys.available_bytes),
                human_bytes(sys.total_bytes),
                free_frac * 100.0
            )],
            requires_admin: false,
            reversible: true,
            rollback: "Nothing is changed by this item itself.".into(),
            action: None,
            requires_confirmation: false,
            selected_by_default: false,
        });
    }
    if let Some(bytes) = s.temp_cleanable_bytes {
        if bytes >= 500 * 1024 * 1024 || (low && bytes >= 50 * 1024 * 1024) {
            out.push(Recommendation {
                id: "storage.clean-temp".into(),
                title: format!("Remove {} of old temporary files", human_bytes(bytes)),
                category: if low { Category::VerifiedProblem } else { Category::Preference },
                confidence: if low { 85 } else { 60 },
                risk: Risk::Low,
                expected_benefit: format!("Frees about {}. Only files in your user Temp folder that have not changed for 7+ days are removed.", human_bytes(bytes)),
                downsides: "Deleted temp files cannot be restored. Files in use are skipped automatically. This frees space; it does not make the PC faster unless the drive is nearly full.".into(),
                evidence: vec![format!("{} in temp files older than 7 days", human_bytes(bytes))],
                requires_admin: false,
                reversible: false,
                rollback: "Not reversible. Temporary files are recreated by apps as needed.".into(),
                action: Some(Action::CleanTempFiles { older_than_days: 7 }),
                requires_confirmation: true,
                selected_by_default: false,
            });
        }
    }
    if sys.kind == DiskKind::Hdd {
        out.push(Recommendation {
            id: "disk.system-hdd".into(),
            title: "Windows is installed on a hard disk drive".into(),
            category: Category::LikelyBottleneck,
            confidence: 85,
            risk: Risk::Low,
            expected_benefit: "Moving Windows to an SSD is typically the single largest improvement to boot time and app launch time on an HDD-based PC. No software setting can match it.".into(),
            downsides: "Requires buying an SSD and migrating or reinstalling Windows.".into(),
            evidence: vec![format!("{} reported as rotational (HDD)", sys.mount_point)],
            requires_admin: false,
            reversible: true,
            rollback: "Advice only; APEX changes nothing.".into(),
            action: None,
            requires_confirmation: false,
            selected_by_default: false,
        });
    }
}

fn memory_rules(s: &SystemSnapshot, out: &mut Vec<Recommendation>) {
    let m = &s.memory;
    if m.total_bytes == 0 {
        return;
    }
    let used = m.used_fraction();
    if used >= 0.85 {
        let top: Vec<String> = {
            let mut p = s.top_processes.clone();
            p.sort_by_key(|x| std::cmp::Reverse(x.memory_bytes));
            p.iter()
                .take(5)
                .map(|p| format!("{} — {}", p.name, human_bytes(p.memory_bytes)))
                .collect()
        };
        let mut evidence = vec![format!(
            "{:.0}% of {} in use; {} available",
            used * 100.0,
            human_bytes(m.total_bytes),
            human_bytes(m.available_bytes)
        )];
        evidence.extend(top.into_iter().map(|t| format!("Largest: {t}")));
        out.push(Recommendation {
            id: "memory.high-pressure".into(),
            title: "Memory is nearly full".into(),
            category: Category::LikelyBottleneck,
            confidence: 70,
            risk: Risk::Low,
            expected_benefit: "When RAM runs out Windows pages to disk, causing stutters and slow app switching. Closing the large apps listed, or reducing startup apps, relieves this.".into(),
            downsides: "This is a snapshot; check Performance Monitor to see whether it is sustained.".into(),
            evidence,
            requires_admin: false,
            reversible: true,
            rollback: "Advice only. APEX does not \"purge\" RAM — that makes numbers look better while making apps reload from disk.".into(),
            action: None,
            requires_confirmation: false,
            selected_by_default: false,
        });
    }
    if m.total_bytes < 8 * GIB - GIB / 2 {
        out.push(Recommendation {
            id: "memory.low-capacity".into(),
            title: "Installed memory is below 8 GB".into(),
            category: Category::LikelyBottleneck,
            confidence: 65,
            risk: Risk::Low,
            expected_benefit: "Windows 11 with a browser and a game commonly needs more than 8 GB; adding RAM is often the most effective upgrade for such systems.".into(),
            downsides: "Hardware purchase; check your motherboard's supported memory first.".into(),
            evidence: vec![format!("{} installed", human_bytes(m.total_bytes))],
            requires_admin: false,
            reversible: true,
            rollback: "Advice only.".into(),
            action: None,
            requires_confirmation: false,
            selected_by_default: false,
        });
    }
}

fn cpu_rules(s: &SystemSnapshot, out: &mut Vec<Recommendation>) {
    let cores = s.cpu.logical_cores.max(1) as f32;
    // sysinfo reports per-process CPU where 100% = one full core.
    let heavy: Vec<_> = s
        .top_processes
        .iter()
        .filter(|p| !p.critical && p.cpu_percent / cores >= 15.0)
        .collect();
    if !heavy.is_empty() {
        out.push(Recommendation {
            id: "cpu.background-load".into(),
            title: "Processes using a large share of the CPU right now".into(),
            category: Category::LikelyBottleneck,
            confidence: 50,
            risk: Risk::Low,
            expected_benefit: "If these are not apps you are actively using, closing them frees CPU time for what you are doing.".into(),
            downsides: "Measured over a short window; a brief spike is normal (e.g. during updates or indexing). Use Process Explorer to watch over time.".into(),
            evidence: heavy
                .iter()
                .map(|p| format!("{} (PID {}) — {:.0}% of total CPU", p.name, p.pid, p.cpu_percent / cores))
                .collect(),
            requires_admin: false,
            reversible: true,
            rollback: "Advice only; APEX never closes processes automatically.".into(),
            action: None,
            requires_confirmation: false,
            selected_by_default: false,
        });
    }
}

fn power_rules(s: &SystemSnapshot, mode: Mode, out: &mut Vec<Recommendation>) {
    let Some(p) = &s.power else { return };
    let active = p.active.guid.to_ascii_lowercase();
    let find = |g: &str| p.available.iter().find(|x| x.guid.eq_ignore_ascii_case(g));
    let on_battery = p.on_battery == Some(true);
    let laptop = p.battery_present == Some(true);

    if active == power_guids::POWER_SAVER {
        if let Some(bal) = find(power_guids::BALANCED) {
            out.push(Recommendation {
                id: "power.leave-power-saver".into(),
                title: "Power saver plan is limiting performance".into(),
                category: if on_battery { Category::Preference } else { Category::LikelyBottleneck },
                confidence: if on_battery { 40 } else { 75 },
                risk: Risk::Low,
                expected_benefit: "Power saver caps processor performance and display brightness. Balanced lets the CPU boost when needed and still idles efficiently.".into(),
                downsides: if laptop { "Shorter battery life than Power saver.".into() } else { "Slightly higher power use under load.".into() },
                evidence: vec![format!("Active plan: {}", p.active.name)],
                requires_admin: false,
                reversible: true,
                rollback: format!("Undo restores \"{}\".", p.active.name),
                action: Some(Action::SetPowerScheme { guid: bal.guid.clone(), name: bal.name.clone() }),
                requires_confirmation: false,
                selected_by_default: !on_battery,
            });
        }
    }

    if matches!(mode, Mode::Gaming | Mode::MaximumPerformance)
        && active != power_guids::HIGH_PERFORMANCE
        && active != power_guids::ULTIMATE_PERFORMANCE
    {
        if let Some(hp) = find(power_guids::HIGH_PERFORMANCE) {
            out.push(Recommendation {
                id: "power.high-performance".into(),
                title: "Use the High performance power plan while gaming".into(),
                category: Category::Preference,
                confidence: if laptop { 30 } else { 40 },
                risk: Risk::Low,
                expected_benefit: "Keeps the CPU at higher minimum performance states, which can reduce clock ramp-up delay. On modern CPUs the measured benefit is often small; use Benchmark Lab before/after to check on your PC.".into(),
                downsides: format!(
                    "Higher idle power draw, heat and fan noise{}.",
                    if laptop { " and noticeably shorter battery life" } else { "" }
                ),
                evidence: vec![format!("Active plan: {}", p.active.name), "High performance plan is installed".into()],
                requires_admin: false,
                reversible: true,
                rollback: format!("Undo restores \"{}\".", p.active.name),
                action: Some(Action::SetPowerScheme { guid: hp.guid.clone(), name: hp.name.clone() }),
                requires_confirmation: laptop,
                selected_by_default: false,
            });
        }
    }

    if mode == Mode::MaximumPerformance && active != power_guids::ULTIMATE_PERFORMANCE {
        if let Some(up) = find(power_guids::ULTIMATE_PERFORMANCE) {
            out.push(Recommendation {
                id: "power.ultimate".into(),
                title: "Use the Ultimate Performance power plan".into(),
                category: Category::Experimental,
                confidence: 20,
                risk: Risk::Medium,
                expected_benefit: "Designed by Microsoft for workstation workloads; removes most power-saving micro-latencies. Gains for games are usually within measurement noise.".into(),
                downsides: "Highest power draw and heat; prevents deep idle states. Not intended for laptops on battery.".into(),
                evidence: vec!["Ultimate Performance plan is already installed".into()],
                requires_admin: false,
                reversible: true,
                rollback: format!("Undo restores \"{}\".", p.active.name),
                action: Some(Action::SetPowerScheme { guid: up.guid.clone(), name: up.name.clone() }),
                requires_confirmation: true,
                selected_by_default: false,
            });
        }
    }

    if matches!(mode, Mode::Gaming | Mode::MaximumPerformance)
        && p.available.len() == 1
        && active == power_guids::BALANCED
    {
        out.push(Recommendation {
            id: "power.modern-standby-note".into(),
            title: "Use Windows' Power mode slider for best performance".into(),
            category: Category::Preference,
            confidence: 50,
            risk: Risk::Low,
            expected_benefit: "This PC exposes only the Balanced plan (common with Modern Standby). Settings › System › Power › Power mode › Best performance is the supported way to raise performance here.".into(),
            downsides: "More heat and power use while set to Best performance.".into(),
            evidence: vec!["Only one power plan available".into()],
            requires_admin: false,
            reversible: true,
            rollback: "Advice only; change it back in Settings.".into(),
            action: None,
            requires_confirmation: false,
            selected_by_default: false,
        });
    }
}

fn game_mode_rules(s: &SystemSnapshot, mode: Mode, out: &mut Vec<Recommendation>) {
    if s.game_mode_enabled == Some(false) && matches!(mode, Mode::Gaming | Mode::MaximumPerformance)
    {
        out.push(Recommendation {
            id: "gaming.enable-game-mode".into(),
            title: "Turn Windows Game Mode back on".into(),
            category: Category::Preference,
            confidence: 55,
            risk: Risk::Low,
            expected_benefit: "Microsoft documents that Game Mode stops Windows Update from installing drivers and showing restart notifications during play, and aims for a more stable frame rate depending on the game and system. It is on by default.".into(),
            downsides: "Rarely, specific games behave worse with it; turn it off again if you measure a regression.".into(),
            evidence: vec!["Game Mode is currently off".into()],
            requires_admin: false,
            reversible: true,
            rollback: "Undo turns Game Mode off again.".into(),
            action: Some(Action::SetGameMode { enabled: true }),
            requires_confirmation: false,
            selected_by_default: true,
        });
    }
}

fn startup_rules(s: &SystemSnapshot, mode: Mode, out: &mut Vec<Recommendation>) {
    let enabled: Vec<_> = s.startup.iter().filter(|e| e.enabled).collect();
    for e in &enabled {
        if safety::is_protected_startup(&e.name, &e.command) {
            continue;
        }
        let missing = e.executable_exists == Some(false);
        let reason = if missing {
            Some("The program this entry launches no longer exists, so it does nothing except produce an error at sign-in.")
        } else {
            safety::on_demand_reason(&e.name, &e.command)
        };
        let Some(reason) = reason else { continue };
        let generic = !missing && reason.starts_with("Update helpers");
        let admin = e.location.requires_admin();
        out.push(Recommendation {
            id: format!("startup.disable.{}", e.id),
            title: if missing {
                format!("Remove broken startup entry \"{}\"", e.name)
            } else {
                format!("Don't start \"{}\" with Windows", e.name)
            },
            category: if missing { Category::VerifiedProblem } else { Category::Preference },
            confidence: if missing { 90 } else if generic { 40 } else { 65 },
            risk: Risk::Low,
            expected_benefit: format!(
                "{reason} Fewer startup apps shortens the time until the desktop is responsive and frees memory{}.",
                if mode == Mode::Gaming { " for games" } else { "" }
            ),
            downsides: "The app will not run in the background until you open it.".into(),
            evidence: vec![
                format!("Location: {}", e.location.describe()),
                format!("Command: {}", e.command),
            ],
            requires_admin: admin,
            reversible: true,
            rollback: "Disabled the same way as Task Manager › Startup apps; undo (or Task Manager) re-enables it. The entry itself is not deleted.".into(),
            action: Some(Action::SetStartupEntryEnabled { entry_id: e.id.clone(), name: e.name.clone(), enabled: false }),
            requires_confirmation: false,
            selected_by_default: !admin && !generic,
        });
    }

    // Redundant entries: same executable started from two places.
    let mut seen: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
    for e in &enabled {
        let Some(exe) = e.executable_path.as_ref().map(|p| p.to_ascii_lowercase()) else {
            continue;
        };
        if let Some(first) = seen.get(&exe) {
            out.push(Recommendation {
                id: format!("startup.duplicate.{}", e.id),
                title: format!("\"{}\" is started twice", e.name),
                category: Category::VerifiedProblem,
                confidence: 75,
                risk: Risk::Low,
                expected_benefit: format!("The same program is also launched by \"{first}\". One launch is enough."),
                downsides: "If the two entries pass different arguments, check the app still behaves as expected.".into(),
                evidence: vec![format!("Executable: {exe}"), format!("Location: {}", e.location.describe())],
                requires_admin: e.location.requires_admin(),
                reversible: true,
                rollback: "Undo re-enables the entry.".into(),
                action: Some(Action::SetStartupEntryEnabled { entry_id: e.id.clone(), name: e.name.clone(), enabled: false }),
                requires_confirmation: false,
                selected_by_default: false,
            });
        } else {
            seen.insert(exe, &e.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use chrono::Utc;

    fn snap() -> SystemSnapshot {
        SystemSnapshot {
            captured_at: Utc::now(),
            os: OsInfo {
                name: "Windows 11".into(),
                ..Default::default()
            },
            cpu: CpuInfo {
                logical_cores: 8,
                ..Default::default()
            },
            memory: MemoryInfo {
                total_bytes: 16 * GIB,
                used_bytes: 6 * GIB,
                available_bytes: 10 * GIB,
                ..Default::default()
            },
            disks: vec![DiskInfo {
                name: "C".into(),
                mount_point: "C:\\".into(),
                kind: DiskKind::Ssd,
                file_system: "NTFS".into(),
                total_bytes: 500 * GIB,
                available_bytes: 200 * GIB,
                removable: false,
                is_system: true,
            }],
            gpus: vec![],
            power: Some(PowerInfo {
                active: PowerScheme {
                    guid: power_guids::BALANCED.into(),
                    name: "Balanced".into(),
                },
                available: vec![
                    PowerScheme {
                        guid: power_guids::BALANCED.into(),
                        name: "Balanced".into(),
                    },
                    PowerScheme {
                        guid: power_guids::HIGH_PERFORMANCE.into(),
                        name: "High performance".into(),
                    },
                ],
                on_battery: Some(false),
                battery_present: Some(false),
            }),
            game_mode_enabled: Some(true),
            startup: vec![],
            top_processes: vec![],
            temp_cleanable_bytes: Some(0),
            limitations: vec![],
        }
    }

    fn entry(id: &str, name: &str, cmd: &str, exe: &str) -> StartupEntry {
        StartupEntry {
            id: id.into(),
            name: name.into(),
            command: cmd.into(),
            location: StartupLocation::UserRun,
            enabled: true,
            executable_path: Some(exe.into()),
            executable_exists: Some(true),
            publisher: None,
        }
    }

    #[test]
    fn healthy_pc_balanced_mode_has_no_actions() {
        let r = recommend(&snap(), Mode::Balanced);
        assert!(r.iter().all(|r| r.action.is_none()), "{r:#?}");
    }

    #[test]
    fn high_performance_only_offered_in_gaming_and_never_preselected() {
        let r = recommend(&snap(), Mode::Gaming);
        let hp = r.iter().find(|r| r.id == "power.high-performance").unwrap();
        assert!(!hp.selected_by_default);
        assert!(recommend(&snap(), Mode::Balanced)
            .iter()
            .all(|r| r.id != "power.high-performance"));
    }

    #[test]
    fn missing_plans_are_never_recommended() {
        let mut s = snap();
        s.power
            .as_mut()
            .unwrap()
            .available
            .retain(|p| p.guid == power_guids::BALANCED);
        let r = recommend(&s, Mode::MaximumPerformance);
        assert!(r
            .iter()
            .all(|r| !matches!(r.action, Some(Action::SetPowerScheme { .. }))));
        assert!(r.iter().any(|r| r.id == "power.modern-standby-note"));
    }

    #[test]
    fn startup_rules_skip_protected_and_flag_broken_and_duplicates() {
        let mut s = snap();
        s.startup = vec![
            entry(
                "u:Steam",
                "Steam",
                "steam.exe -silent",
                r"c:\steam\steam.exe",
            ),
            entry(
                "u:Realtek",
                "RtkAudUService",
                "rtk.exe",
                r"c:\realtek\rtk.exe",
            ),
            entry("u:Steam2", "SteamAgain", "steam.exe", r"C:\Steam\steam.exe"),
            StartupEntry {
                executable_exists: Some(false),
                ..entry("u:Gone", "OldTool", "gone.exe", r"c:\gone.exe")
            },
        ];
        let r = recommend(&s, Mode::Balanced);
        assert!(r.iter().any(|r| r.id == "startup.disable.u:Steam"));
        assert!(r.iter().all(|r| !r.id.contains("Realtek")));
        assert!(r.iter().any(|r| r.id == "startup.duplicate.u:Steam2"));
        let broken = r.iter().find(|r| r.id == "startup.disable.u:Gone").unwrap();
        assert_eq!(broken.category, Category::VerifiedProblem);
    }

    #[test]
    fn low_disk_and_temp_cleanup_require_confirmation() {
        let mut s = snap();
        s.disks[0].available_bytes = 5 * GIB;
        s.temp_cleanable_bytes = Some(2 * GIB);
        let r = recommend(&s, Mode::Balanced);
        assert!(r.iter().any(|r| r.id == "disk.system-low-space"));
        let t = r.iter().find(|r| r.id == "storage.clean-temp").unwrap();
        assert!(t.requires_confirmation && !t.selected_by_default && !t.reversible);
    }

    #[test]
    fn experimental_only_in_max_mode() {
        let mut s = snap();
        s.power.as_mut().unwrap().available.push(PowerScheme {
            guid: power_guids::ULTIMATE_PERFORMANCE.into(),
            name: "Ultimate".into(),
        });
        assert!(recommend(&s, Mode::Gaming)
            .iter()
            .all(|r| r.category != Category::Experimental));
        assert!(recommend(&s, Mode::MaximumPerformance)
            .iter()
            .any(|r| r.category == Category::Experimental));
    }
}
