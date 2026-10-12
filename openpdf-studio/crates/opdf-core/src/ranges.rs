//! Human page-range syntax: `1-3, 5, 8-`, `odd`, `even`, `last`, `-4`.
//! Pages are 1-based in the syntax and 0-based in the result.

use crate::error::{Error, Result};

/// Parse `spec` against a document of `count` pages. Order is preserved and
/// repeats are allowed (`3,1,3`), which extract/split rely on.
pub fn parse(spec: &str, count: usize) -> Result<Vec<usize>> {
    let mut out = Vec::new();
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(Error::invalid_arg("enter at least one page"));
    }
    for raw in spec.split([',', ';']) {
        let part = raw.trim().to_ascii_lowercase();
        if part.is_empty() {
            continue;
        }
        match part.as_str() {
            "all" | "*" => out.extend(0..count),
            "odd" => out.extend((0..count).step_by(2)),
            "even" => out.extend((1..count).step_by(2)),
            _ => {
                let (a, b) = match part.split_once('-') {
                    Some((a, b)) => (a.trim(), Some(b.trim())),
                    None => (part.as_str(), None),
                };
                let page = |s: &str, default: usize| -> Result<usize> {
                    if s.is_empty() {
                        return Ok(default);
                    }
                    if s == "last" || s == "end" || s == "z" {
                        return Ok(count);
                    }
                    let n: usize = s
                        .parse()
                        .map_err(|_| Error::invalid_arg(format!("\"{s}\" is not a page number")))?;
                    if n == 0 || n > count {
                        return Err(Error::invalid_arg(format!(
                            "page {n} does not exist (the document has {count} pages)"
                        )));
                    }
                    Ok(n)
                };
                match b {
                    None => out.push(page(a, 1)? - 1),
                    Some(b) => {
                        let start = page(a, 1)?;
                        let end = page(b, count)?;
                        if start <= end {
                            out.extend(start - 1..end);
                        } else {
                            out.extend((end - 1..start).rev());
                        }
                    }
                }
            }
        }
    }
    if out.is_empty() {
        return Err(Error::invalid_arg("the range selects no pages"));
    }
    Ok(out)
}

/// Format 0-based indices compactly as a 1-based range string.
pub fn format(indices: &[usize]) -> String {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < indices.len() {
        let start = indices[i];
        let mut end = start;
        while i + 1 < indices.len() && indices[i + 1] == end + 1 {
            i += 1;
            end += 1;
        }
        parts.push(if start == end {
            format!("{}", start + 1)
        } else {
            format!("{}-{}", start + 1, end + 1)
        });
        i += 1;
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranges() {
        assert_eq!(parse("1-3, 5", 10).unwrap(), vec![0, 1, 2, 4]);
        assert_eq!(parse("8-", 10).unwrap(), vec![7, 8, 9]);
        assert_eq!(parse("-2", 10).unwrap(), vec![0, 1]);
        assert_eq!(parse("odd", 5).unwrap(), vec![0, 2, 4]);
        assert_eq!(parse("even", 5).unwrap(), vec![1, 3]);
        assert_eq!(parse("last", 4).unwrap(), vec![3]);
        assert_eq!(parse("3-1", 4).unwrap(), vec![2, 1, 0]);
        assert_eq!(parse("2,2", 4).unwrap(), vec![1, 1]);
        assert_eq!(parse("all", 3).unwrap(), vec![0, 1, 2]);
    }

    #[test]
    fn rejects_bad_ranges() {
        assert!(parse("", 3).is_err());
        assert!(parse("0", 3).is_err());
        assert!(parse("4", 3).is_err());
        assert!(parse("a-b", 3).is_err());
        assert!(parse(",", 3).is_err());
    }

    #[test]
    fn formats_ranges() {
        assert_eq!(format(&[0, 1, 2, 4, 6, 7]), "1-3, 5, 7-8");
        assert_eq!(format(&[]), "");
    }
}
