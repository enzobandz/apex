//! `opdf` — the OpenPDF Studio engine on the command line.
//!
//! Every command reads its inputs, writes new files atomically and never
//! modifies inputs in place unless the output path says so.

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use opdf_core::assemble::{self, MergeInput, MergeOptions, SplitPlan};
use opdf_core::images::{self, ImagePageOptions, Orientation, PageSizeChoice};
use opdf_core::metadata::MetadataUpdate;
use opdf_core::optimize::{ImageOptions, OptimizeOptions};
use opdf_core::security::{AllowedActions, ProtectOptions};
use opdf_core::session::{DocSession, Op, OpenRequest};
use opdf_core::{fsio, load, ranges, ErrorCode};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "opdf", version, about = "OpenPDF Studio command line", long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args, Clone)]
struct PasswordArg {
    /// Password for encrypted inputs (or set OPDF_PASSWORD).
    #[arg(long, env = "OPDF_PASSWORD", hide_env_values = true)]
    password: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show page count, metadata, security and features (JSON with --json).
    Info {
        file: PathBuf,
        #[command(flatten)]
        pw: PasswordArg,
        #[arg(long)]
        json: bool,
    },
    /// Merge files in the given order.
    Merge {
        #[arg(required = true, num_args = 2..)]
        files: Vec<PathBuf>,
        #[arg(short, long)]
        output: PathBuf,
        /// Add one bookmark per input file.
        #[arg(long)]
        bookmarks: bool,
        #[command(flatten)]
        pw: PasswordArg,
    },
    /// Split into several files.
    Split {
        file: PathBuf,
        /// Pages per output file.
        #[arg(long, conflicts_with_all = ["ranges", "single"])]
        every: Option<usize>,
        /// One output per range, e.g. --ranges 1-3 4- odd
        #[arg(long, num_args = 1..)]
        ranges: Vec<String>,
        /// One file per page.
        #[arg(long)]
        single: bool,
        #[arg(long)]
        out_dir: PathBuf,
        #[command(flatten)]
        pw: PasswordArg,
    },
    /// Write the selected pages (in the given order) to a new file.
    Extract {
        file: PathBuf,
        #[arg(long)]
        pages: String,
        #[arg(short, long)]
        output: PathBuf,
        #[command(flatten)]
        pw: PasswordArg,
    },
    /// Rotate pages by a multiple of 90 degrees.
    Rotate {
        file: PathBuf,
        #[arg(long, default_value = "all")]
        pages: String,
        #[arg(long, allow_hyphen_values = true)]
        degrees: i64,
        #[arg(short, long)]
        output: PathBuf,
        #[command(flatten)]
        pw: PasswordArg,
    },
    /// Delete pages.
    Delete {
        file: PathBuf,
        #[arg(long)]
        pages: String,
        #[arg(short, long)]
        output: PathBuf,
        #[command(flatten)]
        pw: PasswordArg,
    },
    /// Convert images (PNG, JPEG, GIF, BMP, TIFF, WebP) to one PDF.
    Images2pdf {
        #[arg(required = true)]
        images: Vec<PathBuf>,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value_t = PageSize::Fit)]
        page_size: PageSize,
        /// Margin in points (A4/Letter only).
        #[arg(long, default_value_t = 0.0)]
        margin: f32,
    },
    /// Add password protection (AES-256). Several inputs are processed in parallel.
    Protect {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[command(flatten)]
        out: Outputs,
        #[arg(long, default_value = "")]
        user_password: String,
        #[arg(long, default_value = "")]
        owner_password: String,
        #[arg(long)]
        no_print: bool,
        #[arg(long)]
        no_copy: bool,
        #[arg(long)]
        no_modify: bool,
        #[arg(long)]
        no_annotate: bool,
        #[arg(long)]
        no_assemble: bool,
        #[arg(long)]
        no_fill_forms: bool,
    },
    /// Remove password protection (requires the owner password).
    Unprotect {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[command(flatten)]
        out: Outputs,
        #[command(flatten)]
        pw: PasswordArg,
    },
    /// Reduce file size. Several inputs are processed in parallel.
    Optimize {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[command(flatten)]
        out: Outputs,
        /// Downsample images above this resolution (lossy). Omit for lossless only.
        #[arg(long)]
        max_dpi: Option<u32>,
        #[arg(long, default_value_t = 75)]
        quality: u8,
        #[command(flatten)]
        pw: PasswordArg,
    },
    /// Edit document properties.
    Meta {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        author: Option<String>,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long)]
        keywords: Option<String>,
        #[command(flatten)]
        pw: PasswordArg,
    },
    /// Generate a sample document (useful for testing).
    Sample {
        #[arg(long, default_value_t = 5)]
        pages: usize,
        #[arg(short, long)]
        output: PathBuf,
    },
}

#[derive(Args)]
struct Outputs {
    /// Output file (single input only).
    #[arg(short, long, conflicts_with = "out_dir")]
    output: Option<PathBuf>,
    /// Output directory (keeps file names).
    #[arg(long)]
    out_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum)]
