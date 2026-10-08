//! `apex` — command-line access to the same engine the desktop app uses.
//! Useful for scripting, remote diagnostics and verifying behaviour without the UI.

use anyhow::{bail, Context, Result};
use apex_core::actions::Action;
use apex_core::bench::{compare, BenchKind};
use apex_core::executor::Executor;
use apex_core::ledger::Ledger;
use apex_core::net;
use apex_core::recommend::{recommend, Mode};
use apex_core::storage;
use apex_platform::benchrunner::{run_series, SeriesOptions};
use apex_platform::{backend, collect_snapshot, Monitor};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "apex",
    version,
    about = "APEX Windows performance optimizer — CLI"
)]
struct Cli {
    /// Path to the APEX database (defaults to the desktop app's database).
    #[arg(long, global = true)]
    db: Option<PathBuf>,
    /// Print machine-readable JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Balanced,
    Gaming,
    Max,
}

#[derive(Clone, Copy, ValueEnum)]
enum BenchArg {
    CpuSingle,
    CpuMulti,
    Memory,
    Storage,
}

#[derive(Subcommand)]
enum Cmd {
    /// Measure the system and print a diagnostic snapshot.
    Snapshot,
    /// Show recommendations for a mode.
    Recommend {
        #[arg(value_enum, default_value = "balanced")]
        mode: ModeArg,
    },
    /// Apply recommendations by id (see `recommend`). Requires --yes.
    Apply {
        #[arg(value_enum, long, default_value = "balanced")]
        mode: ModeArg,
        ids: Vec<String>,
        #[arg(long)]
        yes: bool,
    },
    /// List optimization history.
    History {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Undo a whole batch or a single change.
    Undo {
        id: String,
        #[arg(long)]
        change: bool,
        #[arg(long)]
        force: bool,
    },
    /// Run a benchmark series and store it.
    Bench {
        #[arg(value_enum)]
        kind: BenchArg,
        #[arg(long, default_value_t = 5)]
        runs: usize,
        #[arg(long, default_value = "manual")]
        label: String,
    },
    /// Compare two stored benchmark results (before, after).
    Compare { before: String, after: String },
    /// Analyse temp files (read-only).
    Temp {
        #[arg(long, default_value_t = 7)]
        days: u32,
    },
    /// Largest files under a folder (read-only).
    Large {
        path: PathBuf,
        #[arg(long, default_value_t = 25)]
        top: usize,
    },
    /// Measure DNS resolver response times.
    Dns {
        #[arg(long, default_value_t = 3)]
        rounds: u32,
    },
}

pub fn default_db() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("APEX").join("apex.db")
}

fn mode(m: ModeArg) -> Mode {
    match m {
        ModeArg::Balanced => Mode::Balanced,
        ModeArg::Gaming => Mode::Gaming,
        ModeArg::Max => Mode::MaximumPerformance,
    }
}

