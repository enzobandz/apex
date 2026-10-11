//! Images → PDF.
//!
//! JPEG files (grey or RGB) are embedded byte-for-byte with `/DCTDecode`, so
//! there is no generational quality loss. Everything else is decoded and
//! stored losslessly with `/FlateDecode`; alpha becomes an `/SMask`. EXIF
//! orientation is honoured with a transformation matrix, not by re-encoding.

use crate::error::{Error, ErrorCode, Result};
use crate::pages::{A4, LETTER};
use flate2::{write::ZlibEncoder, Compression};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use serde::Deserialize;
use std::io::{Cursor, Write};

/// Reject images that would need more than this many pixels.
pub const MAX_PIXELS: u64 = 200_000_000;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PageSizeChoice {
    /// The page takes the image's size (at 96 dpi or the image's own dpi).
    #[default]
    FitImage,
    A4,
    Letter,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Orientation {
    /// Portrait or landscape, whichever matches the image.
    #[default]
    Auto,
    Portrait,
    Landscape,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePageOptions {
    pub page_size: PageSizeChoice,
    pub orientation: Orientation,
    /// Margin in points (ignored for FitImage).
    pub margin: f32,
}

impl Default for ImagePageOptions {
    fn default() -> Self {
        Self {
            page_size: PageSizeChoice::FitImage,
            orientation: Orientation::Auto,
            margin: 0.0,
        }
    }
}

struct Embedded {
    xobject: Stream,
    smask: Option<Stream>,
    /// Pixel size *after* applying EXIF orientation.
    width: u32,
    height: u32,
    orientation: u8,
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
    e.write_all(data).expect("writing to Vec cannot fail");
    e.finish().expect("writing to Vec cannot fail")
}

fn exif_orientation(bytes: &[u8]) -> u8 {
    let Ok(reader) = ImageReader::new(Cursor::new(bytes)).with_guessed_format() else {
        return 1;
    };
    let Ok(mut decoder) = reader.into_decoder() else {
        return 1;
    };
    match decoder.orientation() {
        Ok(o) => match o {
            image::metadata::Orientation::NoTransforms => 1,
            image::metadata::Orientation::FlipHorizontal => 2,
            image::metadata::Orientation::Rotate180 => 3,
            image::metadata::Orientation::FlipVertical => 4,
            image::metadata::Orientation::Rotate90FlipH => 5,
            image::metadata::Orientation::Rotate90 => 6,
            image::metadata::Orientation::Rotate270FlipH => 7,
            image::metadata::Orientation::Rotate270 => 8,
        },
        Err(_) => 1,
    }
}

/// Parse SOF of a JPEG to get (width, height, components). Returns `None`
/// for anything unusual (arithmetic coding, lossless, 12-bit, CMYK...).
fn jpeg_info(bytes: &[u8]) -> Option<(u32, u32, u8)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            return None;
        }
        let marker = bytes[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        // Baseline (C0), extended (C1), progressive (C2) Huffman only.
        if matches!(marker, 0xC0..=0xC2) {
            let seg = bytes.get(i + 4..i + 2 + len)?;
            let precision = *seg.first()?;
            let h = u16::from_be_bytes([*seg.get(1)?, *seg.get(2)?]) as u32;
            let w = u16::from_be_bytes([*seg.get(3)?, *seg.get(4)?]) as u32;
            let comps = *seg.get(5)?;
            return (precision == 8 && w > 0 && h > 0 && (comps == 1 || comps == 3))
                .then_some((w, h, comps));
        }
        if matches!(marker, 0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF | 0xDA) {
            return None;
        }
        i += 2 + len;
    }
    None
}

fn embed(bytes: &[u8]) -> Result<Embedded> {
    let orientation = exif_orientation(bytes);
    let swap = matches!(orientation, 5..=8);
    if let Some((w, h, comps)) = jpeg_info(bytes) {
        if (w as u64) * (h as u64) > MAX_PIXELS {
            return Err(Error::new(
                ErrorCode::LimitExceeded,
                "the image is too large",
            ));
        }
        let xobject = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => w as i64,
                "Height" => h as i64,
                "ColorSpace" => if comps == 1 { "DeviceGray" } else { "DeviceRGB" },
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            bytes.to_vec(),
        )
        .with_compression(false);
        let (width, height) = if swap { (h, w) } else { (w, h) };
        return Ok(Embedded {
            xobject,
            smask: None,
            width,
            height,
            orientation,
        });
    }

    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| Error::invalid_arg(format!("unreadable image: {e}")))?;
    if reader.format().is_none() {
        return Err(Error::unsupported("this image format is not supported"));
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(2 * 1024 * 1024 * 1024);
    reader.limits(limits);
    let (w, h) = reader
        .into_dimensions()
        .map_err(|e| Error::invalid_arg(format!("unreadable image: {e}")))?;
    if (w as u64) * (h as u64) > MAX_PIXELS {
        return Err(Error::new(
            ErrorCode::LimitExceeded,
            "the image is too large",
        ));
    }
    let img = image::load_from_memory(bytes)?;
    let (data, color_space, alpha) = split_alpha(img);
    let xobject = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => w as i64,
            "Height" => h as i64,
            "ColorSpace" => color_space,
            "BitsPerComponent" => 8,
            "Filter" => "FlateDecode",
        },
        deflate(&data),
    )
    .with_compression(false);
    let smask = alpha.map(|a| {
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => w as i64,
                "Height" => h as i64,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
                "Filter" => "FlateDecode",
            },
            deflate(&a),
        )
        .with_compression(false)
    });
    let (width, height) = if swap { (h, w) } else { (w, h) };
    Ok(Embedded {
        xobject,
        smask,
        width,
        height,
        orientation,
    })
}

