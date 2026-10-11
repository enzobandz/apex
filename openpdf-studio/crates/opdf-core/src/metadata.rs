//! Document information dictionary (and XMP packet) read/write.

use crate::error::Result;
use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone};
use lopdf::{decode_text_string, dictionary, text_string, Dictionary, Document, Object, Stream};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    /// RFC 3339
    pub creation_date: Option<String>,
    /// RFC 3339
    pub modification_date: Option<String>,
}

/// Editable fields. `None` leaves a field untouched; an empty string removes it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataUpdate {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
}

fn info_dict(doc: &Document) -> Option<&Dictionary> {
    match doc.trailer.get(b"Info").ok()? {
        Object::Reference(r) => doc.get_dictionary(*r).ok(),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

fn text(d: &Dictionary, key: &[u8], doc: &Document) -> Option<String> {
    let o = d.get(key).ok()?;
    let o = match o {
        Object::Reference(r) => doc.get_object(*r).ok()?,
        o => o,
    };
    let s = decode_text_string(o).ok()?;
    let s = s.trim_matches(char::from(0)).trim().to_string();
    (!s.is_empty()).then_some(s)
}

pub fn read(doc: &Document) -> Metadata {
    let Some(d) = info_dict(doc) else {
        return Metadata::default();
    };
    Metadata {
        title: text(d, b"Title", doc),
        author: text(d, b"Author", doc),
        subject: text(d, b"Subject", doc),
        keywords: text(d, b"Keywords", doc),
        creator: text(d, b"Creator", doc),
        producer: text(d, b"Producer", doc),
        creation_date: text(d, b"CreationDate", doc)
            .and_then(|s| parse_pdf_date(&s))
            .map(|d| d.to_rfc3339()),
        modification_date: text(d, b"ModDate", doc)
            .and_then(|s| parse_pdf_date(&s))
            .map(|d| d.to_rfc3339()),
    }
}

/// Apply `update`, set ModDate to now and keep an existing XMP packet in sync.
pub fn write(doc: &mut Document, update: &MetadataUpdate) -> Result<()> {
    let info_id = match doc.trailer.get(b"Info") {
        Ok(Object::Reference(r)) if doc.get_dictionary(*r).is_ok() => *r,
        Ok(Object::Dictionary(d)) => {
            let d = d.clone();
            let id = doc.add_object(Object::Dictionary(d));
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
        _ => {
            let id = doc.add_object(Dictionary::new());
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
    };
    let now = format_pdf_date(&Local::now().fixed_offset());
    {
        let info = doc.get_dictionary_mut(info_id)?;
        let fields: [(&str, &Option<String>); 5] = [
            ("Title", &update.title),
            ("Author", &update.author),
            ("Subject", &update.subject),
            ("Keywords", &update.keywords),
            ("Creator", &update.creator),
        ];
        for (key, value) in fields {
            match value.as_deref().map(str::trim) {
                None => {}
                Some("") => {
                    info.remove(key.as_bytes());
                }
                Some(v) => info.set(key, text_string(v)),
            }
        }
        info.set("ModDate", Object::string_literal(now));
    }
    sync_xmp(doc)?;
    Ok(())
}

/// Set ModDate (and CreationDate if missing) — used when saving changes.
pub fn touch(doc: &mut Document) {
    let now = format_pdf_date(&Local::now().fixed_offset());
    let id = match doc.trailer.get(b"Info") {
        Ok(Object::Reference(r)) => *r,
        _ => {
            let id = doc.add_object(Dictionary::new());
            doc.trailer.set("Info", Object::Reference(id));
            id
        }
    };
    if let Ok(info) = doc.get_dictionary_mut(id) {
        info.set("ModDate", Object::string_literal(now.clone()));
        if !info.has(b"CreationDate") {
            info.set("CreationDate", Object::string_literal(now));
        }
    }
}

/// Replace an existing XMP metadata stream with one generated from the info
/// dictionary. A stale XMP packet would otherwise override the edited values
/// in readers that prefer XMP (all PDF 2.0 readers). Documents without XMP
/// are left without it.
fn sync_xmp(doc: &mut Document) -> Result<()> {
    let Ok(xmp_ref) = doc
        .catalog()?
        .get(b"Metadata")
        .and_then(Object::as_reference)
    else {
        return Ok(());
    };
    let m = read(doc);
    let packet = build_xmp(&m);
    let stream = Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        packet.into_bytes(),
    )
    .with_compression(false);
    doc.objects.insert(xmp_ref, Object::Stream(stream));
    Ok(())
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn build_xmp(m: &Metadata) -> String {
    let mut body = String::new();
    if let Some(t) = &m.title {
        body += &format!(
            "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:title>",
            xml_escape(t)
        );
    }
    if let Some(a) = &m.author {
        body += &format!(
            "<dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>",
            xml_escape(a)
        );
    }
    if let Some(s) = &m.subject {
        body += &format!(
            "<dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:description>",
            xml_escape(s)
        );
    }
    if let Some(k) = &m.keywords {
        body += &format!("<pdf:Keywords>{}</pdf:Keywords>", xml_escape(k));
    }
    if let Some(p) = &m.producer {
        body += &format!("<pdf:Producer>{}</pdf:Producer>", xml_escape(p));
    }
    if let Some(c) = &m.creator {
        body += &format!("<xmp:CreatorTool>{}</xmp:CreatorTool>", xml_escape(c));
    }
    if let Some(d) = &m.creation_date {
        body += &format!("<xmp:CreateDate>{}</xmp:CreateDate>", xml_escape(d));
    }
    if let Some(d) = &m.modification_date {
        body += &format!("<xmp:ModifyDate>{}</xmp:ModifyDate>", xml_escape(d));
    }
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">{body}</rdf:Description>\
</rdf:RDF></x:xmpmeta>\n<?xpacket end=\"w\"?>"
    )
}

/// Parse a PDF date string (`D:YYYYMMDDHHmmSSOHH'mm'`, all parts after the
/// year optional).
pub fn parse_pdf_date(s: &str) -> Option<DateTime<FixedOffset>> {
    let s = s.trim();
    let s = s.strip_prefix("D:").unwrap_or(s);
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() < 4 {
        return None;
    }
    let num = |from: usize, len: usize, default: u32| -> u32 {
        digits
            .get(from..from + len)
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let year: i32 = digits[0..4].parse().ok()?;
    let (month, day) = (num(4, 2, 1), num(6, 2, 1));
    let (hour, minute, second) = (num(8, 2, 0), num(10, 2, 0), num(12, 2, 0));
    let rest = &s[digits.len()..];
    let offset_secs = match rest.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let tz: String = rest[1..].chars().filter(|c| c.is_ascii_digit()).collect();
            let h: i32 = tz.get(0..2).and_then(|v| v.parse().ok()).unwrap_or(0);
            let m: i32 = tz.get(2..4).and_then(|v| v.parse().ok()).unwrap_or(0);
            let secs = h * 3600 + m * 60;
            if sign == '-' {
                -secs
            } else {
                secs
            }
        }
        _ => 0, // 'Z' or missing: UTC
    };
    let tz = FixedOffset::east_opt(offset_secs)?;
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let naive = date.and_hms_opt(hour.min(23), minute.min(59), second.min(59))?;
    tz.from_local_datetime(&naive).single()
}

pub fn format_pdf_date(d: &DateTime<FixedOffset>) -> String {
    let off = d.offset().local_minus_utc();
    let sign = if off < 0 { '-' } else { '+' };
    let off = off.abs();
    format!(
        "D:{}{}{:02}'{:02}'",
        d.format("%Y%m%d%H%M%S"),
        sign,
        off / 3600,
        (off % 3600) / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_dates_round_trip() {
        let d = parse_pdf_date("D:20240131235959+05'30'").unwrap();
        assert_eq!(d.to_rfc3339(), "2024-01-31T23:59:59+05:30");
        assert_eq!(format_pdf_date(&d), "D:20240131235959+05'30'");
        assert_eq!(
            parse_pdf_date("D:2023").unwrap().to_rfc3339(),
            "2023-01-01T00:00:00+00:00"
        );
        assert_eq!(
            parse_pdf_date("D:20230405Z").unwrap().to_rfc3339(),
            "2023-04-05T00:00:00+00:00"
        );
        assert!(parse_pdf_date("garbage").is_none());
        assert!(parse_pdf_date("D:20231399").is_none());
    }

    #[test]
    fn xmp_is_escaped() {
        let m = Metadata {
            title: Some("A <b> & \"c\"".into()),
            ..Default::default()
        };
        let x = build_xmp(&m);
        assert!(x.contains("A &lt;b&gt; &amp; &quot;c&quot;"));
    }
}
