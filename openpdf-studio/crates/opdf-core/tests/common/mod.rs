#![allow(dead_code)]

use opdf_core::fixtures::{self, FixtureOptions};
use opdf_core::lopdf::Document;
use opdf_core::{load, LoadedPdf};
use std::path::Path;
use std::process::Command;

pub fn fixture(opts: FixtureOptions) -> LoadedPdf {
    load(&fixtures::generate_bytes(&opts), None).unwrap()
}

pub fn simple(n: usize) -> LoadedPdf {
    load(&fixtures::simple(n), None).unwrap()
}

pub fn labels(doc: &Document) -> Vec<String> {
    fixtures::page_labels(doc)
}

pub fn labels_of(bytes: &[u8], password: Option<&str>) -> Vec<String> {
    labels(&load(bytes, password).unwrap().doc)
}

pub fn save(doc: &Document) -> Vec<u8> {
    opdf_core::load::serialize(doc, None).unwrap()
}

pub fn qpdf_available() -> bool {
    Command::new("qpdf")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Structural validation with qpdf when it is installed (CI installs it).
/// Exit status 0 = clean, 3 = warnings only (both acceptable for output).
pub fn qpdf_check(bytes: &[u8], password: Option<&str>) {
    if !qpdf_available() {
        eprintln!("qpdf not installed; skipping external validation");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("check.pdf");
    std::fs::write(&p, bytes).unwrap();
    let mut cmd = Command::new("qpdf");
    if let Some(pw) = password {
        cmd.arg(format!("--password={pw}"));
    }
    let out = cmd.arg("--check").arg(&p).output().unwrap();
    let code = out.status.code().unwrap_or(-1);
    assert!(
        code == 0,
        "qpdf --check failed ({code}):\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Run qpdf with arbitrary args producing `out`; returns None when qpdf is missing.
pub fn qpdf(args: &[&str], input: &[u8]) -> Option<Vec<u8>> {
    if !qpdf_available() {
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    let i = dir.path().join("in.pdf");
    let o = dir.path().join("out.pdf");
    std::fs::write(&i, input).unwrap();
    let status = Command::new("qpdf")
        .args(args)
        .arg(&i)
        .arg(&o)
        .status()
        .unwrap();
    assert!(
        status.code() == Some(0) || status.code() == Some(3),
        "qpdf {args:?} failed"
    );
    Some(std::fs::read(&o).unwrap())
}

pub fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

pub fn exists(p: &Path) -> bool {
    p.exists()
}
