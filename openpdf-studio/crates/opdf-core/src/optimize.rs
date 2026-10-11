//! File-size optimisation.
//!
//! Lossless steps (always): drop unreachable objects, merge byte-identical
//! streams (common after merging files that embed the same fonts), Flate-
//! compress uncompressed streams, and write object/xref streams.
//!
//! Lossy step (opt-in): downsample images whose resolution exceeds a target
//! dpi *for the largest page they appear on* and re-encode them as JPEG. The
//! page-size bound is conservative — an image drawn smaller than the full
//! page keeps more than the target resolution, never less. Only images whose
//! encoding we fully understand are touched (8-bit grey/RGB, Flate or DCT,
//! no colour-key mask, no Decode array); everything else is left as is.

use crate::error::Result;
use crate::pages;
use image::{imageops::FilterType, DynamicImage, GrayImage, ImageFormat, RgbImage};
use lopdf::{Document, Object, ObjectId, Stream};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageOptions {
    /// Images above this resolution are downsampled (e.g. 150).
    pub max_dpi: u32,
    /// JPEG quality 1-100 (e.g. 75).
    pub jpeg_quality: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeOptions {
    pub images: Option<ImageOptions>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeReport {
    pub unused_objects_removed: usize,
    pub duplicate_streams_merged: usize,
    pub streams_compressed: usize,
    pub images_downsampled: usize,
    pub images_skipped: usize,
}

const MAX_IMAGE_BYTES: usize = 512 * 1024 * 1024;

pub fn optimize(doc: &mut Document, opts: &OptimizeOptions) -> Result<OptimizeReport> {
    let mut report = OptimizeReport {
        unused_objects_removed: doc.prune_objects().len(),
        ..Default::default()
    };
    if let Some(img) = opts.images {
        let (done, skipped) = downsample_images(doc, img);
        report.images_downsampled = done;
        report.images_skipped = skipped;
    }
    report.duplicate_streams_merged = dedupe_streams(doc);
    let before: usize = doc
        .objects
        .values()
        .filter(|o| matches!(o, Object::Stream(s) if s.dict.get(b"Filter").is_err()))
        .count();
    doc.compress();
    let after: usize = doc
        .objects
        .values()
        .filter(|o| matches!(o, Object::Stream(s) if s.dict.get(b"Filter").is_err()))
        .count();
    report.streams_compressed = before - after;
    report.unused_objects_removed += doc.prune_objects().len();
    Ok(report)
}

fn dedupe_streams(doc: &mut Document) -> usize {
    let mut canonical: HashMap<[u8; 32], ObjectId> = HashMap::new();
    let mut replace: HashMap<ObjectId, ObjectId> = HashMap::new();
    let mut ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    ids.sort();
    for id in ids {
        let Some(Object::Stream(s)) = doc.objects.get(&id) else {
            continue;
        };
        // Never merge metadata or object/xref streams.
        if s.dict.has_type(b"Metadata") || s.dict.has_type(b"ObjStm") || s.dict.has_type(b"XRef") {
            continue;
        }
        let mut h = Sha256::new();
        h.update(format!("{:?}", s.dict).as_bytes());
        h.update([0u8]);
        h.update(&s.content);
        let key: [u8; 32] = h.finalize().into();
        match canonical.get(&key) {
            Some(&c) => {
                replace.insert(id, c);
            }
            None => {
                canonical.insert(key, id);
            }
        }
    }
    if replace.is_empty() {
        return 0;
    }
    fn walk(obj: &mut Object, map: &HashMap<ObjectId, ObjectId>) {
        match obj {
            Object::Reference(id) => {
                if let Some(n) = map.get(id) {
                    *id = *n;
                }
            }
            Object::Array(a) => a.iter_mut().for_each(|o| walk(o, map)),
            Object::Dictionary(d) => d.iter_mut().for_each(|(_, o)| walk(o, map)),
            Object::Stream(s) => s.dict.iter_mut().for_each(|(_, o)| walk(o, map)),
            _ => {}
        }
    }
    for obj in doc.objects.values_mut() {
        walk(obj, &replace);
    }
    doc.trailer.iter_mut().for_each(|(_, o)| walk(o, &replace));
    for id in replace.keys() {
        doc.objects.remove(id);
    }
    replace.len()
}

/// For every image XObject reachable from a page (directly or through one
/// level of form XObjects), the largest dimension (points) of any page that
/// uses it.
fn image_page_extent(doc: &Document) -> HashMap<ObjectId, f32> {
    let mut out: HashMap<ObjectId, f32> = HashMap::new();
    for page in pages::page_ids(doc) {
        let (w, h) = pages::page_size(doc, page);
        let extent = w.max(h);
        let Some(res) = pages::inherited(doc, page, b"Resources") else {
            continue;
        };
        let mut stack: Vec<(Object, usize)> = vec![(res, 0)];
        let mut seen = HashSet::new();
        while let Some((res, depth)) = stack.pop() {
            let res = match res {
                Object::Reference(r) => match doc.get_object(r) {
                    Ok(o) => o.clone(),
                    Err(_) => continue,
                },
                o => o,
            };
            let Ok(xobjs) = res
                .as_dict()
                .and_then(|d| d.get_deref(b"XObject", doc))
                .and_then(Object::as_dict)
            else {
                continue;
            };
            for (_, v) in xobjs.iter() {
                let Ok(id) = v.as_reference() else { continue };
                if !seen.insert(id) {
                    continue;
                }
                let Ok(s) = doc.get_object(id).and_then(Object::as_stream) else {
                    continue;
                };
                match s.dict.get(b"Subtype").and_then(Object::as_name) {
                    Ok(b"Image") => {
                        let e = out.entry(id).or_insert(0.0);
                        *e = e.max(extent);
                    }
                    Ok(b"Form") if depth < 2 => {
                        if let Ok(r) = s.dict.get(b"Resources") {
                            stack.push((r.clone(), depth + 1));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

enum Candidate {
    Dct(Vec<u8>),
    Raw(Vec<u8>),
}

fn candidate(doc: &Document, s: &Stream) -> Option<(Candidate, u32, u32, u8)> {
    let d = &s.dict;
    if d.get(b"ImageMask")
        .and_then(Object::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    if d.get(b"BitsPerComponent").and_then(Object::as_i64).ok()? != 8 {
        return None;
    }
    if d.has(b"Decode") || matches!(d.get(b"Mask"), Ok(Object::Array(_))) {
        return None;
    }
    let w = d.get(b"Width").and_then(Object::as_i64).ok()?;
    let h = d.get(b"Height").and_then(Object::as_i64).ok()?;
    if w <= 0 || h <= 0 || w > 65535 || h > 65535 {
        return None;
    }
    let cs = d.get_deref(b"ColorSpace", doc).ok()?;
    let comps: u8 = match cs {
        Object::Name(n) if n == b"DeviceRGB" => 3,
        Object::Name(n) if n == b"DeviceGray" => 1,
        Object::Array(a) if a.first().and_then(|o| o.as_name().ok()) == Some(b"ICCBased") => {
            let icc = a.get(1)?.as_reference().ok()?;
            let n = doc
                .get_object(icc)
                .ok()?
                .as_stream()
                .ok()?
                .dict
                .get(b"N")
                .ok()?
                .as_i64()
                .ok()?;
            match n {
                1 => 1,
                3 => 3,
                _ => return None,
            }
        }
        _ => return None,
    };
    let filters = s.filters().ok()?;
    let data = match filters.as_slice() {
        [b"DCTDecode"] => Candidate::Dct(s.content.clone()),
        [b"FlateDecode"] => {
            let raw = s.decompressed_content_with_limit(MAX_IMAGE_BYTES).ok()?;
            if raw.len() < (w * h) as usize * comps as usize {
                return None;
            }
            Candidate::Raw(raw)
        }
        _ => return None,
    };
    Some((data, w as u32, h as u32, comps))
}

fn reencode(
    c: Candidate,
    w: u32,
    h: u32,
    comps: u8,
    max_px: u32,
    quality: u8,
) -> Option<(Vec<u8>, u32, u32)> {
    let img: DynamicImage = match c {
        Candidate::Dct(bytes) => {
            let img = image::load_from_memory_with_format(&bytes, ImageFormat::Jpeg).ok()?;
            if img.width() != w || img.height() != h {
                return None;
            }
            if comps == 1 {
                DynamicImage::ImageLuma8(img.to_luma8())
            } else {
                DynamicImage::ImageRgb8(img.to_rgb8())
            }
        }
        Candidate::Raw(raw) => {
            let n = (w * h) as usize * comps as usize;
            let raw = raw[..n].to_vec();
            if comps == 1 {
                DynamicImage::ImageLuma8(GrayImage::from_raw(w, h, raw)?)
            } else {
                DynamicImage::ImageRgb8(RgbImage::from_raw(w, h, raw)?)
            }
        }
    };
    let scale = max_px as f32 / w.max(h) as f32;
    let (nw, nh) = (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    let small = img.resize_exact(nw, nh, FilterType::CatmullRom);
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(
        Cursor::new(&mut out),
        quality.clamp(1, 100),
    );
    enc.encode_image(&small).ok()?;
    Some((out, nw, nh))
}

fn downsample_images(doc: &mut Document, opts: ImageOptions) -> (usize, usize) {
    let dpi = opts.max_dpi.clamp(36, 1200) as f32;
    let extents = image_page_extent(doc);
    let jobs: Vec<(ObjectId, Candidate, u32, u32, u8, u32, usize)> = extents
        .iter()
        .filter_map(|(&id, &extent)| {
            let s = doc.get_object(id).ok()?.as_stream().ok()?;
            let max_px = ((extent / 72.0) * dpi).ceil() as u32;
            let (c, w, h, comps) = candidate(doc, s)?;
            // Only worth it when clearly above target resolution.
            (w.max(h) as f32 > max_px as f32 * 1.15).then_some((
                id,
                c,
                w,
                h,
                comps,
                max_px,
                s.content.len(),
            ))
        })
        .collect();
    let results: Vec<(ObjectId, Vec<u8>, u32, u32, u8)> = jobs
        .into_par_iter()
        .filter_map(|(id, c, w, h, comps, max_px, old_len)| {
            let (bytes, nw, nh) = reencode(c, w, h, comps, max_px, opts.jpeg_quality)?;
            (bytes.len() < old_len).then_some((id, bytes, nw, nh, comps))
        })
        .collect();
    let done = results.len();
    for (id, bytes, nw, nh, _comps) in results {
        if let Some(Object::Stream(s)) = doc.objects.get_mut(&id) {
            s.dict.set("Width", nw as i64);
            s.dict.set("Height", nh as i64);
            s.dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
            s.dict.remove(b"DecodeParms");
            s.set_content(bytes);
            s.allows_compression = false;
        }
    }
    (done, extents.len() - done)
}
