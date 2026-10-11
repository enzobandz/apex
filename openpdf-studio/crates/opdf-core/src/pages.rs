//! Page-level structural operations.
//!
//! All operations work on a flat page list: inherited attributes are copied
//! onto each page ([`materialize_inherited`]) and the page tree is rebuilt as
//! a single `/Pages` node ([`rebuild_page_tree`]). This is simple, valid for
//! every reader, and makes reorder/insert/delete trivially correct.
//!
//! Deleting pages *scrubs* every reference to them (outlines, links, named
//! destinations, structure tree) and prunes unreachable objects, so content of
//! a deleted page does not linger in the saved file.

use crate::error::{Error, Result};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Attributes a page may inherit from its ancestors (ISO 32000-1, 7.7.3.4).
const INHERITABLE: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Default page size (US Letter is not assumed: A4 in points).
pub const A4: (f32, f32) = (595.276, 841.89);
pub const LETTER: (f32, f32) = (612.0, 792.0);

/// Ordered page object ids.
pub fn page_ids(doc: &Document) -> Vec<ObjectId> {
    doc.page_iter().collect()
}

pub fn page_count(doc: &Document) -> usize {
    doc.get_pages().len()
}

fn root_pages_id(doc: &Document) -> Result<ObjectId> {
    doc.catalog()?
        .get(b"Pages")
        .and_then(Object::as_reference)
        .map_err(|_| Error::invalid_pdf("the document has no page tree"))
}

/// Look up `key` on the page or the nearest ancestor that defines it.
pub fn inherited(doc: &Document, page_id: ObjectId, key: &[u8]) -> Option<Object> {
    let mut seen = HashSet::new();
    let mut current = Some(page_id);
    while let Some(id) = current {
        if !seen.insert(id) {
            return None; // cycle in /Parent chain
        }
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(v) = dict.get(key) {
            return Some(v.clone());
        }
        current = dict.get(b"Parent").and_then(Object::as_reference).ok();
    }
    None
}

/// Copy inheritable attributes onto the page itself so it no longer depends
/// on its position in the page tree.
pub fn materialize_inherited(doc: &mut Document, page_id: ObjectId) -> Result<()> {
    let mut found: Vec<(&[u8], Object)> = Vec::new();
    {
        let dict = doc.get_dictionary(page_id)?;
        for key in INHERITABLE {
            if !dict.has(key) {
                if let Some(v) = inherited(doc, page_id, key) {
                    found.push((key, v));
                }
            }
        }
    }
    let dict = doc.get_dictionary_mut(page_id)?;
    for (k, v) in found {
        dict.set(k, v);
    }
    // MediaBox is required; fall back to A4 for broken files.
    if !dict.has(b"MediaBox") {
        dict.set("MediaBox", rect(0.0, 0.0, A4.0, A4.1));
    }
    // Resources is required (may be empty).
    if !dict.has(b"Resources") {
        dict.set("Resources", Dictionary::new());
    }
    Ok(())
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Object {
    Object::Array(vec![
        Object::Real(x0),
        Object::Real(y0),
        Object::Real(x1),
        Object::Real(y1),
    ])
}

/// Replace the page tree with a single `/Pages` node whose kids are `pages`.
pub fn rebuild_page_tree(doc: &mut Document, pages: &[ObjectId]) -> Result<()> {
    if pages.is_empty() {
        return Err(Error::invalid_arg("a PDF must contain at least one page"));
    }
    for &p in pages {
        materialize_inherited(doc, p)?;
    }
    let root = root_pages_id(doc)?;
    // Collect intermediate /Pages nodes so they can be removed.
    let mut stale = Vec::new();
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Ok(dict) = doc.get_dictionary(id) else {
            continue;
        };
        if dict.has_type(b"Pages") || id == root {
            if id != root {
                stale.push(id);
            }
            if let Ok(kids) = dict.get(b"Kids").and_then(Object::as_array) {
                stack.extend(kids.iter().filter_map(|k| k.as_reference().ok()));
            }
        }
    }
    for id in stale {
        doc.objects.remove(&id);
    }
    for &p in pages {
        doc.get_dictionary_mut(p)?
            .set("Parent", Object::Reference(root));
    }
    let root_dict = doc.get_dictionary_mut(root)?;
    root_dict.set("Type", Object::Name(b"Pages".to_vec()));
    root_dict.set(
        "Kids",
        Object::Array(pages.iter().map(|&p| Object::Reference(p)).collect()),
    );
    root_dict.set("Count", Object::Integer(pages.len() as i64));
    // Attributes were materialised onto each page; inheritable values left on
    // the root would silently apply to pages inserted later.
    for key in INHERITABLE {
        root_dict.remove(key);
    }
    root_dict.remove(b"Parent");
    Ok(())
}

