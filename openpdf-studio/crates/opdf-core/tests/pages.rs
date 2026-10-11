mod common;

use common::*;
use opdf_core::fixtures::FixtureOptions;
use opdf_core::lopdf::dictionary;
use opdf_core::lopdf::Object;
use opdf_core::pages;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn rotate_accumulates_and_normalises() {
    let mut pdf = simple(3);
    pages::rotate(&mut pdf.doc, &[0, 2], 90).unwrap();
    pages::rotate(&mut pdf.doc, &[0], 90).unwrap();
    pages::rotate(&mut pdf.doc, &[2], -180).unwrap();
    let ids = pages::page_ids(&pdf.doc);
    assert_eq!(pages::rotation(&pdf.doc, ids[0]), 180);
    assert_eq!(pages::rotation(&pdf.doc, ids[1]), 0);
    assert_eq!(pages::rotation(&pdf.doc, ids[2]), 270);
    assert!(pages::rotate(&mut pdf.doc, &[0], 45).is_err());
    assert!(pages::rotate(&mut pdf.doc, &[3], 90).is_err());
    qpdf_check(&save(&pdf.doc), None);
}

#[test]
fn rotation_respects_inherited_value() {
    let mut pdf = fixture(FixtureOptions {
        pages: 5,
        nested_tree: true,
        ..Default::default()
    });
    // Put an inherited /Rotate on the first intermediate node.
    let first = pages::page_ids(&pdf.doc)[0];
    let parent = pdf
        .doc
        .get_dictionary(first)
        .unwrap()
        .get(b"Parent")
        .unwrap()
        .as_reference()
        .unwrap();
    pdf.doc
        .get_dictionary_mut(parent)
        .unwrap()
        .set("Rotate", 90);
    assert_eq!(pages::rotation(&pdf.doc, first), 90);
    pages::rotate(&mut pdf.doc, &[0], 90).unwrap();
    assert_eq!(pages::rotation(&pdf.doc, first), 180);
}

#[test]
fn delete_pages_and_keep_order() {
    let mut pdf = simple(5);
    pages::delete(&mut pdf.doc, &[1, 3]).unwrap();
    assert_eq!(labels(&pdf.doc), s(&["Page 1", "Page 3", "Page 5"]));
    let bytes = save(&pdf.doc);
    assert_eq!(labels_of(&bytes, None), s(&["Page 1", "Page 3", "Page 5"]));
    qpdf_check(&bytes, None);
}

#[test]
fn cannot_delete_every_page() {
    let mut pdf = simple(2);
    let err = pages::delete(&mut pdf.doc, &[0, 1]).unwrap_err();
    assert_eq!(err.code, opdf_core::ErrorCode::InvalidArgument);
    assert!(pages::delete(&mut pdf.doc, &[]).is_err());
}

#[test]
fn deleted_page_content_is_not_left_in_the_file() {
    // Outline and link both reference the last page; after deleting it the
    // page's content must not survive in the saved bytes.
    let mut pdf = fixture(FixtureOptions {
        pages: 3,
        outline: true,
        link_to_last: true,
        ..Default::default()
    });
    pages::delete(&mut pdf.doc, &[2]).unwrap();
    let bytes = save(&pdf.doc);
    assert!(
        !contains(&bytes, b"(Page 3)"),
        "deleted page content leaked into output"
    );
    assert!(contains(&bytes, b"(Page 2)"));
    // Outline item for the deleted page is gone, others remain and are linked.
    let doc = opdf_core::load(&bytes, None).unwrap().doc;
    let root = doc
        .catalog()
        .unwrap()
        .get(b"Outlines")
        .unwrap()
        .as_reference()
        .unwrap();
    let root = doc.get_dictionary(root).unwrap();
    assert_eq!(root.get(b"Count").unwrap().as_i64().unwrap(), 2);
    qpdf_check(&bytes, None);
}

#[test]
fn nested_page_tree_is_flattened_with_inherited_attributes() {
    let mut pdf = fixture(FixtureOptions {
        pages: 9,
        nested_tree: true,
        ..Default::default()
    });
    pages::reorder(&mut pdf.doc, &[8, 7, 6, 5, 4, 3, 2, 1, 0]).unwrap();
    let ids = pages::page_ids(&pdf.doc);
    for id in &ids {
        let d = pdf.doc.get_dictionary(*id).unwrap();
        assert!(
            d.has(b"MediaBox") && d.has(b"Resources"),
            "inherited attributes materialised"
        );
    }
    let bytes = save(&pdf.doc);
    let l = labels_of(&bytes, None);
    assert_eq!(l.first().unwrap(), "Page 9");
    assert_eq!(l.last().unwrap(), "Page 1");
    qpdf_check(&bytes, None);
}