enum PageSize {
    Fit,
    A4,
    Letter,
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fsio::read_limited(path, load::MAX_FILE_SIZE)
        .with_context(|| format!("reading {}", path.display()))
}

fn session(path: &Path, pw: &PasswordArg) -> Result<DocSession> {
    let bytes = read(path)?;
    DocSession::open(
        0,
        OpenRequest {
            bytes,
            password: pw.password.clone(),
            name: file_name(path),
            path: None,
            store: None,
            label: "Opened".into(),
        },
    )
    .map_err(|e| explain(e, path))
}

fn explain(e: opdf_core::Error, path: &Path) -> anyhow::Error {
    match e.code {
        ErrorCode::PasswordRequired => {
            anyhow::anyhow!("{} is password protected; pass --password", path.display())
        }
        _ => anyhow::anyhow!("{}: {}", path.display(), e.message),
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document.pdf".into())
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".into())
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    fsio::atomic_write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
    println!("wrote {} ({} bytes)", path.display(), bytes.len());
    Ok(())
}

fn page_list(spec: &str, s: &mut DocSession) -> Result<Vec<usize>> {
    let n = s.state().summary.map(|x| x.page_count).unwrap_or(0);
    Ok(ranges::parse(spec, n)?)
}

fn single_op(
    file: &Path,
    pw: &PasswordArg,
    output: &Path,
    op: impl FnOnce(&mut DocSession) -> Result<Op>,
) -> Result<()> {
    let mut s = session(file, pw)?;
    let op = op(&mut s)?;
    s.apply(&op).map_err(|e| explain(e, file))?;
    write(output, &s.bytes())
}

/// Apply `f` to every input in parallel, writing to `out`.
fn batch(
    files: &[PathBuf],
    out: &Outputs,
    f: impl Fn(&Path) -> Result<Vec<u8>> + Sync,
) -> Result<()> {
    let targets: Vec<(PathBuf, PathBuf)> = match (&out.output, &out.out_dir) {
        (Some(o), None) => {
            if files.len() != 1 {
                bail!("use --out-dir with several input files");
            }
            vec![(files[0].clone(), o.clone())]
        }
        (None, Some(dir)) => {
            std::fs::create_dir_all(dir)?;
            files
                .iter()
                .map(|f| (f.clone(), dir.join(file_name(f))))
                .collect()
        }
        _ => bail!("give either --output or --out-dir"),
    };
    for (i, o) in &targets {
        if std::fs::canonicalize(i).ok() == std::fs::canonicalize(o).ok() {
            bail!(
                "refusing to overwrite the input {}; choose another output",
                i.display()
            );
        }
    }
    let failures: Vec<String> = targets
        .par_iter()
        .filter_map(|(i, o)| match f(i).and_then(|b| write(o, &b)) {
            Ok(()) => None,
            Err(e) => Some(format!("{}: {e:#}", i.display())),
        })
        .collect();
    if !failures.is_empty() {
        bail!(
            "{} of {} files failed:\n  {}",
            failures.len(),
            targets.len(),
            failures.join("\n  ")
        );
    }
    Ok(())
}

fn main() {
    if let Err(e) = run(Cli::parse()) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.cmd {
        Cmd::Info { file, pw, json } => {
            let mut s = session(&file, &pw)?;
            let st = s.state();
            if json {
                println!("{}", serde_json::to_string_pretty(&st.summary)?);
                return Ok(());
            }
            let Some(sum) = st.summary else {
                bail!("{}", st.view_only_reason.unwrap_or_default());
            };
            println!("File:        {}", file.display());
            println!("Size:        {} bytes", st.size);
            println!("PDF version: {}", sum.pdf_version);
            println!("Pages:       {}", sum.page_count);
            let m = &sum.metadata;
            for (k, v) in [
                ("Title", &m.title),
                ("Author", &m.author),
                ("Subject", &m.subject),
                ("Keywords", &m.keywords),
                ("Creator", &m.creator),
                ("Producer", &m.producer),
                ("Created", &m.creation_date),
                ("Modified", &m.modification_date),
            ] {
                if let Some(v) = v {
                    println!("{k:<12} {v}");
                }
            }
            let p = sum.permissions;
            println!(
                "Encrypted:   {}",
                if p.encrypted {
                    if p.owner_access {
                        "yes (full access)"
                    } else {
                        "yes (restricted)"
                    }
                } else {
                    "no"
                }
            );
            if p.encrypted && !p.owner_access {
                println!(
                    "  print={} modify={} copy={} annotate={} forms={} assemble={}",
                    p.print, p.modify, p.copy, p.annotate, p.fill_forms, p.assemble
                );
            }
            println!("Forms:       {}", sum.has_forms);
            println!("Signed:      {}", sum.has_signatures);
            println!("Bookmarks:   {}", sum.has_outline);
            println!("Tagged:      {}", sum.tagged);
            println!("JavaScript:  {}", sum.has_javascript);
            println!("Attachments: {}", sum.has_embedded_files);
        }
        Cmd::Merge {
            files,
            output,
            bookmarks,
            pw,
        } => {
            let inputs = files
                .par_iter()
                .map(|f| {
                    let pdf = load(&read(f)?, pw.password.as_deref()).map_err(|e| explain(e, f))?;
                    Ok(MergeInput {
                        name: stem(f),
                        pdf,
                        pages: None,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let doc = assemble::merge(
                inputs,
                MergeOptions {
                    bookmark_per_file: bookmarks,
                },
            )?;
            write(&output, &load::serialize(&doc, None)?)?;
        }
        Cmd::Split {
            file,
            every,
            ranges,
            single,
            out_dir,
            pw,
        } => {
            let plan = match (every, single) {
                (Some(n), _) => SplitPlan::EveryN { n },
                (None, true) => SplitPlan::SinglePages,
                (None, false) if !ranges.is_empty() => SplitPlan::Ranges { ranges },
                _ => bail!("give --every N, --single or --ranges"),
            };
            let mut s = session(&file, &pw)?;
            std::fs::create_dir_all(&out_dir)?;
            let base = stem(&file);
            for (label, bytes) in s.split(&plan).map_err(|e| explain(e, &file))? {
                let label = label.replace(", ", "_");
                write(
                    &fsio::unique_path(&out_dir, &format!("{base} p{label}"), "pdf"),
                    &bytes,
                )?;
            }
        }
        Cmd::Extract {
            file,
            pages,
            output,
            pw,
        } => {
            let mut s = session(&file, &pw)?;
            let idx = page_list(&pages, &mut s)?;
            write(&output, &s.extract(&idx).map_err(|e| explain(e, &file))?)?;
        }
        Cmd::Rotate {
            file,
            pages,
            degrees,
            output,
            pw,
        } => single_op(&file, &pw, &output, |s| {
            Ok(Op::Rotate {
                pages: page_list(&pages, s)?,
                degrees,
            })
        })?,
        Cmd::Delete {
            file,
            pages,
            output,
            pw,
        } => single_op(&file, &pw, &output, |s| {
            Ok(Op::Delete {
                pages: page_list(&pages, s)?,
            })
        })?,
        Cmd::Images2pdf {
            images: files,
            output,
            page_size,
            margin,
        } => {
            let data = files.iter().map(|f| read(f)).collect::<Result<Vec<_>>>()?;
            let opts = ImagePageOptions {
                page_size: match page_size {
                    PageSize::Fit => PageSizeChoice::FitImage,
                    PageSize::A4 => PageSizeChoice::A4,
                    PageSize::Letter => PageSizeChoice::Letter,
                },
                orientation: Orientation::Auto,
                margin,
            };
            let doc = images::images_to_pdf(&data, &opts)?;
            write(&output, &load::serialize(&doc, None)?)?;
        }
        Cmd::Protect {
            files,
            out,
            user_password,
            owner_password,
            no_print,
            no_copy,
            no_modify,
            no_annotate,
            no_assemble,
            no_fill_forms,
        } => {
            let allowed = AllowedActions {
                print: !no_print,
                print_high_quality: !no_print,
                modify: !no_modify,
                copy: !no_copy,
                annotate: !no_annotate,
                fill_forms: !no_fill_forms,
                assemble: !no_assemble,
            };
            let op = Op::Protect {
                options: ProtectOptions {
                    user_password,
                    owner_password,
                    allowed,
                },
            };
            batch(&files, &out, |f| {
                let mut s = session(f, &PasswordArg { password: None })?;
                s.apply(&op).map_err(|e| explain(e, f))?;
                Ok(s.bytes().to_vec())
            })?;
        }
        Cmd::Unprotect { files, out, pw } => batch(&files, &out, |f| {
            let mut s = session(f, &pw)?;
            s.apply(&Op::Unprotect).map_err(|e| explain(e, f))?;
            Ok(s.bytes().to_vec())
        })?,
        Cmd::Optimize {
            files,
            out,
            max_dpi,
            quality,
            pw,
        } => {
            let options = OptimizeOptions {
                images: max_dpi.map(|d| ImageOptions {
                    max_dpi: d,
                    jpeg_quality: quality,
                }),
            };
            batch(&files, &out, |f| {
                let before = std::fs::metadata(f)?.len();
                let mut s = session(f, &pw)?;
                let report = s
                    .apply(&Op::Optimize { options })
                    .map_err(|e| explain(e, f))?;
                let bytes = s.bytes().to_vec();
                if let Some(r) = report {
                    println!(
                        "{}: {} → {} bytes ({} duplicates merged, {} streams compressed, {} images downsampled)",
                        f.display(), before, bytes.len(), r.duplicate_streams_merged, r.streams_compressed, r.images_downsampled
                    );
                }
                Ok(bytes)
            })?;
        }
        Cmd::Meta {
            file,
            output,
            title,
            author,
            subject,
            keywords,
            pw,
        } => single_op(&file, &pw, &output, |_| {
            Ok(Op::SetMetadata {
                update: MetadataUpdate {
                    title,
                    author,
                    subject,
                    keywords,
                    creator: None,
                },
            })
        })?,
        Cmd::Sample { pages, output } => {
            write(
                &output,
                &opdf_core::fixtures::simple(pages.clamp(1, 100_000)),
            )?;
        }
    }
    Ok(())
}