/// Validate zero-based page indices against `count`; returns them sorted and
/// de-duplicated.
pub fn check_indices(indices: &[usize], count: usize) -> Result<Vec<usize>> {
    if indices.is_empty() {
        return Err(Error::invalid_arg("no pages selected"));
    }
    let set: BTreeSet<usize> = indices.iter().copied().collect();
    if let Some(&bad) = set.iter().find(|&&i| i >= count) {
        return Err(Error::invalid_arg(format!(
            "page {} does not exist (the document has {count} pages)",
            bad + 1
        )));
    }
    Ok(set.into_iter().collect())
}

/// Current effective rotation of a page in degrees (0, 90, 180, 270).
pub fn rotation(doc: &Document, page_id: ObjectId) -> i64 {
    let r = inherited(doc, page_id, b"Rotate")
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0);
    normalize_rotation(r)
}

fn normalize_rotation(r: i64) -> i64 {
    // /Rotate must be a multiple of 90; anything else is treated as 0.
    if r % 90 != 0 {
        return 0;
    }
    r.rem_euclid(360)
}

/// Rotate the given pages by `delta` degrees (multiple of 90, may be negative).
pub fn rotate(doc: &mut Document, indices: &[usize], delta: i64) -> Result<()> {
    if delta % 90 != 0 {
        return Err(Error::invalid_arg("rotation must be a multiple of 90°"));
    }
    let ids = page_ids(doc);
    for i in check_indices(indices, ids.len())? {
        let id = ids[i];
        let new = normalize_rotation(rotation(doc, id) + delta);
        doc.get_dictionary_mut(id)?
            .set("Rotate", Object::Integer(new));
    }
    Ok(())
}

/// Delete pages; at least one page must remain.
pub fn delete(doc: &mut Document, indices: &[usize]) -> Result<()> {
    let ids = page_ids(doc);
    let del = check_indices(indices, ids.len())?;
    if del.len() == ids.len() {
        return Err(Error::invalid_arg(
            "cannot delete every page — a PDF needs at least one page",
        ));
    }
    let del_set: HashSet<usize> = del.into_iter().collect();
    let keep: Vec<ObjectId> = ids
        .iter()
        .enumerate()
        .filter(|(i, _)| !del_set.contains(i))
        .map(|(_, &id)| id)
        .collect();
    let removed: HashSet<ObjectId> = ids
        .iter()
        .enumerate()
        .filter(|(i, _)| del_set.contains(i))
        .map(|(_, &id)| id)
        .collect();
    rebuild_page_tree(doc, &keep)?;
    remove_pages_and_scrub(doc, &removed)?;
    structure_changed(doc);
    Ok(())
}

/// Remove page objects and every reference to them, then prune.
fn remove_pages_and_scrub(doc: &mut Document, removed: &HashSet<ObjectId>) -> Result<()> {
    // Annotations living on removed pages must go too (form widgets are also
    // referenced from /AcroForm /Fields).
    let mut removed_annots: HashSet<ObjectId> = HashSet::new();
    for &id in removed {
        if let Ok(dict) = doc.get_dictionary(id) {
            if let Ok(annots) = dict.get(b"Annots") {
                let annots = match annots {
                    Object::Reference(r) => doc.get_object(*r).and_then(Object::as_array).ok(),
                    other => other.as_array().ok(),
                };
                if let Some(a) = annots {
                    removed_annots.extend(a.iter().filter_map(|o| o.as_reference().ok()));
                }
            }
        }
    }
    for id in removed {
        doc.objects.remove(id);
    }
    prune_form_fields(doc, &removed_annots);
    for id in &removed_annots {
        doc.objects.remove(id);
    }
    let gone: HashSet<ObjectId> = removed.union(&removed_annots).copied().collect();
    scrub_references(doc, &gone);
    clean_outline(doc);
    doc.prune_objects();
    Ok(())
}