#[test]
fn reorder_requires_permutation() {
    let mut pdf = simple(3);
    assert!(pages::reorder(&mut pdf.doc, &[0, 1]).is_err());
    assert!(pages::reorder(&mut pdf.doc, &[0, 1, 1]).is_err());
    assert!(pages::reorder(&mut pdf.doc, &[0, 1, 3]).is_err());
    pages::reorder(&mut pdf.doc, &[2, 0, 1]).unwrap();
    assert_eq!(labels(&pdf.doc), s(&["Page 3", "Page 1", "Page 2"]));
}

#[test]
fn move_pages_like_drag_and_drop() {
    assert_eq!(
        pages::move_permutation(5, &[3], 0).unwrap(),
        vec![3, 0, 1, 2, 4]
    );
    assert_eq!(
        pages::move_permutation(5, &[0, 1], 5).unwrap(),
        vec![2, 3, 4, 0, 1]
    );
    assert_eq!(
        pages::move_permutation(5, &[0, 4], 2).unwrap(),
        vec![1, 0, 4, 2, 3]
    );
    let mut pdf = simple(4);
    pages::move_pages(&mut pdf.doc, &[3], 1).unwrap();
    assert_eq!(
        labels(&pdf.doc),
        s(&["Page 1", "Page 4", "Page 2", "Page 3"])
    );
}

#[test]
fn insert_blank_pages() {
    let mut pdf = simple(2);
    pages::insert_blank(&mut pdf.doc, 1, pages::A4, 2).unwrap();
    assert_eq!(labels(&pdf.doc), s(&["Page 1", "", "", "Page 2"]));
    let ids = pages::page_ids(&pdf.doc);
    let (w, h) = pages::page_size(&pdf.doc, ids[1]);
    assert!((w - 595.276).abs() < 0.01 && (h - 841.89).abs() < 0.01);
    assert!(pages::insert_blank(&mut pdf.doc, 9, pages::A4, 1).is_err());
    assert!(pages::insert_blank(&mut pdf.doc, 0, (1.0, 1.0), 1).is_err());
    qpdf_check(&save(&pdf.doc), None);
}

#[test]
fn duplicate_pages_with_annotations() {
    let mut pdf = fixture(FixtureOptions {
        pages: 2,
        link_to_last: true,
        form_field: true,
        ..Default::default()
    });
    pages::duplicate(&mut pdf.doc, &[0]).unwrap();
    assert_eq!(labels(&pdf.doc), s(&["Page 1", "Page 1", "Page 2"]));
    let ids = pages::page_ids(&pdf.doc);
    let annots = |i: usize| -> Vec<Object> {
        pdf.doc
            .get_dictionary(ids[i])
            .unwrap()
            .get(b"Annots")
            .map(|a| a.as_array().unwrap().clone())
            .unwrap_or_default()
    };
    // Original keeps link + widget; copy gets only an independent link.
    assert_eq!(annots(0).len(), 2);
    assert_eq!(annots(1).len(), 1);
    assert_ne!(
        annots(0)[0],
        annots(1)[0],
        "annotations must not be shared between pages"
    );
    qpdf_check(&save(&pdf.doc), None);
}

#[test]
fn extract_selected_pages_in_given_order() {
    let pdf = fixture(FixtureOptions {
        pages: 6,
        outline: true,
        ..Default::default()
    });
    let out = pages::extract(&pdf.doc, &[4, 1, 4]).unwrap();
    assert_eq!(labels(&out), s(&["Page 5", "Page 2", "Page 5"]));
    let bytes = save(&out);
    assert!(!contains(&bytes, b"(Page 1)"));
    assert!(!contains(&bytes, b"(Page 6)"));
    qpdf_check(&bytes, None);
}

#[test]
fn deleting_a_form_page_prunes_its_field() {
    let mut pdf = fixture(FixtureOptions {
        pages: 2,
        form_field: true,
        ..Default::default()
    });
    pages::delete(&mut pdf.doc, &[0]).unwrap();
    let acro = pdf
        .doc
        .catalog()
        .unwrap()
        .get(b"AcroForm")
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(acro.get(b"Fields").unwrap().as_array().unwrap().is_empty());
    qpdf_check(&save(&pdf.doc), None);
}

#[test]
fn page_labels_are_dropped_after_reordering() {
    let mut pdf = simple(3);
    pdf.doc
        .catalog_mut()
        .unwrap()
        .set("PageLabels", dictionary! { "Nums" => vec![0.into()] });
    pages::rotate(&mut pdf.doc, &[0], 90).unwrap();
    assert!(
        pdf.doc.catalog().unwrap().has(b"PageLabels"),
        "rotation keeps labels"
    );
    pages::reorder(&mut pdf.doc, &[1, 0, 2]).unwrap();
    assert!(!pdf.doc.catalog().unwrap().has(b"PageLabels"));
}
