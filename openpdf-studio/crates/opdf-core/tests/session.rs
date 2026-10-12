mod common;

use common::*;
use opdf_core::fixtures::{self, FixtureOptions};
use opdf_core::metadata::MetadataUpdate;
use opdf_core::optimize::{ImageOptions, OptimizeOptions};
use opdf_core::recovery::{self, SessionDir};
use opdf_core::session::{DocSession, Op, OpenRequest, MAX_UNDO};
use opdf_core::{load, ErrorCode};
use std::fs;
use std::path::PathBuf;

fn req(bytes: Vec<u8>, path: Option<PathBuf>, store: Option<PathBuf>) -> OpenRequest {
    OpenRequest {
        bytes,
        password: None,
        name: "doc.pdf".into(),
        path,
        store,
        label: "Opened".into(),
    }
}

#[test]
fn undo_redo_restores_exact_bytes() {
    let original = fixtures::simple(4);
    let mut s = DocSession::open(1, req(original.clone(), None, None)).unwrap();
    s.apply(&Op::Delete { pages: vec![0] }).unwrap();
    let after_delete = s.bytes().to_vec();
    s.apply(&Op::Rotate {
        pages: vec![0],
        degrees: 90,
    })
    .unwrap();
    let st = s.state();
    assert!(st.can_undo && !st.can_redo);
    assert_eq!(st.undo_label.as_deref(), Some("Rotate page"));
    assert_eq!(st.summary.unwrap().page_count, 3);

    s.undo().unwrap();
    assert_eq!(s.bytes().to_vec(), after_delete);
    s.undo().unwrap();
    assert_eq!(s.bytes().to_vec(), original);
    assert!(s.undo().is_err());
    assert_eq!(s.state().summary.unwrap().page_count, 4);

    s.redo().unwrap();
    assert_eq!(s.bytes().to_vec(), after_delete);
    // A new edit drops the redo branch.
    s.apply(&Op::Duplicate { pages: vec![0] }).unwrap();
    assert!(!s.state().can_redo);
    assert_eq!(
        labels_of(&s.bytes(), None),
        vec!["Page 2", "Page 2", "Page 3", "Page 4"]
    );
}

#[test]
fn dirty_tracking_and_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.pdf");
    fs::write(&path, fixtures::simple(2)).unwrap();
    let mut s =
        DocSession::open(1, req(fs::read(&path).unwrap(), Some(path.clone()), None)).unwrap();
    assert!(!s.state().dirty);
    s.apply(&Op::Rotate {
        pages: vec![1],
        degrees: 90,
    })
    .unwrap();
    assert!(s.state().dirty);
    s.save(false).unwrap();
    assert!(!s.state().dirty);
    assert_eq!(fs::read(&path).unwrap(), s.bytes().to_vec());
    // Undo after save makes it dirty again; redo returns to the saved state.
    s.undo().unwrap();
    assert!(s.state().dirty);
    s.redo().unwrap();
    assert!(!s.state().dirty);
}

#[test]
fn save_detects_external_modification() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.pdf");
    fs::write(&path, fixtures::simple(2)).unwrap();
    let mut s =
        DocSession::open(1, req(fs::read(&path).unwrap(), Some(path.clone()), None)).unwrap();
    s.apply(&Op::Rotate {
        pages: vec![0],
        degrees: 90,
    })
    .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(&path, fixtures::simple(5)).unwrap(); // someone else changed it
    assert_eq!(s.save(false).unwrap_err().code, ErrorCode::FileChanged);
    s.save(true).unwrap();
    assert_eq!(
        load(&fs::read(&path).unwrap(), None)
            .unwrap()
            .doc
            .get_pages()
            .len(),
        2
    );
}

#[test]
fn save_as_switches_target_and_untitled_documents_start_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = DocSession::open(1, req(fixtures::simple(1), None, None)).unwrap();
    assert!(s.state().dirty, "new documents need saving");
    assert_eq!(s.save(false).unwrap_err().code, ErrorCode::InvalidArgument);
    let p = dir.path().join("New name.pdf");
    s.save_as(&p).unwrap();
    let st = s.state();
    assert!(!st.dirty);
    assert_eq!(st.name, "New name.pdf");
    assert_eq!(st.path.unwrap(), p);
}

#[test]
fn history_is_bounded() {
    let mut s = DocSession::open(1, req(fixtures::simple(1), None, None)).unwrap();
    for _ in 0..(MAX_UNDO + 10) {
        s.apply(&Op::Rotate {
            pages: vec![0],
            degrees: 90,
        })
        .unwrap();
    }
    let mut undos = 0;
    while s.undo().is_ok() {
        undos += 1;
    }
    assert_eq!(undos, MAX_UNDO);
}