/// Replace every reference to an object in `gone` with `null`.
pub(crate) fn scrub_references(doc: &mut Document, gone: &HashSet<ObjectId>) {
    fn walk(obj: &mut Object, gone: &HashSet<ObjectId>) {
        match obj {
            Object::Reference(id) if gone.contains(id) => *obj = Object::Null,
            Object::Array(a) => a.iter_mut().for_each(|o| walk(o, gone)),
            Object::Dictionary(d) => d.iter_mut().for_each(|(_, o)| walk(o, gone)),
            Object::Stream(s) => s.dict.iter_mut().for_each(|(_, o)| walk(o, gone)),
            _ => {}
        }
    }
    for obj in doc.objects.values_mut() {
        walk(obj, gone);
    }
    doc.trailer.iter_mut().for_each(|(_, o)| walk(o, gone));
}

/// Drop terminal form fields whose widget annotation was removed, and parent
/// fields left without kids.
fn prune_form_fields(doc: &mut Document, removed_annots: &HashSet<ObjectId>) {
    if removed_annots.is_empty() {
        return;
    }
    let Ok(acro_obj) = doc.catalog().and_then(|c| c.get(b"AcroForm")).cloned() else {
        return;
    };
    let acro_id = acro_obj.as_reference().ok();
    let fields = match &acro_obj {
        Object::Reference(r) => doc
            .get_dictionary(*r)
            .ok()
            .and_then(|d| d.get(b"Fields").ok().cloned()),
        Object::Dictionary(d) => d.get(b"Fields").ok().cloned(),
        _ => None,
    };
    let Some(fields) = fields else { return };
    let fields = match fields {
        Object::Reference(r) => doc.get_object(r).ok().cloned(),
        o => Some(o),
    };
    let Some(Object::Array(list)) = fields else {
        return;
    };

    fn keep_field(
        doc: &mut Document,
        id: ObjectId,
        removed: &HashSet<ObjectId>,
        depth: usize,
    ) -> bool {
        if removed.contains(&id) || depth > 64 {
            return depth > 64; // keep pathological trees untouched
        }
        let kids = doc
            .get_dictionary(id)
            .ok()
            .and_then(|d| d.get(b"Kids").ok())
            .and_then(|k| k.as_array().ok())
            .map(|a| {
                a.iter()
                    .filter_map(|o| o.as_reference().ok())
                    .collect::<Vec<_>>()
            });
        let Some(kids) = kids else { return true };
        if kids.is_empty() {
            return true;
        }
        let kept: Vec<ObjectId> = kids
            .into_iter()
            .filter(|&k| keep_field(doc, k, removed, depth + 1))
            .collect();
        if kept.is_empty() {
            return false;
        }
        if let Ok(d) = doc.get_dictionary_mut(id) {
            d.set(
                "Kids",
                Object::Array(kept.into_iter().map(Object::Reference).collect()),
            );
        }
        true
    }

    let ids: Vec<ObjectId> = list.iter().filter_map(|o| o.as_reference().ok()).collect();
    let kept: Vec<Object> = ids
        .into_iter()
        .filter(|&id| keep_field(doc, id, removed_annots, 0))
        .map(Object::Reference)
        .collect();
    let new_fields = Object::Array(kept);
    match acro_id {
        Some(id) => {
            if let Ok(d) = doc.get_dictionary_mut(id) {
                d.set("Fields", new_fields);
            }
        }
        None => {
            if let Ok(Object::Dictionary(d)) =
                doc.catalog_mut().and_then(|c| c.get_mut(b"AcroForm"))
            {
                d.set("Fields", new_fields);
            }
        }
    }
}

/// Remove outline items whose explicit destination now points nowhere and
/// that have no children; relinks the remaining items.
pub(crate) fn clean_outline(doc: &mut Document) {
    let Some(root) = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Outlines").ok())
        .and_then(|o| o.as_reference().ok())
    else {
        return;
    };
    let mut visited = HashSet::new();
    let remaining = clean_outline_level(doc, root, &mut visited, 0);
    if remaining == 0 {
        if let Ok(c) = doc.catalog_mut() {
            c.remove(b"Outlines");
        }
    }
}

fn outline_children(
    doc: &Document,
    parent: ObjectId,
    visited: &mut HashSet<ObjectId>,
) -> Vec<ObjectId> {
    let mut out = Vec::new();
    let mut cur = doc
        .get_dictionary(parent)
        .ok()
        .and_then(|d| d.get(b"First").ok())
        .and_then(|o| o.as_reference().ok());
    while let Some(id) = cur {
        if !visited.insert(id) || out.len() > 100_000 {
            break;
        }
        out.push(id);
        cur = doc
            .get_dictionary(id)
            .ok()
            .and_then(|d| d.get(b"Next").ok())
            .and_then(|o| o.as_reference().ok());
    }
    out
}

