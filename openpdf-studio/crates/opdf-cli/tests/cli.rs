use std::path::Path;
use std::process::Command;

fn opdf(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_opdf"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

fn p(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn end_to_end_workflow() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.pdf");
    let b = dir.path().join("b.pdf");
    assert!(opdf(&["sample", "--pages", "3", "-o", p(&a)]).0);
    assert!(opdf(&["sample", "--pages", "2", "-o", p(&b)]).0);

    let merged = dir.path().join("merged.pdf");
    let (ok, out) = opdf(&["merge", p(&a), p(&b), "-o", p(&merged), "--bookmarks"]);
    assert!(ok, "{out}");
    let (ok, out) = opdf(&["info", p(&merged)]);
    assert!(ok && out.contains("Pages:       5"), "{out}");

    let rotated = dir.path().join("rot.pdf");
    assert!(
        opdf(&[
            "rotate",
            p(&merged),
            "--pages",
            "1-2",
            "--degrees",
            "-90",
            "-o",
            p(&rotated)
        ])
        .0
    );

    let parts = dir.path().join("parts");
    let (ok, out) = opdf(&["split", p(&merged), "--every", "2", "--out-dir", p(&parts)]);
    assert!(ok, "{out}");
    assert_eq!(std::fs::read_dir(&parts).unwrap().count(), 3);

    let prot = dir.path().join("prot");
    let (ok, out) = opdf(&[
        "protect",
        p(&a),
        p(&b),
        "--out-dir",
        p(&prot),
        "--user-password",
        "pw",
    ]);
    assert!(ok, "{out}");
    let (ok, out) = opdf(&["info", p(&prot.join("a.pdf"))]);
    assert!(!ok && out.contains("password protected"), "{out}");
    let (ok, out) = opdf(&["info", p(&prot.join("a.pdf")), "--password", "pw", "--json"]);
    assert!(ok && out.contains("\"pageCount\": 3"), "{out}");

    let plain = dir.path().join("plain.pdf");
    assert!(
        opdf(&[
            "unprotect",
            p(&prot.join("a.pdf")),
            "--password",
            "pw",
            "-o",
            p(&plain)
        ])
        .0
    );
    assert!(opdf(&["info", p(&plain)]).0);

    // Never overwrite an input in place by accident.
    let (ok, out) = opdf(&["optimize", p(&a), "-o", p(&a)]);
    assert!(!ok && out.contains("refusing"), "{out}");
}

#[test]
fn bad_input_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let junk = dir.path().join("junk.pdf");
    std::fs::write(&junk, b"definitely not a pdf").unwrap();
    let (ok, out) = opdf(&["info", p(&junk)]);
    assert!(!ok && out.contains("not a PDF"), "{out}");
}
