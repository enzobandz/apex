//! Storage analysis: temp-file cleanup, large-file and duplicate detection.
//!
//! Safety rules:
//! * Cleanup only ever touches files inside an explicitly allowed temp root.
//! * Symlinks / junctions are never followed and never deleted through.
//! * Only regular files older than the age threshold are deleted; directories are
//!   only removed when empty *and* older than the threshold.
//! * Files that are locked or fail to delete are reported, never forced.
//! * Large-file / duplicate scans are read-only; APEX never deletes personal files.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("refusing to operate on {0}: not a recognised temporary directory")]
    UnsafeRoot(String),
    #[error("path not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TempAnalysis {
    pub roots: Vec<String>,
    pub total_files: u64,
    pub total_bytes: u64,
    pub eligible_files: u64,
    pub eligible_bytes: u64,
    pub skipped_symlinks: u64,
    pub unreadable_entries: u64,
    pub older_than_days: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReport {
    pub deleted_files: u64,
    pub deleted_bytes: u64,
    pub removed_dirs: u64,
    pub failed_files: u64,
    /// Up to 50 example paths that could not be deleted (usually in use).
    pub failed_examples: Vec<String>,
}

impl CleanupReport {
    pub fn summary(&self) -> String {
        format!(
            "Deleted {} file(s) ({}), removed {} empty folder(s); {} file(s) were in use or protected and were left alone.",
            self.deleted_files,
            human_bytes(self.deleted_bytes),
            self.removed_dirs,
            self.failed_files
        )
    }
}

pub fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// A root is acceptable for automated cleanup only if its last component is a
/// conventional temp-directory name and it is not a filesystem root.
pub fn is_safe_temp_root(p: &Path) -> bool {
    let Ok(canon) = fs::canonicalize(p) else {
        return false;
    };
    if canon.parent().is_none() || canon.components().count() < 3 {
        return false;
    }
    let name = canon
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(name.as_str(), "temp" | "tmp")
}

