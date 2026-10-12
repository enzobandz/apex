//! Deterministic PDF generators used by tests, benchmarks and the CLI's
//! `sample` command. They intentionally exercise awkward structures: nested
//! page trees with inherited attributes, outlines, links, form fields.

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

pub struct FixtureOptions {
    pub pages: usize,
    /// Build a two-level page tree with MediaBox/Resources inherited from the
    /// intermediate nodes instead of set on each page.
    pub nested_tree: bool,
    /// Add an outline with one item per page.
    pub outline: bool,
    /// Add a text form field on the first page.
    pub form_field: bool,
    /// Add a link on page 1 pointing to the last page.
    pub link_to_last: bool,
    pub title: Option<String>,
}

impl Default for FixtureOptions {
    fn default() -> Self {
        Self {
            pages: 3,
            nested_tree: false,
            outline: false,
            form_field: false,
            link_to_last: false,
            title: Some("Fixture".into()),
        }
    }
}

/// Generate a document whose page *i* (1-based) shows the text `Page i`.
pub fn generate(opts: &FixtureOptions) -> Document {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources = dictionary! { "Font" => dictionary! { "F1" => font_id } };
    let media_box: Vec<Object> = vec![0.into(), 0.into(), 612.into(), 792.into()];

    let mut page_ids = Vec::with_capacity(opts.pages);
    let mut groups: Vec<(ObjectId, Vec<ObjectId>)> = Vec::new();
    for i in 0..opts.pages {
        let text = format!("BT /F1 36 Tf 72 700 Td (Page {}) Tj ET", i + 1);
        let content_id = doc.add_object(Stream::new(Dictionary::new(), text.into_bytes()));
        let parent = if opts.nested_tree {
            if i % 4 == 0 {
                groups.push((doc.new_object_id(), Vec::new()));
            }
            groups.last().unwrap().0
        } else {
            pages_id
        };
        let mut page = dictionary! {
            "Type" => "Page",
            "Parent" => parent,
            "Contents" => content_id,
        };
        if !opts.nested_tree {
            page.set("MediaBox", media_box.clone());
            page.set("Resources", resources.clone());
        }
        let id = doc.add_object(page);
        if opts.nested_tree {
            groups.last_mut().unwrap().1.push(id);
        }
        page_ids.push(id);
    }

    let kids: Vec<Object> = if opts.nested_tree {
        for (gid, members) in &groups {
            doc.objects.insert(
                *gid,
                Object::Dictionary(dictionary! {
                    "Type" => "Pages",
                    "Parent" => pages_id,
                    "Kids" => members.iter().map(|&m| Object::Reference(m)).collect::<Vec<_>>(),
                    "Count" => members.len() as i64,
                    "MediaBox" => media_box.clone(),
                    "Resources" => resources.clone(),
                }),
            );
        }
        groups.iter().map(|(g, _)| Object::Reference(*g)).collect()
    } else {
        page_ids.iter().map(|&p| Object::Reference(p)).collect()
    };
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => opts.pages as i64,
        }),
    );
    let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages_id };

    if opts.outline && !page_ids.is_empty() {
        let root = doc.new_object_id();
        let items: Vec<ObjectId> = (0..page_ids.len()).map(|_| doc.new_object_id()).collect();
        for (i, &item) in items.iter().enumerate() {
            let mut d = dictionary! {
                "Title" => Object::string_literal(format!("Chapter {}", i + 1)),
                "Parent" => root,
                "Dest" => vec![Object::Reference(page_ids[i]), "Fit".into()],
            };
            if i > 0 {
                d.set("Prev", items[i - 1]);
            }
            if i + 1 < items.len() {
                d.set("Next", items[i + 1]);
            }
            doc.objects.insert(item, Object::Dictionary(d));
        }
        doc.objects.insert(
            root,
            Object::Dictionary(dictionary! {
                "Type" => "Outlines",
                "First" => items[0],
                "Last" => *items.last().unwrap(),
                "Count" => items.len() as i64,
            }),
        );
        catalog.set("Outlines", root);
    }

    if opts.link_to_last && page_ids.len() > 1 {
        let link = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Link",
            "Rect" => vec![72.into(), 690.into(), 300.into(), 740.into()],
            "Border" => vec![0.into(), 0.into(), 0.into()],
            "Dest" => vec![Object::Reference(*page_ids.last().unwrap()), "Fit".into()],
        });
        doc.get_dictionary_mut(page_ids[0])
            .unwrap()
            .set("Annots", vec![Object::Reference(link)]);
    }

    if opts.form_field && !page_ids.is_empty() {
        let field = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Widget",
            "FT" => "Tx",
            "T" => Object::string_literal("name"),
            "V" => Object::string_literal(""),
            "Rect" => vec![72.into(), 600.into(), 300.into(), 620.into()],
            "P" => page_ids[0],
            "DA" => Object::string_literal("/Helv 12 Tf 0 g"),
        });
        let mut annots = doc
            .get_dictionary(page_ids[0])
            .unwrap()
            .get(b"Annots")
            .and_then(Object::as_array)
            .cloned()
            .unwrap_or_default();
        annots.push(Object::Reference(field));
        doc.get_dictionary_mut(page_ids[0])
            .unwrap()
            .set("Annots", annots);
        let helv = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        catalog.set(
            "AcroForm",
            dictionary! {
                "Fields" => vec![Object::Reference(field)],
                "DA" => Object::string_literal("/Helv 12 Tf 0 g"),
                "DR" => dictionary! { "Font" => dictionary! { "Helv" => helv } },
            },
        );
    }

    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", catalog_id);
    if let Some(title) = &opts.title {
        let info = doc.add_object(dictionary! {
            "Title" => Object::string_literal(title.as_str()),
            "Producer" => Object::string_literal("opdf fixtures"),
        });
        doc.trailer.set("Info", info);
    }
    doc
}

/// Serialised [`generate`] output.
pub fn generate_bytes(opts: &FixtureOptions) -> Vec<u8> {
    let mut doc = generate(opts);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("in-memory save cannot fail");
    out
}

/// A plain `n`-page document.
pub fn simple(n: usize) -> Vec<u8> {
    generate_bytes(&FixtureOptions {
        pages: n,
        ..Default::default()
    })
}

/// Extract the "Page N" label drawn on each page, in page order — lets tests
/// assert page order after structural operations without a renderer.
pub fn page_labels(doc: &Document) -> Vec<String> {
    doc.page_iter()
        .map(|id| {
            let content = doc.get_page_content(id);
            let s = String::from_utf8_lossy(&content);
            s.split('(')
                .nth(1)
                .and_then(|r| r.split(')').next())
                .unwrap_or("")
                .to_string()
        })
        .collect()
}
