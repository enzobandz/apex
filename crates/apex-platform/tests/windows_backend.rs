//! Integration tests against the real Windows APIs. They only compile and run on
//! Windows (CI: windows-latest). Each test restores everything it changes.
#![cfg(windows)]

use apex_core::actions::{Action, PriorityClass, Setting, SettingValue, SystemBackend};
use apex_core::executor::{Executor, Outcome};
use apex_core::ledger::{BatchStatus, Ledger};
use apex_core::model::StartupLocation;
use apex_platform::startup_util::make_id;
use apex_platform::windows::WindowsBackend;
use winreg::enums::*;
use winreg::RegKey;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const TEST_VALUE: &str = "APEX-IntegrationTest";

fn ledger() -> Ledger {
    Ledger::open_in_memory().unwrap()
}

#[test]
fn power_scheme_round_trip() {
    let b = WindowsBackend;
    let original = b
        .read(&Setting::ActivePowerScheme)
        .expect("read active scheme");
    let info = apex_platform::windows::power_info().expect("power info");
    let Some(other) = info
        .available
        .iter()
        .find(|s| SettingValue::Text(s.guid.clone()) != original)
    else {
        eprintln!("only one power scheme installed; skipping");
        return;
    };
    let l = ledger();
    let ex = Executor::new(&b, &l);
    let rep = ex
        .apply_plan(
            "test",
            &[Action::SetPowerScheme {
                guid: other.guid.clone(),
                name: other.name.clone(),
            }],
        )
        .unwrap();
    assert_eq!(rep.status, BatchStatus::Applied, "{rep:#?}");
    assert_eq!(
        b.read(&Setting::ActivePowerScheme).unwrap(),
        SettingValue::Text(other.guid.clone())
    );
    let undo = ex.undo_batch(&rep.batch_id, false).unwrap();
    assert!(undo.iter().all(|u| u.ok), "{undo:#?}");
    assert_eq!(b.read(&Setting::ActivePowerScheme).unwrap(), original);
}

#[test]
fn rejects_unknown_power_scheme() {
    let b = WindowsBackend;
    let err = b.write(
        &Setting::ActivePowerScheme,
        &SettingValue::Text("00000000-0000-0000-0000-000000000000".into()),
    );
    assert!(err.is_err());
    let err = b.write(
        &Setting::ActivePowerScheme,
        &SettingValue::Text("/hibernate off".into()),
    );
    assert!(err.is_err());
}

#[test]
fn game_mode_round_trip_restores_exact_original() {
    let b = WindowsBackend;
    let original = b.read(&Setting::GameMode).unwrap();
    let target = !matches!(original, SettingValue::Bool(true) | SettingValue::Absent);
    let l = ledger();
    let ex = Executor::new(&b, &l);
    let rep = ex
        .apply_plan("test", &[Action::SetGameMode { enabled: target }])
        .unwrap();
    if !matches!(rep.results[0].outcome, Outcome::Skipped(_)) {
        assert_eq!(rep.status, BatchStatus::Applied, "{rep:#?}");
        ex.undo_batch(&rep.batch_id, false).unwrap();
    }
    assert_eq!(
        b.read(&Setting::GameMode).unwrap(),
        original,
        "original value (including 'absent') must be restored"
    );
}

#[test]
fn startup_entry_disable_and_undo() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey(RUN).unwrap();
    run.set_value(
        TEST_VALUE,
        &r#""C:\Windows\System32\notepad.exe""#.to_string(),
    )
    .unwrap();
    let id = make_id(StartupLocation::UserRun, TEST_VALUE);

    let (entries, _) = apex_platform::startup_entries();
    let e = entries
        .iter()
        .find(|e| e.id == id)
        .expect("test entry enumerated");
    assert!(e.enabled);
    assert_eq!(e.executable_exists, Some(true));

    let b = WindowsBackend;
    let l = ledger();
    let ex = Executor::new(&b, &l);
    let rep = ex
        .apply_plan(
            "test",
            &[Action::SetStartupEntryEnabled {
                entry_id: id.clone(),
                name: TEST_VALUE.into(),
                enabled: false,
            }],
        )
        .unwrap();
    let after_disable = apex_platform::startup_entries()
        .0
        .into_iter()
        .find(|e| e.id == id)
        .unwrap();
    let undo = ex.undo_batch(&rep.batch_id, false).unwrap();
    let after_undo = b
        .read(&Setting::StartupEntryEnabled {
            entry_id: id.clone(),
        })
        .unwrap();

    // cleanup before asserting
    let _ = run.delete_value(TEST_VALUE);
    if let Ok(k) = hkcu.open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
        KEY_SET_VALUE,
    ) {
        let _ = k.delete_value(TEST_VALUE);
    }

    assert_eq!(rep.status, BatchStatus::Applied, "{rep:#?}");
    assert!(!after_disable.enabled);
    assert!(undo.iter().all(|u| u.ok));
    assert_eq!(after_undo, SettingValue::Absent);
}

#[test]
fn refuses_to_toggle_nonexistent_entry() {
    let b = WindowsBackend;
    let id = make_id(StartupLocation::UserRun, "APEX-does-not-exist-123");
    assert!(b
        .write(
            &Setting::StartupEntryEnabled { entry_id: id },
            &SettingValue::Bool(false)
        )
        .is_err());
}

#[test]
fn process_priority_round_trip() {
    let mut child = std::process::Command::new(r"C:\Windows\System32\ping.exe")
        .args(["-n", "30", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let b = WindowsBackend;
    let l = ledger();
    let ex = Executor::new(&b, &l);
    let rep = ex.apply_plan(
        "test",
        &[Action::SetProcessPriority {
            pid,
            name: "PING.EXE".into(),
            priority: PriorityClass::BelowNormal,
        }],
    );
    let read_mid = b.read(&Setting::ProcessPriority { pid });
    let undo = rep.as_ref().ok().map(|r| ex.undo_batch(&r.batch_id, false));
    let read_end = b.read(&Setting::ProcessPriority { pid });
    let _ = child.kill();
    let _ = child.wait();
    let rep = rep.unwrap();
    assert_eq!(rep.status, BatchStatus::Applied, "{rep:#?}");
    assert_eq!(read_mid.unwrap(), SettingValue::Text("belowNormal".into()));
    assert!(undo.unwrap().unwrap().iter().all(|u| u.ok));
    assert_eq!(read_end.unwrap(), SettingValue::Text("normal".into()));
}

#[test]
fn refuses_protected_process_priority() {
    let b = WindowsBackend;
    // csrss/lsass PIDs vary; PID 4 is always System.
    assert!(b
        .write(
            &Setting::ProcessPriority { pid: 4 },
            &SettingValue::Text("high".into())
        )
        .is_err());
}

#[test]
fn snapshot_on_windows_has_real_details() {
    let mut m = apex_platform::Monitor::new();
    let s = apex_platform::collect_snapshot(&mut m, &WindowsBackend);
    assert!(s.os.build.unwrap_or(0) >= 10240, "{:?}", s.os);
    assert!(s.power.is_some(), "{:?}", s.limitations);
    assert!(s.disks.iter().any(|d| d.is_system));
    let g = apex_platform::gaming_status();
    assert!(g.displays.iter().all(|d| d.width > 0) || g.displays.is_empty());
}