/// Returns (samples, colour space, optional alpha channel). Fully opaque
/// alpha channels are dropped.
fn split_alpha(img: DynamicImage) -> (Vec<u8>, &'static str, Option<Vec<u8>>) {
    let gray = matches!(
        img,
        DynamicImage::ImageLuma8(_)
            | DynamicImage::ImageLuma16(_)
            | DynamicImage::ImageLumaA8(_)
            | DynamicImage::ImageLumaA16(_)
    );
    let has_alpha = img.color().has_alpha();
    if gray {
        let la = img.to_luma_alpha8();
        let mut g = Vec::with_capacity(la.len() / 2);
        let mut a = Vec::with_capacity(la.len() / 2);
        for p in la.pixels() {
            g.push(p.0[0]);
            a.push(p.0[1]);
        }
        let alpha = (has_alpha && a.iter().any(|&v| v != 255)).then_some(a);
        (g, "DeviceGray", alpha)
    } else {
        let rgba = img.to_rgba8();
        let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
        let mut a = Vec::with_capacity(rgba.len() / 4);
        for p in rgba.pixels() {
            rgb.extend_from_slice(&p.0[..3]);
            a.push(p.0[3]);
        }
        let alpha = (has_alpha && a.iter().any(|&v| v != 255)).then_some(a);
        (rgb, "DeviceRGB", alpha)
    }
}

/// The `cm` matrix that draws a unit-square image into a `w`×`h` box at
/// (`x`,`y`) with EXIF `orientation` applied.
fn placement_matrix(orientation: u8, x: f32, y: f32, w: f32, h: f32) -> [f32; 6] {
    match orientation {
        2 => [-w, 0.0, 0.0, h, x + w, y],
        3 => [-w, 0.0, 0.0, -h, x + w, y + h],
        4 => [w, 0.0, 0.0, -h, x, y + h],
        5 => [0.0, -h, -w, 0.0, x + w, y + h],
        6 => [0.0, -h, w, 0.0, x, y + h],
        7 => [0.0, h, w, 0.0, x, y],
        8 => [0.0, h, -w, 0.0, x + w, y],
        _ => [w, 0.0, 0.0, h, x, y],
    }
}

fn add_image_page(
    doc: &mut Document,
    pages_id: ObjectId,
    bytes: &[u8],
    opts: &ImagePageOptions,
) -> Result<ObjectId> {
    let e = embed(bytes)?;
    let (iw, ih) = (e.width as f32 * 0.75, e.height as f32 * 0.75); // 96 dpi
    let (pw, ph) = match opts.page_size {
        PageSizeChoice::FitImage => {
            // Keep pages within the 200-inch PDF limit.
            let scale = (14_400.0 / iw.max(ih)).min(1.0);
            (iw * scale, ih * scale)
        }
        PageSizeChoice::A4 | PageSizeChoice::Letter => {
            let (a, b) = if opts.page_size == PageSizeChoice::A4 {
                A4
            } else {
                LETTER
            };
            let landscape = match opts.orientation {
                Orientation::Auto => iw > ih,
                Orientation::Portrait => false,
                Orientation::Landscape => true,
            };
            if landscape {
                (b, a)
            } else {
                (a, b)
            }
        }
    };
    let margin = if opts.page_size == PageSizeChoice::FitImage {
        0.0
    } else {
        opts.margin.clamp(0.0, pw.min(ph) / 2.0 - 1.0)
    };
    let (bw, bh) = (pw - 2.0 * margin, ph - 2.0 * margin);
    let scale = (bw / iw).min(bh / ih);
    let (dw, dh) = (iw * scale, ih * scale);
    let (x, y) = (margin + (bw - dw) / 2.0, margin + (bh - dh) / 2.0);
    let m = placement_matrix(e.orientation, x, y, dw, dh);

    let mut xobject = e.xobject;
    if let Some(smask) = e.smask {
        let sid = doc.add_object(smask);
        xobject.dict.set("SMask", Object::Reference(sid));
    }
    let img_id = doc.add_object(xobject);
    let content = format!(
        "q {} {} {} {} {} {} cm /Im0 Do Q",
        m[0], m[1], m[2], m[3], m[4], m[5]
    );
    let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    let page = dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), Object::Real(pw), Object::Real(ph)],
        "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => img_id } },
        "Contents" => content_id,
    };
    Ok(doc.add_object(page))
}