/// Returns the number of visible descendants kept under `parent`.
fn clean_outline_level(
    doc: &mut Document,
    parent: ObjectId,
    visited: &mut HashSet<ObjectId>,
    depth: usize,
) -> usize {
    if depth > 64 {
        return 1;
    }
    let children = outline_children(doc, parent, visited);
    let mut kept = Vec::new();
    for child in children {
        let descendants = clean_outline_level(doc, child, visited, depth + 1);
        let dead = descendants == 0 && outline_target_is_dead(doc, child);
        if !dead {
            kept.push(child);
        }
    }
    relink_outline(doc, parent, &kept);
    kept.len()
}

fn outline_target_is_dead(doc: &Document, item: ObjectId) -> bool {
    let Ok(d) = doc.get_dictionary(item) else {
        return true;
    };
    let dest = if let Ok(dest) = d.get(b"Dest") {
        Some(dest)
    } else if let Ok(a) = d.get(b"A") {
        let a = match a {
            Object::Reference(r) => doc.get_dictionary(*r).ok(),
            o => o.as_dict().ok(),
        };
        match a {
            Some(a) if a.get(b"S").and_then(Object::as_name).ok() == Some(b"GoTo") => {
                a.get(b"D").ok()
            }
            // Other actions (URI, Launch...) are not page-bound.
            _ => return false,
        }
    } else {
        // An item without any target is legal (a heading); keep it.
        return false;
    };
    let dest = match dest {
        Some(Object::Reference(r)) => doc.get_object(*r).ok(),
        other => other,
    };
    match dest {
        Some(Object::Array(a)) => matches!(a.first(), Some(Object::Null) | None),
        Some(Object::Dictionary(d)) => matches!(
            d.get(b"D").ok(),
            Some(Object::Array(a)) if matches!(a.first(), Some(Object::Null) | None)
        ),
        Some(Object::Null) => true,
        _ => false, // named destination: cannot cheaply prove it is dead
    }
}

fn relink_outline(doc: &mut Document, parent: ObjectId, kids: &[ObjectId]) {
    for (i, &k) in kids.iter().enumerate() {
        if let Ok(d) = doc.get_dictionary_mut(k) {
            d.set("Parent", Object::Reference(parent));
            match i.checked_sub(1).map(|j| kids[j]) {
                Some(p) => d.set("Prev", Object::Reference(p)),
                None => {
                    d.remove(b"Prev");
                }
            }
            match kids.get(i + 1) {
                Some(&n) => d.set("Next", Object::Reference(n)),
                None => {
                    d.remove(b"Next");
                }
            }
        }
    }
    if let Ok(d) = doc.get_dictionary_mut(parent) {
        match (kids.first(), kids.last()) {
            (Some(&f), Some(&l)) => {
                d.set("First", Object::Reference(f));
                d.set("Last", Object::Reference(l));
                // Keep the open/closed sign of an existing Count.
                let open = d
                    .get(b"Count")
                    .and_then(Object::as_i64)
                    .map(|c| c >= 0)
                    .unwrap_or(true);
                let n = kids.len() as i64;
                d.set("Count", Object::Integer(if open { n } else { -n }));
            }
            _ => {
                d.remove(b"First");
                d.remove(b"Last");
                d.remove(b"Count");
            }
        }
    }
}

/// Page labels and thumbnails describe the old page order; drop them after
/// structural changes rather than show wrong labels.
pub(crate) fn structure_changed(doc: &mut Document) {
    if let Ok(c) = doc.catalog_mut() {
        c.remove(b"PageLabels");
    }
}

/// Reorder pages: `order[i]` is the old index of the page that ends up at
/// position `i`. Must be a permutation.
pub fn reorder(doc: &mut Document, order: &[usize]) -> Result<()> {
    let ids = page_ids(doc);
    if order.len() != ids.len() {
        return Err(Error::invalid_arg(
            "the new order must list every page exactly once",
        ));
    }
    let mut seen = vec![false; ids.len()];
    for &o in order {
        if o >= ids.len() || std::mem::replace(&mut seen[o], true) {
            return Err(Error::invalid_arg(
                "the new order must list every page exactly once",
            ));
        }
    }
    let new: Vec<ObjectId> = order.iter().map(|&i| ids[i]).collect();
    rebuild_page_tree(doc, &new)?;
    structure_changed(doc);
    Ok(())
}