#[test]
fn recovery_files_follow_unsaved_work_and_survive_a_crash() {
    let root = tempfile::tempdir().unwrap();
    let src_dir = tempfile::tempdir().unwrap();
    let path = src_dir.path().join("report.pdf");
    fs::write(&path, fixtures::simple(3)).unwrap();

    let doc_dir;
    {
        let session = SessionDir::create(root.path()).unwrap();
        doc_dir = session.doc_dir(7).unwrap();
        let mut s = DocSession::open(
            7,
            req(
                fs::read(&path).unwrap(),
                Some(path.clone()),
                Some(doc_dir.clone()),
            ),
        )
        .unwrap();
        // Opening alone writes nothing.
        assert!(fs::read_dir(&doc_dir).unwrap().next().is_none());
        s.apply(&Op::Delete { pages: vec![0] }).unwrap();
        assert!(recovery::read_manifest(&doc_dir)
            .unwrap()
            .has_unsaved_changes());
        // While the session is alive, it is not offered for recovery.
        assert!(recovery::scan(root.path()).is_empty());
        // Simulate a crashed instance: a session directory whose lock file
        // nobody holds (the OS releases locks of dead processes).
        let crashed = root.path().join("session-crashed");
        fs::create_dir(&crashed).unwrap();
        fs::write(crashed.join("session.lock"), b"").unwrap();
        let crashed_doc = crashed.join("doc-7");
        fs::create_dir(&crashed_doc).unwrap();
        for e in fs::read_dir(&doc_dir).unwrap().flatten() {
            fs::copy(e.path(), crashed_doc.join(e.file_name())).unwrap();
        }
        drop(s);
        drop(session); // clean shutdown removes its own directory
    }
    assert!(!doc_dir.exists(), "clean shutdown cleans up");
    let found = recovery::scan(root.path());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "doc.pdf");
    assert_eq!(found[0].last_change, "Delete page");
    let (manifest, bytes) = recovery::restore(root.path(), &found[0].dir).unwrap();
    assert_eq!(manifest.path.as_deref(), Some(path.as_path()));
    assert_eq!(labels_of(&bytes, None), vec!["Page 2", "Page 3"]);
    recovery::discard(root.path(), &found[0].dir).unwrap();
    assert!(recovery::scan(root.path()).is_empty());
    assert_eq!(
        fs::read_dir(root.path()).unwrap().count(),
        0,
        "empty dead sessions are removed"
    );
}

#[test]
fn recovery_rejects_paths_outside_the_root() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    assert!(recovery::restore(root.path(), other.path()).is_err());
    assert!(recovery::discard(root.path(), other.path()).is_err());
}

#[test]
fn saved_documents_are_not_offered_for_recovery() {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.pdf");
    fs::write(&path, fixtures::simple(2)).unwrap();
    let session = SessionDir::create(root.path()).unwrap();
    let doc_dir = session.doc_dir(1).unwrap();
    let mut s = DocSession::open(
        1,
        req(
            fs::read(&path).unwrap(),
            Some(path.clone()),
            Some(doc_dir.clone()),
        ),
    )
    .unwrap();
    s.apply(&Op::Rotate {
        pages: vec![0],
        degrees: 90,
    })
    .unwrap();
    s.save(false).unwrap();
    assert!(!recovery::read_manifest(&doc_dir)
        .unwrap()
        .has_unsaved_changes());
}

#[test]
fn metadata_edit_round_trip() {
    let mut s = DocSession::open(1, req(fixtures::simple(1), None, None)).unwrap();
    s.apply(&Op::SetMetadata {
        update: MetadataUpdate {
            title: Some("Quarterly Report — Ünïcødé".into()),
            author: Some("Ada".into()),
            subject: Some("".into()),
            keywords: None,
            creator: None,
        },
    })
    .unwrap();
    let m = s.state().summary.unwrap().metadata;
    assert_eq!(m.title.as_deref(), Some("Quarterly Report — Ünïcødé"));
    assert_eq!(m.author.as_deref(), Some("Ada"));
    assert_eq!(m.subject, None);
    assert!(m.modification_date.is_some());
    qpdf_check(&s.bytes(), None);
}

#[test]
fn metadata_edit_rewrites_existing_xmp() {
    let mut pdf = load(&fixtures::simple(1), None).unwrap();
    let xmp = pdf.doc.add_object(opdf_core::lopdf::Stream::new(
        opdf_core::lopdf::dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        b"<x:xmpmeta><dc:title>Old title</dc:title></x:xmpmeta>".to_vec(),
    ));
    pdf.doc.catalog_mut().unwrap().set("Metadata", xmp);
    let bytes = save(&pdf.doc);
    let mut s = DocSession::open(1, req(bytes, None, None)).unwrap();
    s.apply(&Op::SetMetadata {
        update: MetadataUpdate {
            title: Some("New title".into()),
            ..Default::default()
        },
    })
    .unwrap();
    let out = s.bytes();
    assert!(!contains(&out, b"Old title"));
    assert!(contains(&out, b"New title"));
}