/// Build a new PDF with one page per image (in order).
pub fn images_to_pdf(images: &[Vec<u8>], opts: &ImagePageOptions) -> Result<Document> {
    if images.is_empty() {
        return Err(Error::invalid_arg("no images selected"));
    }
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut kids = Vec::with_capacity(images.len());
    for (i, bytes) in images.iter().enumerate() {
        let id = add_image_page(&mut doc, pages_id, bytes, opts).map_err(|mut e| {
            e.message = format!("image {}: {}", i + 1, e.message);
            e
        })?;
        kids.push(Object::Reference(id));
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids.clone(),
            "Count" => kids.len() as i64,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let info = doc.add_object(dictionary! {
        "Producer" => Object::string_literal("OpenPDF Studio"),
    });
    doc.trailer.set("Info", info);
    crate::metadata::touch(&mut doc);
    Ok(doc)
}

/// Supported input extensions (lower case) for file pickers.
pub const IMAGE_EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "webp"];

pub fn is_supported_image(bytes: &[u8]) -> bool {
    matches!(
        image::guess_format(bytes),
        Ok(ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::Gif
            | ImageFormat::Bmp
            | ImageFormat::Tiff
            | ImageFormat::WebP)
    )
}

#[cfg(test)]
pub(crate) fn test_png(w: u32, h: u32, alpha: bool) -> Vec<u8> {
    let mut out = Vec::new();
    if alpha {
        let img = image::RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([x as u8, y as u8, 128, (x % 256) as u8])
        });
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
    } else {
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([x as u8, y as u8, 200]));
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
    }
    out
}

#[cfg(test)]
pub(crate) fn test_jpeg(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([x as u8, y as u8, 50]));
    DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg)
        .unwrap();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_is_passed_through_unchanged() {
        let jpg = test_jpeg(40, 20);
        let doc = images_to_pdf(std::slice::from_ref(&jpg), &ImagePageOptions::default()).unwrap();
        let img = doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .find(|s| s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image"))
            .unwrap();
        assert_eq!(img.content, jpg);
        assert_eq!(
            img.dict.get(b"Filter").unwrap().as_name().unwrap(),
            b"DCTDecode"
        );
    }

    #[test]
    fn png_alpha_becomes_smask() {
        let png = test_png(16, 8, true);
        let doc = images_to_pdf(&[png], &ImagePageOptions::default()).unwrap();
        let img = doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .find(|s| s.dict.has(b"SMask"))
            .expect("image with SMask");
        assert_eq!(img.dict.get(b"Width").unwrap().as_i64().unwrap(), 16);
        let data = img.decompressed_content().unwrap();
        assert_eq!(data.len(), 16 * 8 * 3);
    }

    #[test]
    fn page_sizes() {
        let png = test_png(400, 200, false);
        let fit = images_to_pdf(std::slice::from_ref(&png), &ImagePageOptions::default()).unwrap();
        let p = fit.page_iter().next().unwrap();
        assert_eq!(crate::pages::page_size(&fit, p), (300.0, 150.0));
        let a4 = images_to_pdf(
            &[png],
            &ImagePageOptions {
                page_size: PageSizeChoice::A4,
                orientation: Orientation::Auto,
                margin: 36.0,
            },
        )
        .unwrap();
        let p = a4.page_iter().next().unwrap();
        let (w, h) = crate::pages::page_size(&a4, p);
        assert!(w > h, "wide image gets a landscape page");
    }

    #[test]
    fn rejects_non_images() {
        assert!(images_to_pdf(&[b"not an image".to_vec()], &ImagePageOptions::default()).is_err());
        assert!(images_to_pdf(&[], &ImagePageOptions::default()).is_err());
    }

    #[test]
    fn orientation_matrices_cover_box() {
        // Every orientation must map the unit square onto the target box.
        for o in 1..=8u8 {
            let m = placement_matrix(o, 10.0, 20.0, 100.0, 50.0);
            let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)];
            let mut xs: Vec<f32> = corners
                .iter()
                .map(|(u, v)| m[0] * u + m[2] * v + m[4])
                .collect();
            let mut ys: Vec<f32> = corners
                .iter()
                .map(|(u, v)| m[1] * u + m[3] * v + m[5])
                .collect();
            xs.sort_by(f32::total_cmp);
            ys.sort_by(f32::total_cmp);
            assert_eq!(
                (xs[0], xs[3], ys[0], ys[3]),
                (10.0, 110.0, 20.0, 70.0),
                "orientation {o}"
            );
        }
    }
}