/// Compute the permutation that moves `indices` (in their current relative
/// order) so they start at position `to` in the *original* numbering
/// (i.e. "insert before page `to`"; `to == count` appends).
pub fn move_permutation(count: usize, indices: &[usize], to: usize) -> Result<Vec<usize>> {
    let moving = check_indices(indices, count)?;
    if to > count {
        return Err(Error::invalid_arg(
            "target position is outside the document",
        ));
    }
    let moving_set: HashSet<usize> = moving.iter().copied().collect();
    let mut order: Vec<usize> = Vec::with_capacity(count);
    for i in 0..count {
        if i == to {
            order.extend(&moving);
        }
        if !moving_set.contains(&i) {
            order.push(i);
        }
    }
    if to == count {
        order.extend(&moving);
    }
    Ok(order)
}

pub fn move_pages(doc: &mut Document, indices: &[usize], to: usize) -> Result<()> {
    let order = move_permutation(page_count(doc), indices, to)?;
    reorder(doc, &order)
}

/// Insert `count` blank pages of `size` (points) before index `at`.
pub fn insert_blank(doc: &mut Document, at: usize, size: (f32, f32), count: usize) -> Result<()> {
    let mut ids = page_ids(doc);
    if at > ids.len() {
        return Err(Error::invalid_arg(
            "insert position is outside the document",
        ));
    }
    if count == 0 || count > 1000 {
        return Err(Error::invalid_arg(
            "between 1 and 1000 pages can be inserted at once",
        ));
    }
    if !(size.0 >= 3.0 && size.1 >= 3.0 && size.0 <= 14400.0 && size.1 <= 14400.0) {
        return Err(Error::invalid_arg(
            "page size must be between 3 and 14400 points",
        ));
    }
    let root = root_pages_id(doc)?;
    let mut new_ids = Vec::with_capacity(count);
    for _ in 0..count {
        let content = doc.add_object(Stream::new(Dictionary::new(), Vec::new()));
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(root));
        page.set("MediaBox", rect(0.0, 0.0, size.0, size.1));
        page.set("Resources", Dictionary::new());
        page.set("Contents", Object::Reference(content));
        new_ids.push(doc.add_object(page));
    }
    ids.splice(at..at, new_ids);
    rebuild_page_tree(doc, &ids)?;
    structure_changed(doc);
    Ok(())
}

/// Insert a copy of each selected page directly after the original.
///
/// Page content and resources are shared (they are immutable streams), and
/// annotations are copied — except form widgets, which would create two
/// widgets for one field value, and popups, which belong to their parent.
pub fn duplicate(doc: &mut Document, indices: &[usize]) -> Result<()> {
    let ids = page_ids(doc);
    let sel: HashSet<usize> = check_indices(indices, ids.len())?.into_iter().collect();
    let mut out = Vec::with_capacity(ids.len() + sel.len());
    for (i, &id) in ids.iter().enumerate() {
        out.push(id);
        if sel.contains(&i) {
            materialize_inherited(doc, id)?;
            let mut copy = doc.get_dictionary(id)?.clone();
            copy.remove(b"StructParents");
            copy.remove(b"Annots");
            let new_id = doc.add_object(Object::Dictionary(copy));
            let annots = copy_annotations(doc, id, new_id)?;
            if !annots.is_empty() {
                doc.get_dictionary_mut(new_id)?
                    .set("Annots", Object::Array(annots));
            }
            out.push(new_id);
        }
    }
    rebuild_page_tree(doc, &out)?;
    structure_changed(doc);
    Ok(())
}

