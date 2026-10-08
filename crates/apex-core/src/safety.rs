//! Safety policy: what APEX must never touch or recommend touching.

/// Processes whose termination crashes, logs out, or destabilises Windows,
/// or disables security. APEX refuses to terminate these.
const CRITICAL_PROCESSES: &[&str] = &[
    "system",
    "registry",
    "idle",
    "system idle process",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "svchost.exe",
    "dwm.exe",
    "fontdrvhost.exe",
    "memory compression",
    "secure system",
    "spoolsv.exe",
    "sihost.exe",
    "ctfmon.exe",
    "explorer.exe",
    "msmpeng.exe",
    "nissrv.exe",
    "securityhealthservice.exe",
    "mpdefendercoreservice.exe",
    "wudfhost.exe",
    "audiodg.exe",
    "conhost.exe",
    "runtimebroker.exe",
    "taskhostw.exe",
    "startmenuexperiencehost.exe",
    "searchhost.exe",
    "shellexperiencehost.exe",
    "textinputhost.exe",
    "dllhost.exe",
    "lsm.exe",
    "mpcmdrun.exe",
    "smartscreen.exe",
    // Linux/macOS equivalents so the dev build is equally careful.
    "init",
    "systemd",
    "launchd",
    "kernel_task",
    "kthreadd",
];

/// Process names that are protective security software (never suggest stopping them).
const SECURITY_HINTS: &[&str] = &[
    "defender",
    "securityhealth",
    "msmpeng",
    "antivirus",
    "avast",
    "avg",
    "bitdefender",
    "kaspersky",
    "malwarebytes",
    "mbam",
    "norton",
    "mcafee",
    "eset",
    "sophos",
    "crowdstrike",
    "sentinel",
    "webroot",
    "trendmicro",
    "f-secure",
    "windows security",
];

/// Startup items that provide hardware functionality (audio, touchpad, GPU control,
/// keyboard hotkeys, accessibility). Never recommended for disabling.
const HARDWARE_HINTS: &[&str] = &[
    "realtek",
    "nvidia",
    "amd",
    "radeon",
    "intel",
    "synaptics",
    "elan",
    "alps",
    "igfx",
    "nahimic",
    "dolby",
    "waves",
    "maxx",
    "conexant",
    "hp hotkey",
    "lenovo",
    "asus",
    "msi",
    "dell",
    "logitech",
    "razer synapse",
    "corsair",
    "steelseries",
    "bluetooth",
    "touchpad",
    "pen",
    "wacom",
    "narrator",
    "magnify",
    "osk",
    "accessibility",
    "securityhealth",
];

pub fn is_critical_process(name: &str, pid: u32) -> bool {
    if pid <= 4 {
        return true;
    }
    let n = name.to_ascii_lowercase();
    CRITICAL_PROCESSES.iter().any(|c| *c == n) || is_security_software(&n)
}

pub fn is_security_software(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    SECURITY_HINTS.iter().any(|h| n.contains(h))
}

pub fn is_protected_startup(name: &str, command: &str) -> bool {
    let hay = format!("{} {}", name, command).to_ascii_lowercase();
    is_security_software(&hay) || HARDWARE_HINTS.iter().any(|h| hay.contains(h))
}

/// Applications that commonly add themselves to startup but work normally when
/// launched on demand. Matched as substrings of name or command (lower case).
/// (pattern, explanation)
pub const ON_DEMAND_APPS: &[(&str, &str)] = &[
    ("steam", "Steam works normally when you open it yourself; at startup it loads its client and web helper."),
    ("epicgameslauncher", "Epic Games Launcher can be opened when you want to play."),
    ("eadesktop", "EA app can be opened when you want to play."),
    ("battle.net", "Battle.net can be opened when you want to play."),
    ("ubisoft connect", "Ubisoft Connect can be opened when you want to play."),
    ("upc.exe", "Ubisoft Connect can be opened when you want to play."),
    ("galaxyclient", "GOG Galaxy can be opened when you want to play."),
    ("discord", "Discord works normally when opened manually; you won't get messages until you open it."),
    ("spotify", "Spotify works normally when opened manually."),
    ("teams", "Microsoft Teams works when opened manually; you won't get calls/messages until you open it."),
    ("slack", "Slack works when opened manually; you won't get messages until you open it."),
    ("zoom", "Zoom works when you join a meeting."),
    ("skype", "Skype works when opened manually."),
    ("adobe creative cloud", "Creative Cloud apps still launch; sync/updates wait until you open Creative Cloud."),
    ("ccxprocess", "Adobe helper; Creative Cloud apps still work without it at startup."),
    ("acrotray", "Adobe Acrobat tray helper; Acrobat still works when opened."),
    ("cortana", "Cortana can be opened manually."),
    ("itunes", "iTunes helper; iTunes still works when opened."),
    ("googledrivesync", "Drive sync pauses until you open it."),
    ("dropbox", "Dropbox sync pauses until you open it."),
    ("whatsapp", "WhatsApp works when opened manually."),
    ("telegram", "Telegram works when opened manually."),
    ("opera", "Opera browser assistant; the browser still works."),
    ("msedge", "Edge startup boost preloads Edge; Edge still works without it."),
    ("update", "Update helpers usually also run on a schedule or when the app starts."),
];

pub fn on_demand_reason(name: &str, command: &str) -> Option<&'static str> {
    let hay = format!("{} {}", name, command).to_ascii_lowercase();
    ON_DEMAND_APPS
        .iter()
        .find(|(p, _)| hay.contains(p))
        .map(|(_, why)| *why)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn critical_processes_are_protected() {
        assert!(is_critical_process("csrss.exe", 600));
        assert!(is_critical_process("LSASS.EXE", 700));
        assert!(is_critical_process("MsMpEng.exe", 900));
        assert!(is_critical_process("anything", 4));
        assert!(!is_critical_process("notepad.exe", 1234));
    }

    #[test]
    fn hardware_and_security_startups_are_protected() {
        assert!(is_protected_startup(
            "RtkAudUService",
            r"C:\Windows\System32\Realtek\RtkAudUService64.exe"
        ));
        assert!(is_protected_startup(
            "SecurityHealth",
            r"%windir%\system32\SecurityHealthSystray.exe"
        ));
        assert!(!is_protected_startup(
            "Steam",
            r#""C:\Program Files (x86)\Steam\steam.exe" -silent"#
        ));
        assert!(on_demand_reason("Steam", "steam.exe -silent").is_some());
    }
}