#[test]
fn optimize_merges_duplicates_and_keeps_pages() {
    // Merge the same file with itself → identical streams to deduplicate.
    let a = load(&fixtures::simple(20), None).unwrap();
    let b = load(&fixtures::simple(20), None).unwrap();
    let merged = opdf_core::assemble::merge(
        vec![
            opdf_core::assemble::MergeInput {
                name: "a".into(),
                pdf: a,
                pages: None,
            },
            opdf_core::assemble::MergeInput {
                name: "b".into(),
                pdf: b,
                pages: None,
            },
        ],
        Default::default(),
    )
    .unwrap();
    let bytes = save(&merged);
    let mut s = DocSession::open(1, req(bytes.clone(), None, None)).unwrap();
    let report = s
        .apply(&Op::Optimize {
            options: OptimizeOptions { images: None },
        })
        .unwrap()
        .unwrap();
    assert!(report.duplicate_streams_merged >= 20, "{report:?}");
    let out = s.bytes();
    assert!(out.len() < bytes.len(), "{} !< {}", out.len(), bytes.len());
    assert_eq!(labels_of(&out, None).len(), 40);
    qpdf_check(&out, None);
    // Later edits keep the compact format and stay valid.
    s.apply(&Op::Rotate {
        pages: vec![0],
        degrees: 90,
    })
    .unwrap();
    qpdf_check(&s.bytes(), None);
}

#[test]
fn optimize_downsamples_oversized_images() {
    // A 3000×3000 photo-like PNG on a Letter-sized page (≈ 375 dpi).
    let img = image::RgbImage::from_fn(3000, 3000, |x, y| {
        image::Rgb([(x / 7) as u8, (y / 5) as u8, ((x + y) / 11) as u8])
    });
    let mut png = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let doc = opdf_core::images::images_to_pdf(
        &[png],
        &opdf_core::images::ImagePageOptions {
            page_size: opdf_core::images::PageSizeChoice::Letter,
            orientation: opdf_core::images::Orientation::Portrait,
            margin: 0.0,
        },
    )
    .unwrap();
    let bytes = save(&doc);
    let mut s = DocSession::open(1, req(bytes.clone(), None, None)).unwrap();
    let report = s
        .apply(&Op::Optimize {
            options: OptimizeOptions {
                images: Some(ImageOptions {
                    max_dpi: 150,
                    jpeg_quality: 75,
                }),
            },
        })
        .unwrap()
        .unwrap();
    assert_eq!(report.images_downsampled, 1, "{report:?}");
    let out = s.bytes();
    assert!(
        out.len() * 3 < bytes.len(),
        "{} vs {}",
        out.len(),
        bytes.len()
    );
    let pdf = load(&out, None).unwrap();
    let img = pdf
        .doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .find(|s| s.dict.get(b"Subtype").and_then(|o| o.as_name()).ok() == Some(b"Image"))
        .unwrap();
    // 11 in × 150 dpi = 1650 px on the long side.
    assert_eq!(img.dict.get(b"Width").unwrap().as_i64().unwrap(), 1650);
    qpdf_check(&out, None);
}

#[test]
fn commit_bytes_requires_same_page_count() {
    let mut s = DocSession::open(1, req(fixtures::simple(2), None, None)).unwrap();
    let err = s.commit_bytes(fixtures::simple(3), "Annotate").unwrap_err();
    assert_eq!(err.code, ErrorCode::Internal);
    let other = fixtures::generate_bytes(&FixtureOptions {
        pages: 2,
        title: Some("changed".into()),
        ..Default::default()
    });
    s.commit_bytes(other, "Annotate").unwrap();
    assert_eq!(s.state().undo_label.as_deref(), Some("Annotate"));
    assert!(s.commit_bytes(b"garbage".to_vec(), "x").is_err());
}

#[test]
fn extract_and_split_keep_protection() {
    let mut s = DocSession::open(1, req(fixtures::simple(4), None, None)).unwrap();
    s.apply(&Op::Protect {
        options: opdf_core::security::ProtectOptions {
            user_password: "pw".into(),
            owner_password: "".into(),
            allowed: opdf_core::security::AllowedActions::ALL,
        },
    })
    .unwrap();
    let part = s.extract(&[1, 2]).unwrap();
    assert_eq!(
        load(&part, None).unwrap_err().code,
        ErrorCode::PasswordRequired
    );
    assert_eq!(labels_of(&part, Some("pw")), vec!["Page 2", "Page 3"]);
    let parts = s
        .split(&opdf_core::assemble::SplitPlan::EveryN { n: 3 })
        .unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(labels_of(&parts[1].1, Some("pw")), vec!["Page 4"]);
    qpdf_check(&parts[0].1, Some("pw"));
}