fn age_cutoff(days: u32) -> SystemTime {
    SystemTime::now()
        .checked_sub(Duration::from_secs(days as u64 * 86_400))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn is_old(meta: &fs::Metadata, cutoff: SystemTime) -> bool {
    // Use the most recent of modified/created/accessed-not (atime is unreliable); modified + created.
    let modified = meta.modified().unwrap_or(SystemTime::now());
    let created = meta.created().unwrap_or(modified);
    modified.max(created) < cutoff
}

pub fn analyze_temp(roots: &[PathBuf], older_than_days: u32) -> TempAnalysis {
    let cutoff = age_cutoff(older_than_days);
    let mut a = TempAnalysis {
        roots: roots.iter().map(|r| r.display().to_string()).collect(),
        older_than_days,
        ..Default::default()
    };
    for root in roots {
        for entry in WalkDir::new(root).follow_links(false).min_depth(1) {
            let Ok(entry) = entry else {
                a.unreadable_entries += 1;
                continue;
            };
            let ft = entry.file_type();
            if ft.is_symlink() {
                a.skipped_symlinks += 1;
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                a.unreadable_entries += 1;
                continue;
            };
            a.total_files += 1;
            a.total_bytes += meta.len();
            if is_old(&meta, cutoff) {
                a.eligible_files += 1;
                a.eligible_bytes += meta.len();
            }
        }
    }
    a
}

pub fn clean_temp(roots: &[PathBuf], older_than_days: u32) -> Result<CleanupReport, StorageError> {
    clean_temp_before(roots, age_cutoff(older_than_days))
}

/// Deletes eligible files whose newest timestamp is before `cutoff`.
pub fn clean_temp_before(
    roots: &[PathBuf],
    cutoff: SystemTime,
) -> Result<CleanupReport, StorageError> {
    for r in roots {
        if !is_safe_temp_root(r) {
            return Err(StorageError::UnsafeRoot(r.display().to_string()));
        }
    }
    let mut rep = CleanupReport::default();
    for root in roots {
        let root_canon = fs::canonicalize(root)
            .map_err(|_| StorageError::NotFound(root.display().to_string()))?;
        // contents_first so we see files before their parent directories.
        let mut dirs = Vec::new();
        for entry in WalkDir::new(&root_canon)
            .follow_links(false)
            .min_depth(1)
            .contents_first(true)
        {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            // Belt and braces: never leave the root.
            if !path.starts_with(&root_canon) {
                continue;
            }
            let ft = entry.file_type();
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                dirs.push(path.to_path_buf());
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !is_old(&meta, cutoff) {
                continue;
            }
            match fs::remove_file(path) {
                Ok(()) => {
                    rep.deleted_files += 1;
                    rep.deleted_bytes += meta.len();
                }
                Err(_) => {
                    rep.failed_files += 1;
                    if rep.failed_examples.len() < 50 {
                        rep.failed_examples.push(path.display().to_string());
                    }
                }
            }
        }
        for d in dirs {
            let old = fs::symlink_metadata(&d)
                .map(|m| is_old(&m, cutoff))
                .unwrap_or(false);
            // remove_dir only succeeds on empty directories.
            if old && fs::remove_dir(&d).is_ok() {
                rep.removed_dirs += 1;
            }
        }
    }
    Ok(rep)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub size_bytes: u64,
    pub modified_unix: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFileScan {
    pub root: String,
    pub files_scanned: u64,
    pub bytes_scanned: u64,
    pub unreadable_entries: u64,
    pub largest: Vec<FileEntry>,
    /// Bytes per immediate child of root (folder breakdown for visualisation).
    pub by_child: Vec<(String, u64)>,
    pub truncated: bool,
}

fn unix_secs(t: SystemTime) -> Option<i64> {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// Read-only scan. `max_files` bounds the work so a scan of C:\ stays responsive.
pub fn scan_large_files(
    root: &Path,
    top_n: usize,
    max_files: u64,
) -> Result<LargeFileScan, StorageError> {
    if !root.exists() {
        return Err(StorageError::NotFound(root.display().to_string()));
    }
    let mut scan = LargeFileScan {
        root: root.display().to_string(),
        ..Default::default()
    };
    let mut largest: Vec<FileEntry> = Vec::new();
    let mut by_child: HashMap<String, u64> = HashMap::new();
    for entry in WalkDir::new(root).follow_links(false).min_depth(1) {
        let Ok(entry) = entry else {
            scan.unreadable_entries += 1;
            continue;
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            scan.unreadable_entries += 1;
            continue;
        };
        scan.files_scanned += 1;
        scan.bytes_scanned += meta.len();
        if let Ok(rel) = entry.path().strip_prefix(root) {
            if let Some(first) = rel.components().next() {
                *by_child
                    .entry(first.as_os_str().to_string_lossy().into_owned())
                    .or_default() += meta.len();
            }
        }
        if largest.len() < top_n
            || largest
                .last()
                .map(|l| meta.len() > l.size_bytes)
                .unwrap_or(true)
        {
            largest.push(FileEntry {
                path: entry.path().display().to_string(),
                size_bytes: meta.len(),
                modified_unix: meta.modified().ok().and_then(unix_secs),
            });
            largest.sort_by_key(|f| std::cmp::Reverse(f.size_bytes));
            largest.truncate(top_n);
        }
        if scan.files_scanned >= max_files {
            scan.truncated = true;
            break;
        }
    }
    let mut children: Vec<(String, u64)> = by_child.into_iter().collect();
    children.sort_by_key(|c| std::cmp::Reverse(c.1));
    scan.by_child = children;
    scan.largest = largest;
    Ok(scan)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub size_bytes: u64,
    pub paths: Vec<String>,
    /// Space that would be freed by keeping one copy.
    pub reclaimable_bytes: u64,
}

fn files_identical(a: &Path, b: &Path) -> std::io::Result<bool> {
    let mut fa = fs::File::open(a)?;
    let mut fb = fs::File::open(b)?;
    let mut ba = vec![0u8; 64 * 1024];
    let mut bb = vec![0u8; 64 * 1024];
    loop {
        let na = read_full(&mut fa, &mut ba)?;
        let nb = read_full(&mut fb, &mut bb)?;
        if na != nb || ba[..na] != bb[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

fn read_full(f: &mut fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let r = f.read(&mut buf[n..])?;
        if r == 0 {
            break;
        }
        n += r;
    }
    Ok(n)
}

/// Finds byte-for-byte identical files (size match, then full content comparison).
/// Report only — never deletes.
pub fn find_duplicates(
    root: &Path,
    min_size: u64,
    max_files: u64,
) -> Result<Vec<DuplicateGroup>, StorageError> {
    if !root.exists() {
        return Err(StorageError::NotFound(root.display().to_string()));
    }
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut n = 0u64;
    for entry in WalkDir::new(root).follow_links(false).into_iter().flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.len() < min_size {
            continue;
        }
        by_size
            .entry(meta.len())
            .or_default()
            .push(entry.into_path());
        n += 1;
        if n >= max_files {
            break;
        }
    }
    let mut groups = Vec::new();
    for (size, paths) in by_size.into_iter().filter(|(_, p)| p.len() > 1) {
        // Partition into equivalence classes by full comparison.
        let mut classes: Vec<Vec<PathBuf>> = Vec::new();
        'outer: for p in paths {
            for class in classes.iter_mut() {
                if files_identical(&class[0], &p).unwrap_or(false) {
                    class.push(p);
                    continue 'outer;
                }
            }
            classes.push(vec![p]);
        }
        for class in classes.into_iter().filter(|c| c.len() > 1) {
            groups.push(DuplicateGroup {
                size_bytes: size,
                reclaimable_bytes: size * (class.len() as u64 - 1),
                paths: class.into_iter().map(|p| p.display().to_string()).collect(),
            });
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.reclaimable_bytes));
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn refuses_non_temp_roots() {
        let d = tempfile::tempdir().unwrap();
        let docs = d.path().join("Documents");
        fs::create_dir(&docs).unwrap();
        assert!(clean_temp(&[docs], 0).is_err());
        assert!(clean_temp(&[PathBuf::from("/")], 0).is_err());
    }

    #[test]
    fn cleans_only_old_files_inside_root() {
        let d = tempfile::tempdir().unwrap();
        let temp = d.path().join("Temp");
        fs::create_dir_all(temp.join("sub")).unwrap();
        let a = temp.join("a.log");
        let b = temp.join("sub").join("b.bin");
        for p in [&a, &b] {
            File::create(p).unwrap().write_all(b"hello").unwrap();
        }
        let outside = d.path().join("precious.txt");
        File::create(&outside).unwrap();

        // Cutoff in the past: everything is "new", nothing may be deleted.
        let past = SystemTime::now() - Duration::from_secs(86_400);
        let rep = clean_temp_before(std::slice::from_ref(&temp), past).unwrap();
        assert_eq!(rep.deleted_files, 0);
        assert!(a.exists() && b.exists());

        // Cutoff in the future: everything inside root is eligible.
        let future = SystemTime::now() + Duration::from_secs(86_400);
        let rep = clean_temp_before(std::slice::from_ref(&temp), future).unwrap();
        assert_eq!(rep.deleted_files, 2);
        assert_eq!(rep.deleted_bytes, 10);
        assert_eq!(rep.removed_dirs, 1);
        assert!(temp.exists(), "root itself is never removed");
        assert!(outside.exists(), "file outside root must survive");
    }

    #[test]
    fn analysis_counts_files() {
        let d = tempfile::tempdir().unwrap();
        let temp = d.path().join("tmp");
        fs::create_dir_all(&temp).unwrap();
        File::create(temp.join("x"))
            .unwrap()
            .write_all(&[0u8; 100])
            .unwrap();
        let a = analyze_temp(&[temp], 0);
        assert_eq!(a.total_files, 1);
        assert_eq!(a.total_bytes, 100);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks_out_of_temp() {
        let d = tempfile::tempdir().unwrap();
        let temp = d.path().join("tmp");
        let important = d.path().join("important");
        fs::create_dir_all(&temp).unwrap();
        fs::create_dir_all(&important).unwrap();
        let victim = important.join("data.txt");
        File::create(&victim).unwrap();
        std::os::unix::fs::symlink(&important, temp.join("link")).unwrap();
        let future = SystemTime::now() + Duration::from_secs(86_400);
        clean_temp_before(&[temp], future).unwrap();
        assert!(victim.exists());
    }

    #[test]
    fn finds_true_duplicates_only() {
        let d = tempfile::tempdir().unwrap();
        let w = |n: &str, c: &[u8]| {
            File::create(d.path().join(n))
                .unwrap()
                .write_all(c)
                .unwrap()
        };
        w("a.bin", b"same content!");
        w("b.bin", b"same content!");
        w("c.bin", b"diff content!"); // same size, different bytes
        let g = find_duplicates(d.path(), 1, 1000).unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].paths.len(), 2);
        assert_eq!(g[0].reclaimable_bytes, 13);
    }

    #[test]
    fn large_file_scan_orders_by_size() {
        let d = tempfile::tempdir().unwrap();
        for (n, s) in [("a", 10usize), ("b", 1000), ("c", 100)] {
            File::create(d.path().join(n))
                .unwrap()
                .write_all(&vec![0u8; s])
                .unwrap();
        }
        let s = scan_large_files(d.path(), 2, 1000).unwrap();
        assert_eq!(s.largest.len(), 2);
        assert_eq!(s.largest[0].size_bytes, 1000);
        assert_eq!(s.largest[1].size_bytes, 100);
        assert_eq!(s.bytes_scanned, 1110);
    }
}
