mod common;

use common::*;
use opdf_core::assemble::{self, MergeInput, MergeOptions, SplitPlan};
use opdf_core::fixtures::FixtureOptions;
use opdf_core::lopdf::dictionary;
use opdf_core::lopdf::Object;
use opdf_core::pages;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn input(name: &str, pdf: opdf_core::LoadedPdf) -> MergeInput {
    MergeInput {
        name: name.into(),
        pdf,
        pages: None,
    }
}

#[test]
fn merge_two_documents_in_order() {
    let a = simple(2);
    let b = fixture(FixtureOptions {
        pages: 3,
        nested_tree: true,
        ..Default::default()
    });
    let doc = assemble::merge(vec![input("a", a), input("b", b)], MergeOptions::default()).unwrap();
    assert_eq!(
        labels(&doc),
        s(&["Page 1", "Page 2", "Page 1", "Page 2", "Page 3"])
    );
    qpdf_check(&save(&doc), None);
}

#[test]
fn merge_with_page_selection() {
    let a = simple(3);
    let b = simple(3);
    let doc = assemble::merge(
        vec![
            MergeInput {
                name: "a".into(),
                pdf: a,
                pages: Some(vec![2]),
            },
            MergeInput {
                name: "b".into(),
                pdf: b,
                pages: Some(vec![0, 1]),
            },
        ],
        MergeOptions::default(),
    )
    .unwrap();
    assert_eq!(labels(&doc), s(&["Page 3", "Page 1", "Page 2"]));
}

#[test]
fn merge_builds_bookmark_per_file_with_nested_outlines() {
    let a = fixture(FixtureOptions {
        pages: 2,
        outline: true,
        ..Default::default()
    });
    let b = fixture(FixtureOptions {
        pages: 2,
        outline: true,
        ..Default::default()
    });
    let doc = assemble::merge(
        vec![input("Report", a), input("Appendix", b)],
        MergeOptions {
            bookmark_per_file: true,
        },
    )
    .unwrap();
    let bytes = save(&doc);
    qpdf_check(&bytes, None);
    let doc = opdf_core::load(&bytes, None).unwrap().doc;
    let root = doc
        .catalog()
        .unwrap()
        .get(b"Outlines")
        .unwrap()
        .as_reference()
        .unwrap();
    let first = doc
        .get_dictionary(root)
        .unwrap()
        .get(b"First")
        .unwrap()
        .as_reference()
        .unwrap();
    let first = doc.get_dictionary(first).unwrap();
    assert_eq!(first.get(b"Title").unwrap().as_str().unwrap(), b"Report");
    assert_eq!(
        first.get(b"Count").unwrap().as_i64().unwrap(),
        -2,
        "file bookmarks start collapsed with 2 children"
    );
    let second = first.get(b"Next").unwrap().as_reference().unwrap();
    let second = doc.get_dictionary(second).unwrap();
    assert_eq!(second.get(b"Title").unwrap().as_str().unwrap(), b"Appendix");
    // The Appendix bookmark points at page 3 of the merged file.
    let dest = second.get(b"Dest").unwrap().as_array().unwrap()[0]
        .as_reference()
        .unwrap();
    assert_eq!(pages::page_ids(&doc)[2], dest);
}

#[test]
fn merge_keeps_form_fields_and_renames_collisions() {
    let a = fixture(FixtureOptions {
        pages: 1,
        form_field: true,
        ..Default::default()
    });
    let b = fixture(FixtureOptions {
        pages: 1,
        form_field: true,
        ..Default::default()
    });
    let doc = assemble::merge(vec![input("a", a), input("b", b)], MergeOptions::default()).unwrap();
    let acro = doc
        .catalog()
        .unwrap()
        .get_deref(b"AcroForm", &doc)
        .unwrap()
        .as_dict()
        .unwrap();
    let fields = acro.get(b"Fields").unwrap().as_array().unwrap();
    assert_eq!(fields.len(), 2);
    let names: Vec<Vec<u8>> = fields
        .iter()
        .map(|f| {
            doc.get_dictionary(f.as_reference().unwrap())
                .unwrap()
                .get(b"T")
                .unwrap()
                .as_str()
                .unwrap()
                .to_vec()
        })
        .collect();
    assert_eq!(names, vec![b"name".to_vec(), b"name_2".to_vec()]);
    qpdf_check(&save(&doc), None);
}

