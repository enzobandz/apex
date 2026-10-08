//! Platform-independent helpers for startup entries (ID encoding, command-line parsing).

use apex_core::actions::BackendError;
use apex_core::model::StartupLocation;

pub fn location_code(l: StartupLocation) -> &'static str {
    match l {
        StartupLocation::UserRun => "hkcu-run",
        StartupLocation::MachineRun => "hklm-run",
        StartupLocation::MachineRun32 => "hklm-run32",
        StartupLocation::UserStartupFolder => "user-folder",
        StartupLocation::CommonStartupFolder => "common-folder",
    }
}

fn parse_code(c: &str) -> Option<StartupLocation> {
    Some(match c {
        "hkcu-run" => StartupLocation::UserRun,
        "hklm-run" => StartupLocation::MachineRun,
        "hklm-run32" => StartupLocation::MachineRun32,
        "user-folder" => StartupLocation::UserStartupFolder,
        "common-folder" => StartupLocation::CommonStartupFolder,
        _ => return None,
    })
}

pub fn make_id(l: StartupLocation, value_name: &str) -> String {
    format!("{}|{}", location_code(l), value_name)
}

pub fn parse_id(id: &str) -> Result<(StartupLocation, String), BackendError> {
    let (code, name) = id
        .split_once('|')
        .ok_or_else(|| BackendError::NotFound(format!("malformed startup id {id}")))?;
    let loc = parse_code(code)
        .ok_or_else(|| BackendError::NotFound(format!("unknown startup location {code}")))?;
    if name.is_empty() || name.contains('\\') || name.contains('\0') {
        return Err(BackendError::Refused("invalid startup entry name".into()));
    }
    Ok((loc, name.to_string()))
}

/// Expand `%VAR%` references using the current environment.
pub fn expand_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('%') {
            let var = &after[..end];
            match std::env::var(var) {
                Ok(v) if !var.is_empty() => out.push_str(&v),
                _ => {
                    out.push('%');
                    out.push_str(var);
                    out.push('%');
                }
            }
            rest = &after[end + 1..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

/// Extract the executable path from a Run-key command line.
pub fn executable_from_command(cmd: &str) -> Option<String> {
    let cmd = expand_env(cmd.trim());
    if let Some(stripped) = cmd.strip_prefix('"') {
        return stripped
            .split('"')
            .next()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
    }
    let lower = cmd.to_ascii_lowercase();
    if let Some(i) = lower.find(".exe") {
        return Some(cmd[..i + 4].to_string());
    }
    cmd.split_whitespace().next().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_parsing() {
        assert_eq!(
            executable_from_command(r#""C:\Program Files\App\app.exe" --min"#).unwrap(),
            r"C:\Program Files\App\app.exe"
        );
        assert_eq!(
            executable_from_command(r"C:\Tools\x.EXE /s").unwrap(),
            r"C:\Tools\x.EXE"
        );
        assert!(parse_id("hkcu-run|Steam").is_ok());
        assert!(parse_id("hkcu-run|..\\x").is_err());
        assert!(parse_id("bogus|x").is_err());
    }
}
