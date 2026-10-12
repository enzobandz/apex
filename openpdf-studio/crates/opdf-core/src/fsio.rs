//! Crash-safe file writes.

use crate::error::{Error, ErrorCode, Result};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Write `bytes` to `path` atomically: write a sibling temp file, flush it to
/// disk, then rename it over the target. Readers see either the old or the
/// new file, never a torn one. Unix permissions of an existing file are kept.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| Error::invalid_arg("the destination is not a file path"))?;
    let tmp: PathBuf = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| -> std::io::Result<()> {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        #[cfg(unix)]
        if let Ok(meta) = fs::metadata(path) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        drop(f);
        fs::rename(&tmp, path)?;
        // Persist the rename itself.
        #[cfg(unix)]
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(match e.kind() {
            std::io::ErrorKind::PermissionDenied => Error::new(
                ErrorCode::Io,
                format!("no permission to write \"{}\"", path.display()),
            ),
            _ => Error::new(
                ErrorCode::Io,
                format!("could not save \"{}\": {e}", path.display()),
            ),
        });
    }
    Ok(())
}

/// Size and modification time, used to notice that a file changed on disk
/// behind our back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
}

pub fn stamp(path: &Path) -> Option<FileStamp> {
    let m = fs::metadata(path).ok()?;
    Some(FileStamp {
        len: m.len(),
        modified: m.modified().ok(),
    })
}

/// Read a file with a size ceiling.
pub fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let meta = fs::metadata(path).map_err(|e| {
        Error::new(
            ErrorCode::Io,
            format!("cannot open \"{}\": {e}", path.display()),
        )
    })?;
    if !meta.is_file() {
        return Err(Error::invalid_arg(format!(
            "\"{}\" is not a file",
            path.display()
        )));
    }
    if meta.len() > limit {
        return Err(Error::new(
            ErrorCode::LimitExceeded,
            "the file is too large",
        ));
    }
    fs::read(path).map_err(|e| {
        Error::new(
            ErrorCode::Io,
            format!("cannot read \"{}\": {e}", path.display()),
        )
    })
}

/// Make `name` safe as a single file-name component on every OS.
pub fn sanitize_file_name(name: &str) -> String {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() || s == "." || s == ".." {
        s = "document".into();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        s.insert(0, '_');
    }
    // Keep well below common 255-byte limits.
    if s.len() > 180 {
        let mut cut = 180;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
    }
    s
}

/// Find `dir/stem.ext`, or `dir/stem (2).ext`... that does not exist yet.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let stem = sanitize_file_name(stem);
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    for n in 2..100_000 {
        let p = dir.join(format!("{stem} ({n}).{ext}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem} {}.{ext}", uuid::Uuid::new_v4().simple()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.pdf");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn atomic_write_into_missing_dir_fails_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("missing").join("a.pdf");
        assert!(atomic_write(&p, b"x").is_err());
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_file_name("a/b:c?.pdf"), "a_b_c_.pdf");
        assert_eq!(sanitize_file_name("CON"), "_CON");
        assert_eq!(sanitize_file_name("  .. "), "document");
        assert_eq!(sanitize_file_name("x\u{0}y"), "x_y");
    }

    #[test]
    fn unique_paths() {
        let dir = tempfile::tempdir().unwrap();
        let a = unique_path(dir.path(), "out", "pdf");
        fs::write(&a, b"").unwrap();
        let b = unique_path(dir.path(), "out", "pdf");
        assert_eq!(b.file_name().unwrap(), "out (2).pdf");
    }
}
