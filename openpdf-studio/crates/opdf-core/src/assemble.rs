//! Multi-document operations: merge, split, insert pages from another file.

use crate::error::{Error, Result};
use crate::load::LoadedPdf;
use crate::pages;
use lopdf::{Document, Object, ObjectId};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

pub struct MergeInput {
    /// Shown as the bookmark title when `bookmark_per_file` is set.
    pub name: String,
    pub pdf: LoadedPdf,
    /// 0-based pages to take (in order); `None` = all.
    pub pages: Option<Vec<usize>>,
}

#[derive(Clone, Copy, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MergeOptions {
    /// Add a top-level bookmark per input file, with that file's own
    /// bookmarks nested underneath.
    pub bookmark_per_file: bool,
}

/// Refuse to take pages out of a document whose author forbade assembly.
pub fn require_assemble(pdf: &LoadedPdf, name: &str) -> Result<()> {
    if pdf.permissions().assemble {
        Ok(())
    } else {
        Err(Error::denied(format!(
            "\"{name}\" does not allow its pages to be reused; open it with the permissions password first"
        )))
    }
}

fn subset(doc: &Document, pages: &Option<Vec<usize>>) -> Result<Document> {
    match pages {
        None => Ok(doc.clone()),
        Some(p) => pages::extract(doc, p),
    }
}

/// Merge inputs in order into a new, unencrypted document.
///
/// The first input's catalog-level data (metadata, viewer preferences...) is
/// kept. Named destinations are converted to explicit ones first so links and
/// bookmarks keep working after the catalogs are combined.
pub fn merge(inputs: Vec<MergeInput>, opts: MergeOptions) -> Result<Document> {
    if inputs.is_empty() {
        return Err(Error::invalid_arg("choose at least one file to merge"));
    }
    for i in &inputs {
        require_assemble(&i.pdf, &i.name)?;
    }
    let mut iter = inputs.into_iter();
    let first = iter.next().expect("non-empty");
    let mut target = subset(&first.pdf.doc, &first.pages)?;
    resolve_named_destinations(&mut target);
    target.encryption_state = None;

    if opts.bookmark_per_file {
        let first_page = pages::page_ids(&target)[0];
        let items = pages::take_outline(&mut target);
        let file_item = pages::add_outline_item(&mut target, None, &first.name, first_page)?;
        pages::append_outline_children(&mut target, file_item, &items);
        collapse(&mut target, file_item);
    }

    for input in iter {
        let mut src = subset(&input.pdf.doc, &input.pages)?;
        resolve_named_destinations(&mut src);
        let at = pages::page_count(&target);
        let appended = pages::append_document(&mut target, src, at)?;
        if opts.bookmark_per_file {
            let item = pages::add_outline_item(&mut target, None, &input.name, appended.pages[0])?;
            pages::append_outline_children(&mut target, item, &appended.outline_items);
            collapse(&mut target, item);
        } else if !appended.outline_items.is_empty() {
            let root = pages::outline_root(&mut target)?;
            pages::append_outline_children(&mut target, root, &appended.outline_items);
        }
    }
    // Names (named destinations, embedded files...) of the first document may
    // now be incomplete for merged content; explicit destinations were already
    // substituted, so drop only the destination name tree to avoid ambiguity.
    if let Ok(names) = target.catalog_mut().and_then(|c| c.get_mut(b"Names")) {
        if let Ok(d) = names.as_dict_mut() {
            d.remove(b"Dests");
        }
    }
    if let Ok(c) = target.catalog_mut() {
        c.remove(b"Dests");
    }
    target.prune_objects();
    Ok(target)
}

/// Make a file-level bookmark start collapsed (negative Count).
fn collapse(doc: &mut Document, item: ObjectId) {
    if let Ok(d) = doc.get_dictionary_mut(item) {
        if let Ok(c) = d.get(b"Count").and_then(Object::as_i64) {
            d.set("Count", Object::Integer(-c.abs()));
        }
    }
}