fn copy_annotations(doc: &mut Document, from: ObjectId, to: ObjectId) -> Result<Vec<Object>> {
    let annots = match doc.get_dictionary(from)?.get(b"Annots") {
        Ok(Object::Array(a)) => a.clone(),
        Ok(Object::Reference(r)) => doc.get_object(*r)?.as_array().cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for a in annots {
        let dict = match &a {
            Object::Reference(r) => doc.get_dictionary(*r).ok().cloned(),
            Object::Dictionary(d) => Some(d.clone()),
            _ => None,
        };
        let Some(mut d) = dict else { continue };
        let subtype = d.get(b"Subtype").and_then(Object::as_name).unwrap_or(b"");
        if subtype == b"Widget" || subtype == b"Popup" {
            continue;
        }
        d.remove(b"Popup");
        d.remove(b"StructParent");
        d.remove(b"IRT");
        d.remove(b"NM"); // unique names must stay unique
        d.set("P", Object::Reference(to));
        out.push(Object::Reference(doc.add_object(Object::Dictionary(d))));
    }
    Ok(out)
}

/// Keep only `indices` (in the given order; duplicates allowed) and drop
/// everything else, producing a stand-alone document.
pub fn extract(doc: &Document, indices: &[usize]) -> Result<Document> {
    let ids = page_ids(doc);
    if indices.is_empty() {
        return Err(Error::invalid_arg("no pages selected"));
    }
    if let Some(&bad) = indices.iter().find(|&&i| i >= ids.len()) {
        return Err(Error::invalid_arg(format!(
            "page {} does not exist",
            bad + 1
        )));
    }
    let mut out = doc.clone();
    let keep: HashSet<usize> = indices.iter().copied().collect();
    let drop: Vec<usize> = (0..ids.len()).filter(|i| !keep.contains(i)).collect();
    if !drop.is_empty() {
        delete(&mut out, &drop)?;
    }
    // Map original index -> object id (ids are unchanged by delete).
    let mut wanted: Vec<ObjectId> = Vec::with_capacity(indices.len());
    let mut used: HashSet<ObjectId> = HashSet::new();
    for &i in indices {
        let id = ids[i];
        if used.insert(id) {
            wanted.push(id);
        } else {
            // Repeated page: duplicate it.
            let mut copy = out.get_dictionary(id)?.clone();
            copy.remove(b"Annots");
            copy.remove(b"StructParents");
            wanted.push(out.add_object(Object::Dictionary(copy)));
        }
    }
    rebuild_page_tree(&mut out, &wanted)?;
    structure_changed(&mut out);
    Ok(out)
}

/// Result of appending another document's pages.
pub struct Appended {
    /// Ids of the inserted pages in order.
    pub pages: Vec<ObjectId>,
    /// Top-level outline items of the source document (already moved into
    /// the target and unlinked from any parent), in order.
    pub outline_items: Vec<ObjectId>,
}

/// Insert all pages of `source` before index `at` of `target`.
///
/// Objects are renumbered above the target's highest id and moved across;
/// the source catalog, page tree and info dictionary are dropped. Form fields
/// are appended to the target's `/AcroForm` (colliding top-level names are
/// suffixed so filled values do not bleed between documents).
pub fn append_document(target: &mut Document, mut source: Document, at: usize) -> Result<Appended> {
    let mut ids = page_ids(target);
    if at > ids.len() {
        return Err(Error::invalid_arg(
            "insert position is outside the document",
        ));
    }
    // Make the source pages self-contained before detaching them.
    let src_pages = page_ids(&source);
    if src_pages.is_empty() {
        return Err(Error::invalid_pdf("the inserted document has no pages"));
    }
    for &p in &src_pages {
        materialize_inherited(&mut source, p)?;
    }
    source.renumber_objects_with(target.max_id + 1);
    let src_pages = page_ids(&source);
    let catalog = source.catalog()?.clone();
    let src_root = catalog.get(b"Pages").and_then(Object::as_reference).ok();
    let catalog_id = source
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .ok();
    let info_id = source
        .trailer
        .get(b"Info")
        .and_then(Object::as_reference)
        .ok();

    // Intermediate page-tree nodes are dropped.
    let mut tree_nodes = HashSet::new();
    if let Some(r) = src_root {
        let mut stack = vec![r];
        while let Some(id) = stack.pop() {
            if !tree_nodes.insert(id) {
                continue;
            }
            if let Ok(d) = source.get_dictionary(id) {
                if d.has_type(b"Pages") || Some(id) == src_root {
                    if let Ok(kids) = d.get(b"Kids").and_then(Object::as_array) {
                        stack.extend(kids.iter().filter_map(|k| k.as_reference().ok()));
                    }
                } else {
                    tree_nodes.remove(&id); // a leaf page
                }
            }
        }
    }

    let target_root = root_pages_id(target)?;
    let src_fields = acroform_fields(&source, &catalog);
    let src_dr_fonts = acroform_dr_fonts(&source, &catalog);
    let outline_items = match catalog.get(b"Outlines").and_then(Object::as_reference) {
        Ok(r) => {
            let mut v = HashSet::new();
            outline_children(&source, r, &mut v)
        }
        Err(_) => Vec::new(),
    };

    target.max_id = target.max_id.max(source.max_id);
    for (id, obj) in std::mem::take(&mut source.objects) {
        if Some(id) == catalog_id || Some(id) == info_id || tree_nodes.contains(&id) {
            continue;
        }
        target.objects.insert(id, obj);
    }
    for &p in &src_pages {
        target
            .get_dictionary_mut(p)?
            .set("Parent", Object::Reference(target_root));
        // The source's structure tree is not carried over.
        target.get_dictionary_mut(p)?.remove(b"StructParents");
    }
    for &item in &outline_items {
        if let Ok(d) = target.get_dictionary_mut(item) {
            d.remove(b"Parent");
            d.remove(b"Prev");
            d.remove(b"Next");
        }
    }
    if !src_fields.is_empty() {
        merge_fields(target, src_fields, src_dr_fonts)?;
    }

    ids.splice(at..at, src_pages.iter().copied());
    rebuild_page_tree(target, &ids)?;
    structure_changed(target);
    Ok(Appended {
        pages: src_pages,
        outline_items,
    })
}

fn acroform_dict<'a>(doc: &'a Document, catalog: &'a Dictionary) -> Option<&'a Dictionary> {
    match catalog.get(b"AcroForm").ok()? {
        Object::Reference(r) => doc.get_dictionary(*r).ok(),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

fn acroform_fields(doc: &Document, catalog: &Dictionary) -> Vec<ObjectId> {
    let Some(acro) = acroform_dict(doc, catalog) else {
        return Vec::new();
    };
    let fields = match acro.get(b"Fields") {
        Ok(Object::Reference(r)) => doc.get_object(*r).ok(),
        Ok(o) => Some(o),
        Err(_) => None,
    };
    fields
        .and_then(|f| f.as_array().ok())
        .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default()
}

fn acroform_dr_fonts(doc: &Document, catalog: &Dictionary) -> Vec<(Vec<u8>, Object)> {
    let Some(acro) = acroform_dict(doc, catalog) else {
        return Vec::new();
    };
    let dr = match acro.get(b"DR") {
        Ok(Object::Reference(r)) => doc.get_dictionary(*r).ok(),
        Ok(o) => o.as_dict().ok(),
        Err(_) => None,
    };
    let font = dr.and_then(|dr| match dr.get(b"Font") {
        Ok(Object::Reference(r)) => doc.get_dictionary(*r).ok(),
        Ok(o) => o.as_dict().ok(),
        Err(_) => None,
    });
    font.map(|f| f.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

fn merge_fields(
    target: &mut Document,
    fields: Vec<ObjectId>,
    dr_fonts: Vec<(Vec<u8>, Object)>,
) -> Result<()> {
    // Ensure the target has an indirect AcroForm dictionary.
    let acro_id = match target.catalog()?.get(b"AcroForm") {
        Ok(Object::Reference(r)) => *r,
        Ok(Object::Dictionary(d)) => {
            let d = d.clone();
            let id = target.add_object(Object::Dictionary(d));
            target.catalog_mut()?.set("AcroForm", Object::Reference(id));
            id
        }
        _ => {
            let mut d = Dictionary::new();
            d.set("Fields", Object::Array(vec![]));
            let id = target.add_object(Object::Dictionary(d));
            target.catalog_mut()?.set("AcroForm", Object::Reference(id));
            id
        }
    };
    let existing: Vec<ObjectId> = {
        let acro = target.get_dictionary(acro_id)?;
        match acro.get(b"Fields") {
            Ok(Object::Reference(r)) => target
                .get_object(*r)?
                .as_array()
                .cloned()
                .unwrap_or_default(),
            Ok(Object::Array(a)) => a.clone(),
            _ => Vec::new(),
        }
        .iter()
        .filter_map(|o| o.as_reference().ok())
        .collect()
    };
    let mut names: HashMap<Vec<u8>, usize> = HashMap::new();
    for &f in &existing {
        if let Ok(t) = target
            .get_dictionary(f)
            .and_then(|d| d.get(b"T"))
            .and_then(Object::as_str)
        {
            names.insert(t.to_vec(), 1);
        }
    }
    for &f in &fields {
        let Ok(d) = target.get_dictionary_mut(f) else {
            continue;
        };
        let Ok(t) = d.get(b"T").and_then(Object::as_str).map(|t| t.to_vec()) else {
            continue;
        };
        if let Some(n) = names.get_mut(&t) {
            *n += 1;
            let mut renamed = t.clone();
            renamed.extend_from_slice(format!("_{n}").as_bytes());
            d.set(
                "T",
                Object::String(renamed.clone(), lopdf::StringFormat::Literal),
            );
            names.insert(renamed, 1);
        } else {
            names.insert(t, 1);
        }
    }
    let mut all: Vec<Object> = existing.into_iter().map(Object::Reference).collect();
    all.extend(fields.into_iter().map(Object::Reference));
    let acro = target.get_dictionary_mut(acro_id)?;
    acro.set("Fields", Object::Array(all));
    if !dr_fonts.is_empty() {
        let mut dr = match acro.get(b"DR") {
            Ok(Object::Dictionary(d)) => d.clone(),
            _ => Dictionary::new(), // indirect DR: keep target's, add new inline
        };
        if acro
            .get(b"DR")
            .map(|o| o.as_reference().is_ok())
            .unwrap_or(false)
        {
            return Ok(()); // do not clobber an indirect DR
        }
        let mut font = match dr.get(b"Font") {
            Ok(Object::Dictionary(d)) => d.clone(),
            _ => Dictionary::new(),
        };
        for (k, v) in dr_fonts {
            if !font.has(&k) {
                font.set(k, v);
            }
        }
        dr.set("Font", Object::Dictionary(font));
        acro.set("DR", Object::Dictionary(dr));
    }
    Ok(())
}

/// Add an outline item `title` -> page `page` as the last child of `parent`
/// (or of the outline root when `parent` is `None`), creating the outline
/// root if needed. Returns the item id.
pub fn add_outline_item(
    doc: &mut Document,
    parent: Option<ObjectId>,
    title: &str,
    page: ObjectId,
) -> Result<ObjectId> {
    let parent = match parent {
        Some(p) => p,
        None => outline_root(doc)?,
    };
    let mut item = Dictionary::new();
    item.set("Title", lopdf::text_string(title));
    item.set(
        "Dest",
        Object::Array(vec![Object::Reference(page), Object::Name(b"Fit".to_vec())]),
    );
    let id = doc.add_object(Object::Dictionary(item));
    append_outline_children(doc, parent, &[id]);
    Ok(id)
}

/// Re-parent `items` (already unlinked) as the last children of `parent`.
pub fn append_outline_children(doc: &mut Document, parent: ObjectId, items: &[ObjectId]) {
    let mut v = HashSet::new();
    let mut kids = outline_children(doc, parent, &mut v);
    kids.extend_from_slice(items);
    relink_outline(doc, parent, &kids);
}

pub fn outline_root(doc: &mut Document) -> Result<ObjectId> {
    if let Ok(r) = doc
        .catalog()?
        .get(b"Outlines")
        .and_then(Object::as_reference)
    {
        if doc.get_dictionary(r).is_ok() {
            return Ok(r);
        }
    }
    let mut d = Dictionary::new();
    d.set("Type", Object::Name(b"Outlines".to_vec()));
    let id = doc.add_object(Object::Dictionary(d));
    doc.catalog_mut()?.set("Outlines", Object::Reference(id));
    Ok(id)
}

/// Detach the whole outline (returns its top-level items, unlinked).
pub fn take_outline(doc: &mut Document) -> Vec<ObjectId> {
    let Ok(root) = doc
        .catalog()
        .and_then(|c| c.get(b"Outlines"))
        .and_then(Object::as_reference)
    else {
        return Vec::new();
    };
    let mut v = HashSet::new();
    let items = outline_children(doc, root, &mut v);
    for &i in &items {
        if let Ok(d) = doc.get_dictionary_mut(i) {
            d.remove(b"Parent");
            d.remove(b"Prev");
            d.remove(b"Next");
        }
    }
    if let Ok(c) = doc.catalog_mut() {
        c.remove(b"Outlines");
    }
    doc.objects.remove(&root);
    items
}

/// Page size in points after applying /Rotate (what the user sees).
pub fn page_size(doc: &Document, page_id: ObjectId) -> (f32, f32) {
    let box_ = inherited(doc, page_id, b"CropBox")
        .or_else(|| inherited(doc, page_id, b"MediaBox"))
        .and_then(|o| match o {
            Object::Reference(r) => doc.get_object(r).ok().cloned(),
            o => Some(o),
        })
        .and_then(|o| o.as_array().ok().cloned())
        .and_then(|a| {
            let n: Vec<f32> = a
                .iter()
                .filter_map(|v| {
                    v.as_float()
                        .ok()
                        .or_else(|| v.as_i64().ok().map(|i| i as f32))
                })
                .collect();
            (n.len() == 4).then(|| ((n[2] - n[0]).abs(), (n[3] - n[1]).abs()))
        })
        .unwrap_or(A4);
    if rotation(doc, page_id) % 180 == 90 {
        (box_.1, box_.0)
    } else {
        box_
    }
}