fn print<T: serde::Serialize>(json: bool, v: &T, human: impl FnOnce()) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(v)?);
    } else {
        human();
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db = cli.db.clone().unwrap_or_else(default_db);
    if let Some(p) = db.parent() {
        std::fs::create_dir_all(p).ok();
    }
    let ledger = Ledger::open(&db).with_context(|| format!("opening {}", db.display()))?;
    let be = backend();
    let ex = Executor::new(be.as_ref(), &ledger);
    for r in ex.recover()? {
        eprintln!(
            "recovered interrupted change: {} -> {:?}",
            r.description, r.resolved_status
        );
    }

    match cli.cmd {
        Cmd::Snapshot => {
            let s = collect_snapshot(&mut Monitor::new(), be.as_ref());
            print(cli.json, &s, || {
                println!(
                    "{} {} (build {:?})",
                    s.os.name,
                    s.os.version.clone().unwrap_or_default(),
                    s.os.build
                );
                println!(
                    "CPU: {} — {} logical / {:?} physical cores, {:.0}% busy",
                    s.cpu.brand, s.cpu.logical_cores, s.cpu.physical_cores, s.cpu.usage_percent
                );
                println!(
                    "RAM: {} used of {}",
                    storage::human_bytes(s.memory.total_bytes - s.memory.available_bytes),
                    storage::human_bytes(s.memory.total_bytes)
                );
                for d in &s.disks {
                    println!(
                        "Disk {} {:?} {}: {} free of {}{}",
                        d.mount_point,
                        d.kind,
                        d.file_system,
                        storage::human_bytes(d.available_bytes),
                        storage::human_bytes(d.total_bytes),
                        if d.is_system { " (system)" } else { "" }
                    );
                }
                for g in &s.gpus {
                    println!("GPU: {}", g.name);
                }
                if let Some(p) = &s.power {
                    println!("Power plan: {}", p.active.name);
                }
                println!(
                    "Startup entries: {} ({} enabled)",
                    s.startup.len(),
                    s.startup.iter().filter(|e| e.enabled).count()
                );
                for l in &s.limitations {
                    println!("note: {l}");
                }
            })?;
        }
        Cmd::Recommend { mode: m } => {
            let s = collect_snapshot(&mut Monitor::new(), be.as_ref());
            let recs = recommend(&s, mode(m));
            print(cli.json, &recs, || {
                if recs.is_empty() {
                    println!("No recommendations: nothing measured on this PC needs attention for this mode.");
                }
                for r in &recs {
                    println!("[{:?} · confidence {} · risk {:?}] {}\n  id: {}\n  why: {}\n  downsides: {}\n  {}", r.category, r.confidence, r.risk, r.title, r.id, r.expected_benefit, r.downsides, match &r.action { Some(a) => format!("action: {}", a.describe()), None => "advice only".into() });
                    for e in &r.evidence {
                        println!("  evidence: {e}");
                    }
                }
            })?;
        }
        Cmd::Apply { mode: m, ids, yes } => {
            let s = collect_snapshot(&mut Monitor::new(), be.as_ref());
            let recs = recommend(&s, mode(m));
            let actions: Vec<Action> = ids
                .iter()
                .map(|id| {
                    recs.iter()
                        .find(|r| &r.id == id)
                        .and_then(|r| r.action.clone())
                        .with_context(|| format!("{id} is not a current actionable recommendation"))
                })
                .collect::<Result<_>>()?;
            if actions.is_empty() {
                bail!("no recommendation ids given");
            }
            for a in &actions {
                println!("will: {}", a.describe());
            }
            if !yes {
                bail!("re-run with --yes to apply");
            }
            let rep = ex.apply_plan("CLI plan", &actions)?;
            print(cli.json, &rep, || {
                for r in &rep.results {
                    println!("{:?}: {}", r.outcome, r.description);
                }
                println!("{}\nbatch: {}", rep.summary, rep.batch_id);
            })?;
        }
        Cmd::History { limit } => {
            let h = ledger.history(limit)?;
            print(cli.json, &h, || {
                for b in &h {
                    println!(
                        "{} {} {:?} ({})",
                        b.created_at.format("%Y-%m-%d %H:%M"),
                        b.id,
                        b.status,
                        b.label
                    );
                    for c in &b.changes {
                        println!("   {:?} {} [{}]", c.status, c.action.describe(), c.id);
                    }
                }
            })?;
        }
        Cmd::Undo { id, change, force } => {
            if change {
                let r = ex.undo_change(&id, force)?;
                println!("{}: {}", r.description, r.message);
            } else {
                for r in ex.undo_batch(&id, force)? {
                    println!(
                        "{} — {}: {}",
                        if r.ok { "ok" } else { "FAILED" },
                        r.description,
                        r.message
                    );
                }
            }
        }
        Cmd::Bench { kind, runs, label } => {
            let kind = match kind {
                BenchArg::CpuSingle => BenchKind::CpuSingle,
                BenchArg::CpuMulti => BenchKind::CpuMulti,
                BenchArg::Memory => BenchKind::MemoryBandwidth,
                BenchArg::Storage => BenchKind::StorageSequential,
            };
            let dir = std::env::temp_dir();
            let rec = run_series(
                &SeriesOptions {
                    kind,
                    runs,
                    label,
                    storage_dir: &dir,
                },
                |d, t| eprintln!("run {d}/{t}"),
            )?;
            ledger.save_benchmark(&rec)?;
            let st = apex_core::bench::stats(&rec.samples).unwrap();
            print(cli.json, &rec, || {
                println!(
                    "{}: mean {:.2} {} ± {:.2} (95% CI), CV {:.1}%, n={}",
                    kind.label(),
                    st.mean,
                    rec.unit,
                    st.ci95,
                    st.cv_percent,
                    st.n
                );
                println!("saved as {}", rec.id);
                for w in rec.context["warnings"].as_array().into_iter().flatten() {
                    println!("warning: {}", w.as_str().unwrap_or_default());
                }
            })?;
        }
        Cmd::Compare { before, after } => {
            let b = ledger.get_benchmark(&before)?;
            let a = ledger.get_benchmark(&after)?;
            if a.kind != b.kind {
                bail!("cannot compare {} with {}", b.kind, a.kind);
            }
            let c =
                compare(&b.samples, &a.samples, b.higher_is_better).context("empty sample set")?;
            print(cli.json, &c, || {
                println!("{:?}: {}", c.verdict, c.explanation)
            })?;
        }
        Cmd::Temp { days } => {
            let a = storage::analyze_temp(&be.temp_roots(), days);
            print(cli.json, &a, || {
                println!(
                    "{} files ({}) in temp; {} files ({}) older than {days} days",
                    a.total_files,
                    storage::human_bytes(a.total_bytes),
                    a.eligible_files,
                    storage::human_bytes(a.eligible_bytes)
                )
            })?;
        }
        Cmd::Large { path, top } => {
            let s = storage::scan_large_files(&path, top, 2_000_000)?;
            print(cli.json, &s, || {
                for f in &s.largest {
                    println!("{:>10}  {}", storage::human_bytes(f.size_bytes), f.path);
                }
            })?;
        }
        Cmd::Dns { rounds } => {
            let results: Vec<_> = net::KNOWN_RESOLVERS
                .iter()
                .map(|(ip, label)| {
                    net::dns_test(
                        ip.parse().unwrap(),
                        label,
                        net::TEST_NAMES,
                        rounds,
                        Duration::from_millis(1500),
                    )
                })
                .collect();
            print(cli.json, &results, || {
                for r in &results {
                    match r.median_ms {
                        Some(m) => println!(
                            "{:<20} median {:.1} ms, jitter {:.1} ms, loss {:.0}%",
                            r.label,
                            m,
                            r.jitter_ms.unwrap_or(0.0),
                            r.loss_percent
                        ),
                        None => println!(
                            "{:<20} no replies ({})",
                            r.label,
                            r.errors.first().cloned().unwrap_or_default()
                        ),
                    }
                }
            })?;
        }
    }
    Ok(())
}