/// Insert pages of `source` into `target` before index `at`.
pub fn insert_from(
    target: &mut Document,
    source: &LoadedPdf,
    pages_sel: Option<Vec<usize>>,
    at: usize,
    name: &str,
) -> Result<usize> {
    require_assemble(source, name)?;
    let mut src = subset(&source.doc, &pages_sel)?;
    resolve_named_destinations(&mut src);
    resolve_named_destinations(target);
    let appended = pages::append_document(target, src, at)?;
    // The inserted document's bookmarks are kept, appended at the end.
    if !appended.outline_items.is_empty() {
        let root = pages::outline_root(target)?;
        pages::append_outline_children(target, root, &appended.outline_items);
    }
    target.prune_objects();
    Ok(appended.pages.len())
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SplitPlan {
    /// One file per `n` pages.
    EveryN { n: usize },
    /// One file per page.
    SinglePages,
    /// One file per range spec (e.g. `["1-3", "4-", "odd"]`).
    Ranges { ranges: Vec<String> },
}

pub struct SplitPart {
    /// 1-based label for file naming, e.g. `"1-3"`.
    pub label: String,
    pub doc: Document,
}

/// Plan the page groups for `plan` on a `count`-page document.
pub fn split_groups(plan: &SplitPlan, count: usize) -> Result<Vec<Vec<usize>>> {
    let groups: Vec<Vec<usize>> = match plan {
        SplitPlan::EveryN { n } => {
            if *n == 0 {
                return Err(Error::invalid_arg("pages per file must be at least 1"));
            }
            (0..count)
                .collect::<Vec<_>>()
                .chunks(*n)
                .map(<[usize]>::to_vec)
                .collect()
        }
        SplitPlan::SinglePages => (0..count).map(|i| vec![i]).collect(),
        SplitPlan::Ranges { ranges } => ranges
            .iter()
            .filter(|r| !r.trim().is_empty())
            .map(|r| crate::ranges::parse(r, count))
            .collect::<Result<_>>()?,
    };
    if groups.is_empty() {
        return Err(Error::invalid_arg("the split produces no files"));
    }
    if groups.len() > 10_000 {
        return Err(Error::invalid_arg(
            "the split would create more than 10,000 files",
        ));
    }
    Ok(groups)
}

pub fn split(pdf: &LoadedPdf, plan: &SplitPlan) -> Result<Vec<SplitPart>> {
    require_assemble(pdf, "this document")?;
    let count = pages::page_count(&pdf.doc);
    let groups = split_groups(plan, count)?;
    let mut base = pdf.doc.clone();
    resolve_named_destinations(&mut base);
    base.encryption_state = None;
    groups
        .into_iter()
        .map(|g| {
            Ok(SplitPart {
                label: crate::ranges::format(&g),
                doc: pages::extract(&base, &g)?,
            })
        })
        .collect()
}

/// Replace named destinations in outline items and link annotations with
/// the explicit destination they resolve to.
pub fn resolve_named_destinations(doc: &mut Document) {
    let map = named_destinations(doc);
    if map.is_empty() {
        return;
    }
    let resolve = |o: &Object| -> Option<Object> {
        let key = match o {
            Object::String(s, _) => s.clone(),
            Object::Name(n) => n.clone(),
            _ => return None,
        };
        map.get(&key).cloned()
    };
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    for id in ids {
        let Some(Object::Dictionary(d)) = doc.objects.get_mut(&id) else {
            continue;
        };
        if let Ok(dest) = d.get(b"Dest") {
            if let Some(r) = resolve(dest) {
                d.set("Dest", r);
            }
        }
        if let Ok(Object::Dictionary(a)) = d.get_mut(b"A") {
            if a.get(b"S").and_then(Object::as_name).ok() == Some(b"GoTo") {
                if let Some(r) = a.get(b"D").ok().and_then(resolve) {
                    a.set("D", r);
                }
            }
        }
    }
}

fn named_destinations(doc: &Document) -> HashMap<Vec<u8>, Object> {
    let mut map = HashMap::new();
    let Ok(catalog) = doc.catalog() else {
        return map;
    };
    let normalize = |o: &Object| -> Option<Object> {
        let o = match o {
            Object::Reference(r) => doc.get_object(*r).ok()?.clone(),
            o => o.clone(),
        };
        match o {
            Object::Array(_) => Some(o),
            Object::Dictionary(d) => d.get(b"D").ok().cloned(),
            _ => None,
        }
    };
    // PDF 1.1 style /Dests dictionary.
    if let Ok(dests) = catalog.get_deref(b"Dests", doc).and_then(Object::as_dict) {
        for (k, v) in dests.iter() {
            if let Some(d) = normalize(v) {
                map.insert(k.clone(), d);
            }
        }
    }
    // PDF 1.2+ name tree.
    let tree = catalog
        .get_deref(b"Names", doc)
        .and_then(Object::as_dict)
        .and_then(|n| n.get_deref(b"Dests", doc))
        .and_then(Object::as_dict);
    if let Ok(root) = tree {
        let mut stack = vec![(root.clone(), 0usize)];
        let mut seen = HashSet::new();
        while let Some((node, depth)) = stack.pop() {
            if depth > 32 {
                continue;
            }
            if let Ok(names) = node.get_deref(b"Names", doc).and_then(Object::as_array) {
                for pair in names.chunks(2) {
                    if let [Object::String(k, _), v] = pair {
                        if let Some(d) = normalize(v) {
                            map.insert(k.clone(), d);
                        }
                    }
                }
            }
            if let Ok(kids) = node.get_deref(b"Kids", doc).and_then(Object::as_array) {
                for k in kids {
                    if let Ok(r) = k.as_reference() {
                        if seen.insert(r) {
                            if let Ok(d) = doc.get_dictionary(r) {
                                stack.push((d.clone(), depth + 1));
                            }
                        }
                    }
                }
            }
        }
    }
    map
}
