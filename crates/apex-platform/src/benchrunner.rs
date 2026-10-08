//! Runs a benchmark series and records the conditions it ran under, so noisy
//! results can be explained (background load, few runs) rather than trusted blindly.

use apex_core::bench::{self, BenchKind};
use apex_core::ledger::BenchmarkRecord;
use chrono::Utc;
use serde_json::json;
use std::path::Path;
use sysinfo::{CpuRefreshKind, RefreshKind, System};

pub struct SeriesOptions<'a> {
    pub kind: BenchKind,
    pub runs: usize,
    pub label: String,
    pub storage_dir: &'a Path,
}

fn background_cpu(sys: &mut System) -> f32 {
    sys.refresh_cpu_usage();
    std::thread::sleep(
        sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(std::time::Duration::from_millis(300)),
    );
    sys.refresh_cpu_usage();
    sys.global_cpu_usage()
}

/// One warm-up run (discarded), then `runs` measured runs.
/// `progress(done, total)` is called after each run.
pub fn run_series(
    opts: &SeriesOptions<'_>,
    mut progress: impl FnMut(usize, usize),
) -> std::io::Result<BenchmarkRecord> {
    let runs = opts.runs.clamp(1, 50);
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::nothing().with_cpu_usage()),
    );
    let mut background = Vec::with_capacity(runs);
    let _warm = bench::run_once(opts.kind, opts.storage_dir)?;
    let mut samples = Vec::with_capacity(runs);
    for i in 0..runs {
        background.push(background_cpu(&mut sys));
        samples.push(bench::run_once(opts.kind, opts.storage_dir)?);
        progress(i + 1, runs);
    }
    let max_bg = background.iter().cloned().fold(0.0f32, f32::max);
    let mut warnings = Vec::new();
    if max_bg > 20.0 {
        warnings.push(format!(
            "Background CPU load reached {max_bg:.0}% before a run; results may be noisy."
        ));
    }
    if runs < 3 {
        warnings.push(
            "Fewer than 3 runs: comparisons with this result will be inconclusive.".to_string(),
        );
    }
    if let Some(st) = bench::stats(&samples) {
        if st.cv_percent > 10.0 {
            warnings.push(format!(
                "Run-to-run variation is high (CV {:.1}%).",
                st.cv_percent
            ));
        }
    }
    let temps = crate::metrics::temperatures();
    Ok(BenchmarkRecord {
        id: uuid_like(),
        created_at: Utc::now(),
        kind: opts.kind.id().into(),
        label: opts.label.clone(),
        unit: opts.kind.unit().into(),
        higher_is_better: true,
        samples,
        context: json!({
            "benchmark": opts.kind.label(),
            "warmupRuns": 1,
            "backgroundCpuPercentBeforeEachRun": background,
            "temperaturesC": temps,
            "temperatureAvailable": !temps.is_empty(),
            "logicalCpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            "storageDir": opts.storage_dir.display().to_string(),
            "warnings": warnings,
        }),
    })
}

fn uuid_like() -> String {
    // Avoids a uuid dependency here; time + pid + counter is unique enough for local history.
    use std::sync::atomic::{AtomicU64, Ordering};
    static C: AtomicU64 = AtomicU64::new(0);
    format!(
        "bench-{}-{}-{}",
        Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id(),
        C.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_series_records_context() {
        let d = tempfile::tempdir().unwrap();
        // StorageSequential writes 512 MB per run; use memory bench (fast) for the unit test.
        let opts = SeriesOptions {
            kind: BenchKind::MemoryBandwidth,
            runs: 2,
            label: "t".into(),
            storage_dir: d.path(),
        };
        let mut calls = 0;
        let rec = run_series(&opts, |_, _| calls += 1).unwrap();
        assert_eq!(rec.samples.len(), 2);
        assert_eq!(calls, 2);
        assert!(rec.samples.iter().all(|s| *s > 0.0));
        assert!(rec.context["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("Fewer than 3")));
    }
}
