//! Malformed, hostile and large inputs must produce errors — never panics,
//! hangs or unbounded memory use.

mod common;

use common::*;
use opdf_core::fixtures::{self, FixtureOptions};
use opdf_core::lopdf::{dictionary, Object};
use opdf_core::session::{DocSession, Op, OpenRequest};
use opdf_core::{load, pages, ErrorCode};
use std::time::{Duration, Instant};

fn open(bytes: Vec<u8>) -> opdf_core::Result<DocSession> {
    DocSession::open(
        1,
        OpenRequest {
            bytes,
            password: None,
            name: "x.pdf".into(),
            path: None,
            store: None,
            label: "Opened".into(),
        },
    )
}

/// Run every operation we can on whatever loads; nothing may panic.
fn exercise(bytes: Vec<u8>) {
    let Ok(mut s) = open(bytes) else { return };
    let _ = s.state();
    let n = s.state().summary.map(|x| x.page_count).unwrap_or(0);
    if n == 0 {
        return;
    }
    let _ = s.apply(&Op::Rotate {
        pages: vec![0],
        degrees: 90,
    });
    let _ = s.apply(&Op::Duplicate { pages: vec![0] });
    let _ = s.apply(&Op::Move {
        pages: vec![0],
        to: n,
    });
    if n > 1 {
        let _ = s.apply(&Op::Delete { pages: vec![0] });
    }
    let _ = s.apply(&Op::Optimize {
        options: Default::default(),
    });
    let _ = s.extract(&[0]);
    let _ = s.undo();
}

/// Tiny deterministic PRNG (xorshift) so failures are reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn rejects_non_pdf_input() {
    for bytes in [
        &b""[..],
        b"hello",
        b"%PDF",
        b"\x00\x01\x02",
        b"%PDF-1.7\n%%EOF",
    ] {
        let err = load(bytes, None).unwrap_err();
        assert!(matches!(err.code, ErrorCode::InvalidPdf), "{err:?}");
    }
}

#[test]
fn truncated_files_never_panic() {
    let full = fixtures::generate_bytes(&FixtureOptions {
        pages: 8,
        outline: true,
        form_field: true,
        link_to_last: true,
        ..Default::default()
    });
    for cut in (0..full.len()).step_by(37) {
        exercise(full[..cut].to_vec());
    }
}

#[test]
fn corrupted_bytes_never_panic() {
    let full = fixtures::generate_bytes(&FixtureOptions {
        pages: 6,
        nested_tree: true,
        outline: true,
        form_field: true,
        ..Default::default()
    });
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..300 {
        let mut b = full.clone();
        for _ in 0..(1 + rng.next() % 8) {
            let i = (rng.next() as usize) % b.len();
            b[i] = (rng.next() & 0xFF) as u8;
        }
        exercise(b);
    }
}

#[test]
fn compact_files_corrupted_never_panic() {
    // Object/xref streams exercise different parser paths.
    let mut s = open(fixtures::simple(5)).unwrap();
    s.apply(&Op::Optimize {
        options: Default::default(),
    })
    .unwrap();
    let full = s.bytes().to_vec();
    let mut rng = Rng(42);
    for _ in 0..200 {
        let mut b = full.clone();
        let i = (rng.next() as usize) % b.len();
        b[i] ^= 1 << (rng.next() % 8);
        exercise(b);
    }
}

#[test]
fn cyclic_page_tree_does_not_hang() {
    let mut pdf = load(&fixtures::simple(2), None).unwrap();
    let root = pdf
        .doc
        .catalog()
        .unwrap()
        .get(b"Pages")
        .unwrap()
        .as_reference()
        .unwrap();
    // Root lists itself as a kid.
    let kids = pdf
        .doc
        .get_dictionary_mut(root)
        .unwrap()
        .get_mut(b"Kids")
        .unwrap()
        .as_array_mut()
        .unwrap();
    kids.push(Object::Reference(root));
    let bytes = save(&pdf.doc);
    let start = Instant::now();
    exercise(bytes);
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn page_with_self_referencing_parent_chain() {
    let mut pdf = load(&fixtures::simple(1), None).unwrap();
    let p = pages::page_ids(&pdf.doc)[0];
    pdf.doc.get_dictionary_mut(p).unwrap().remove(b"MediaBox");
    // Parent points to the page itself.
    pdf.doc.get_dictionary_mut(p).unwrap().set("Parent", p);
    assert!(pages::inherited(&pdf.doc, p, b"MediaBox").is_none());
    let _ = pages::page_size(&pdf.doc, p);
}

#[test]
fn missing_mediabox_and_resources_get_defaults() {
    let mut pdf = load(&fixtures::simple(2), None).unwrap();
    for p in pages::page_ids(&pdf.doc) {
        let d = pdf.doc.get_dictionary_mut(p).unwrap();
        d.remove(b"MediaBox");
        d.remove(b"Resources");
    }
    pages::reorder(&mut pdf.doc, &[1, 0]).unwrap();
    let out = save(&pdf.doc);
    qpdf_check(&out, None);
}

#[test]
fn annotation_arrays_with_garbage_entries() {
    let mut pdf = load(&fixtures::simple(2), None).unwrap();
    let p = pages::page_ids(&pdf.doc)[0];
    pdf.doc.get_dictionary_mut(p).unwrap().set(
        "Annots",
        vec![
            Object::Integer(5),
            Object::Reference((9999, 0)),
            Object::Dictionary(dictionary! { "Subtype" => "Text" }),
        ],
    );
    pages::duplicate(&mut pdf.doc, &[0]).unwrap();
    pages::delete(&mut pdf.doc, &[0]).unwrap();
    qpdf_check(&save(&pdf.doc), None);
}

#[test]
fn large_document_operations_are_fast() {
    // 2,000 pages: every structural op must stay interactive.
    let bytes = fixtures::simple(2000);
    let t = Instant::now();
    let mut s = open(bytes).unwrap();
    let all: Vec<usize> = (0..2000).collect();
    s.apply(&Op::Rotate {
        pages: all,
        degrees: 90,
    })
    .unwrap();
    s.apply(&Op::Move {
        pages: vec![1999],
        to: 0,
    })
    .unwrap();
    s.apply(&Op::Delete {
        pages: (0..1000).collect(),
    })
    .unwrap();
    s.undo().unwrap();
    s.undo().unwrap();
    s.redo().unwrap();
    assert_eq!(s.state().summary.unwrap().page_count, 2000);
    let elapsed = t.elapsed();
    // Generous bound for unoptimised debug builds on slow CI machines.
    assert!(elapsed < Duration::from_secs(60), "took {elapsed:?}");
    eprintln!("2000-page workflow: {elapsed:?}");
}

#[test]
fn deep_outline_nesting_is_bounded() {
    let mut pdf = load(&fixtures::simple(2), None).unwrap();
    let p = pages::page_ids(&pdf.doc)[1];
    let mut parent = None;
    for i in 0..500 {
        let id = pages::add_outline_item(&mut pdf.doc, parent, &format!("level {i}"), p).unwrap();
        parent = Some(id);
    }
    // Deleting the target page cleans the chain without blowing the stack.
    pages::delete(&mut pdf.doc, &[1]).unwrap();
    let _ = save(&pdf.doc);
}
