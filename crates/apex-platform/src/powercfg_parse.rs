//! Parsing of `powercfg /list` output. Locale-independent: relies only on the GUID
//! token, the parenthesised name, and the trailing `*` marking the active scheme.

use apex_core::model::PowerScheme;

pub fn is_guid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Returns (schemes, active scheme guid).
pub fn parse_list(output: &str) -> (Vec<PowerScheme>, Option<String>) {
    let mut schemes = Vec::new();
    let mut active = None;
    for line in output.lines() {
        let Some(guid) = line.split_whitespace().find(|t| is_guid(t)) else {
            continue;
        };
        let guid = guid.to_ascii_lowercase();
        let name = match (line.find('('), line.rfind(')')) {
            (Some(a), Some(b)) if b > a => line[a + 1..b].trim().to_string(),
            _ => guid.clone(),
        };
        if line.trim_end().ends_with('*') {
            active = Some(guid.clone());
        }
        schemes.push(PowerScheme { guid, name });
    }
    (schemes, active)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_english_and_localized_output() {
        let en = "Existing Power Schemes (* Active)\n-----------------------------------\nPower Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced) *\nPower Scheme GUID: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (High performance)\nPower Scheme GUID: a1841308-3541-4fab-bc81-f71556f20b4a  (Power saver)\n";
        let (s, a) = parse_list(en);
        assert_eq!(s.len(), 3);
        assert_eq!(a.as_deref(), Some("381b4222-f694-41f0-9685-ff5bb260df2e"));
        assert_eq!(s[1].name, "High performance");

        let de = "Vorhandene Energieschemas (* Aktiv)\nGUID des Energieschemas: 8C5E7FDA-E8BF-4A96-9A85-A6E23A8C635C  (Höchstleistung) *\n";
        let (s, a) = parse_list(de);
        assert_eq!(s[0].name, "Höchstleistung");
        assert_eq!(a.as_deref(), Some("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"));
    }

    #[test]
    fn guid_validation() {
        assert!(is_guid("381b4222-f694-41f0-9685-ff5bb260df2e"));
        assert!(!is_guid("381b4222-f694-41f0-9685-ff5bb260df2e & del"));
        assert!(!is_guid("/setactive"));
    }
}