#[test]
fn merged_links_survive() {
    let a = fixture(FixtureOptions {
        pages: 3,
        link_to_last: true,
        ..Default::default()
    });
    let b = simple(1);
    let doc = assemble::merge(vec![input("a", a), input("b", b)], MergeOptions::default()).unwrap();
    let ids = pages::page_ids(&doc);
    let annots = doc
        .get_dictionary(ids[0])
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    let link = doc
        .get_dictionary(annots[0].as_reference().unwrap())
        .unwrap();
    let target = link.get(b"Dest").unwrap().as_array().unwrap()[0]
        .as_reference()
        .unwrap();
    assert_eq!(target, ids[2], "link still points at page 3");
}

#[test]
fn named_destinations_are_resolved_before_merging() {
    let mut a = simple(2);
    let p2 = pages::page_ids(&a.doc)[1];
    // A link using a named destination "intro".
    let link = a.doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Link",
        "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()],
        "Dest" => Object::string_literal("intro"),
    });
    let p1 = pages::page_ids(&a.doc)[0];
    a.doc
        .get_dictionary_mut(p1)
        .unwrap()
        .set("Annots", vec![Object::Reference(link)]);
    let names = a.doc.add_object(dictionary! {
        "Names" => vec![Object::string_literal("intro"), Object::Array(vec![Object::Reference(p2), "Fit".into()])],
    });
    a.doc
        .catalog_mut()
        .unwrap()
        .set("Names", dictionary! { "Dests" => names });
    let doc = assemble::merge(
        vec![input("a", a), input("b", simple(1))],
        MergeOptions::default(),
    )
    .unwrap();
    let ids = pages::page_ids(&doc);
    let annots = doc
        .get_dictionary(ids[0])
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    let link = doc
        .get_dictionary(annots[0].as_reference().unwrap())
        .unwrap();
    let dest = link.get(b"Dest").unwrap().as_array().unwrap();
    assert_eq!(dest[0].as_reference().unwrap(), ids[1]);
}

#[test]
fn split_every_n_and_ranges() {
    let pdf = simple(5);
    let parts = assemble::split(&pdf, &SplitPlan::EveryN { n: 2 }).unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(
        parts.iter().map(|p| p.label.clone()).collect::<Vec<_>>(),
        s(&["1-2", "3-4", "5"])
    );
    assert_eq!(labels(&parts[2].doc), s(&["Page 5"]));
    let parts = assemble::split(
        &pdf,
        &SplitPlan::Ranges {
            ranges: vec!["1,3".into(), "4-".into()],
        },
    )
    .unwrap();
    assert_eq!(labels(&parts[0].doc), s(&["Page 1", "Page 3"]));
    assert_eq!(labels(&parts[1].doc), s(&["Page 4", "Page 5"]));
    for p in &parts {
        qpdf_check(&save(&p.doc), None);
    }
    assert!(assemble::split(&pdf, &SplitPlan::EveryN { n: 0 }).is_err());
    assert_eq!(
        assemble::split(&pdf, &SplitPlan::SinglePages)
            .unwrap()
            .len(),
        5
    );
}

#[test]
fn insert_pages_from_another_document() {
    let mut target = simple(2);
    let src = fixture(FixtureOptions {
        pages: 3,
        nested_tree: true,
        outline: true,
        ..Default::default()
    });
    let n = assemble::insert_from(&mut target.doc, &src, Some(vec![2, 0]), 1, "src").unwrap();
    assert_eq!(n, 2);
    assert_eq!(
        labels(&target.doc),
        s(&["Page 1", "Page 3", "Page 1", "Page 2"])
    );
    let bytes = save(&target.doc);
    // The unselected source page was not carried over.
    assert_eq!(
        labels_of(&bytes, None)
            .iter()
            .filter(|l| *l == "Page 2")
            .count(),
        1
    );
    qpdf_check(&bytes, None);
}

#[test]
fn merge_requires_at_least_one_input() {
    assert!(assemble::merge(vec![], MergeOptions::default()).is_err());
}
