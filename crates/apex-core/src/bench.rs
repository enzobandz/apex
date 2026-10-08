//! Benchmark workloads and statistics.
//!
//! Workloads are fixed, deterministic computations timed with a monotonic clock.
//! Comparisons use Welch's t-test plus a minimum practical difference, so a result
//! is only called an improvement when it is both statistically and practically
//! distinguishable from run-to-run noise.

use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::hint::black_box;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub n: usize,
    pub mean: f64,
    pub median: f64,
    pub min: f64,
    pub max: f64,
    pub stdev: f64,
    /// Coefficient of variation in percent (stdev / mean).
    pub cv_percent: f64,
    /// Half-width of the 95% confidence interval of the mean.
    pub ci95: f64,
}

/// Two-sided 95% Student t critical values for df = 1..=30.
const T95: [f64; 30] = [
    12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
    2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
    2.052, 2.048, 2.045, 2.042,
];

pub fn t_crit_95(df: f64) -> f64 {
    if df < 1.0 {
        return T95[0];
    }
    if df >= 30.0 {
        return if df >= 120.0 { 1.980 } else { 2.000 };
    }
    // Conservative: use the lower integer df.
    T95[(df.floor() as usize).clamp(1, 30) - 1]
}

pub fn stats(samples: &[f64]) -> Option<Stats> {
    let n = samples.len();
    if n == 0 {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let mean = samples.iter().sum::<f64>() / n as f64;
    let median = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    };
    let var = if n > 1 {
        samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0)
    } else {
        0.0
    };
    let stdev = var.sqrt();
    let ci95 = if n > 1 {
        t_crit_95((n - 1) as f64) * stdev / (n as f64).sqrt()
    } else {
        f64::NAN
    };
    Some(Stats {
        n,
        mean,
        median,
        min: sorted[0],
        max: sorted[n - 1],
        stdev,
        cv_percent: if mean != 0.0 {
            stdev / mean.abs() * 100.0
        } else {
            0.0
        },
        ci95,
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Improved,
    Regressed,
    NoSignificantChange,
    /// Not enough runs (need ≥3 each) or noise too high to say anything.
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub before: Stats,
    pub after: Stats,
    /// Percent change of the mean, signed so that positive = better.
    pub improvement_percent: f64,
    pub t_statistic: f64,
    pub degrees_of_freedom: f64,
    pub significant: bool,
    pub verdict: Verdict,
    pub explanation: String,
}

/// Minimum change (percent) treated as practically meaningful.
pub const MIN_PRACTICAL_PERCENT: f64 = 2.0;

pub fn compare(before: &[f64], after: &[f64], higher_is_better: bool) -> Option<Comparison> {
    let b = stats(before)?;
    let a = stats(after)?;
    if b.n < 3 || a.n < 3 {
        return Some(Comparison {
            improvement_percent: 0.0,
            t_statistic: 0.0,
            degrees_of_freedom: 0.0,
            significant: false,
            verdict: Verdict::Inconclusive,
            explanation:
                "At least 3 runs before and after are needed to separate a real change from noise."
                    .into(),
            before: b,
            after: a,
        });
    }
    let vb = b.stdev.powi(2) / b.n as f64;
    let va = a.stdev.powi(2) / a.n as f64;
    let se = (vb + va).sqrt();
    let diff = a.mean - b.mean;
    let (t, df) = if se == 0.0 {
        (
            if diff == 0.0 {
                0.0
            } else {
                f64::INFINITY * diff.signum()
            },
            (a.n + b.n - 2) as f64,
        )
    } else {
        let df = (vb + va).powi(2)
            / (vb.powi(2) / (b.n as f64 - 1.0) + va.powi(2) / (a.n as f64 - 1.0))
                .max(f64::MIN_POSITIVE);
        (diff / se, df)
    };
    let significant = t.abs() > t_crit_95(df);
    let raw_pct = if b.mean != 0.0 {
        diff / b.mean.abs() * 100.0
    } else {
        0.0
    };
    let improvement = if higher_is_better { raw_pct } else { -raw_pct };
    let practical = improvement.abs() >= MIN_PRACTICAL_PERCENT;
    let noisy = b.cv_percent > 15.0 || a.cv_percent > 15.0;

    let (verdict, explanation) = if significant && practical {
        if improvement > 0.0 {
            (
                Verdict::Improved,
                format!(
                    "{improvement:.1}% better, larger than run-to-run variation (95% confidence)."
                ),
            )
        } else {
            (Verdict::Regressed, format!("{:.1}% worse, larger than run-to-run variation (95% confidence). Consider undoing the change.", -improvement))
        }
    } else if noisy {
        (Verdict::Inconclusive, format!(
            "Runs varied too much (CV {:.0}% before, {:.0}% after) to detect a difference. Close other apps, let the PC cool and retry with more runs.",
            b.cv_percent, a.cv_percent
        ))
    } else {
        (
            Verdict::NoSignificantChange,
            format!(
                "{improvement:+.1}% difference is within measurement noise; no measurable effect."
            ),
        )
    };
    Some(Comparison {
        before: b,
        after: a,
        improvement_percent: improvement,
        t_statistic: t,
        degrees_of_freedom: df,
        significant,
        verdict,
        explanation,
    })
}

// ---------------------------------------------------------------- workloads

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum BenchKind {
    CpuSingle,
    CpuMulti,
    MemoryBandwidth,
    StorageSequential,
}

impl BenchKind {
    pub fn id(self) -> &'static str {
        match self {
            BenchKind::CpuSingle => "cpuSingle",
            BenchKind::CpuMulti => "cpuMulti",
            BenchKind::MemoryBandwidth => "memoryBandwidth",
            BenchKind::StorageSequential => "storageSequential",
        }
    }
    pub fn unit(self) -> &'static str {
        match self {
            BenchKind::CpuSingle | BenchKind::CpuMulti => "Mops/s",
            BenchKind::MemoryBandwidth => "GB/s",
            BenchKind::StorageSequential => "MB/s",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            BenchKind::CpuSingle => "CPU single-thread",
            BenchKind::CpuMulti => "CPU multi-thread",
            BenchKind::MemoryBandwidth => "Memory copy bandwidth",
            BenchKind::StorageSequential => "Storage sequential write (flushed)",
        }
    }
}

/// Integer/branch-heavy kernel: xorshift PRNG + prime sieve. Returns operations performed.
fn cpu_kernel(iterations: u64) -> u64 {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut acc: u64 = 0;
    for i in 0..iterations {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        acc = acc.wrapping_add(x.rotate_left((i & 63) as u32) ^ i);
        if x & 7 == 0 {
            acc = acc.wrapping_mul(3);
        }
    }
    // small sieve to add memory-access/branch pattern
    const N: usize = 200_000;
    let mut sieve = vec![true; N];
    let mut primes = 0u64;
    for i in 2..N {
        if sieve[i] {
            primes += 1;
            let mut j = i * i;
            while j < N {
                sieve[j] = false;
                j += i;
            }
        }
    }
    black_box(acc ^ primes);
    iterations + N as u64
}

const CPU_ITERS: u64 = 60_000_000;

pub fn run_cpu_single() -> f64 {
    let t = Instant::now();
    let ops = cpu_kernel(black_box(CPU_ITERS));
    ops as f64 / t.elapsed().as_secs_f64() / 1e6
}

pub fn run_cpu_multi() -> f64 {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let t = Instant::now();
    let total: u64 = std::thread::scope(|s| {
        let hs: Vec<_> = (0..threads)
            .map(|_| s.spawn(|| cpu_kernel(black_box(CPU_ITERS / 2))))
            .collect();
        hs.into_iter().map(|h| h.join().unwrap_or(0)).sum()
    });
    total as f64 / t.elapsed().as_secs_f64() / 1e6
}

pub fn run_memory_bandwidth() -> f64 {
    const SIZE: usize = 256 * 1024 * 1024;
    const REPS: usize = 4;
    let src = vec![1u8; SIZE];
    let mut dst = vec![0u8; SIZE];
    // touch dst so page faults are excluded from timing
    dst.iter_mut().step_by(4096).for_each(|b| *b = 2);
    let t = Instant::now();
    for _ in 0..REPS {
        dst.copy_from_slice(black_box(&src));
        black_box(&dst);
    }
    // copy reads + writes each byte: count both directions
    (2 * SIZE * REPS) as f64 / t.elapsed().as_secs_f64() / 1e9
}

/// Writes `size_mb` to a new file in `dir` and flushes it to the device (`sync_all`),
/// so the result reflects the drive rather than RAM caching. The file is always removed.
pub fn run_storage_write(dir: &Path, size_mb: usize) -> std::io::Result<f64> {
    let path = dir.join(format!("apex-bench-{}.tmp", std::process::id()));
    let result = (|| {
        let mut f = File::create(&path)?;
        let block = vec![0xA5u8; 4 * 1024 * 1024];
        let blocks = size_mb.div_ceil(4);
        let t = Instant::now();
        for _ in 0..blocks {
            f.write_all(&block)?;
        }
        f.sync_all()?;
        let secs = t.elapsed().as_secs_f64();
        // Read back once to validate integrity (not timed: would be served from cache).
        drop(f);
        let mut r = File::open(&path)?;
        let mut buf = vec![0u8; 4 * 1024 * 1024];
        let n = r.read(&mut buf)?;
        if n == 0 || buf[..n].iter().any(|b| *b != 0xA5) {
            return Err(std::io::Error::other("read-back verification failed"));
        }
        Ok((blocks * 4) as f64 / secs)
    })();
    let _ = fs::remove_file(&path);
    result
}

pub fn run_once(kind: BenchKind, storage_dir: &Path) -> std::io::Result<f64> {
    Ok(match kind {
        BenchKind::CpuSingle => run_cpu_single(),
        BenchKind::CpuMulti => run_cpu_multi(),
        BenchKind::MemoryBandwidth => run_memory_bandwidth(),
        BenchKind::StorageSequential => run_storage_write(storage_dir, 512)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_are_correct() {
        let s = stats(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap();
        assert!((s.mean - 5.0).abs() < 1e-12);
        assert!((s.median - 4.5).abs() < 1e-12);
        assert!((s.stdev - 2.138089935).abs() < 1e-6);
    }

    #[test]
    fn identical_distributions_are_not_significant() {
        let a = [100.0, 101.0, 99.0, 100.5, 99.5];
        let c = compare(&a, &a, true).unwrap();
        assert_eq!(c.verdict, Verdict::NoSignificantChange);
    }

    #[test]
    fn clear_improvement_detected_both_directions() {
        let before = [100.0, 101.0, 99.0, 100.5, 99.5];
        let after = [110.0, 111.0, 109.0, 110.5, 109.5];
        assert_eq!(
            compare(&before, &after, true).unwrap().verdict,
            Verdict::Improved
        );
        // lower-is-better metric (e.g. seconds): same numbers = regression
        assert_eq!(
            compare(&before, &after, false).unwrap().verdict,
            Verdict::Regressed
        );
    }

    #[test]
    fn tiny_but_significant_change_is_not_called_improvement() {
        let before = [100.0, 100.01, 99.99, 100.0, 100.0];
        let after = [101.0, 101.01, 100.99, 101.0, 101.0]; // 1% < MIN_PRACTICAL
        assert_eq!(
            compare(&before, &after, true).unwrap().verdict,
            Verdict::NoSignificantChange
        );
    }

    #[test]
    fn too_few_runs_inconclusive() {
        assert_eq!(
            compare(&[1.0, 2.0], &[3.0, 4.0], true).unwrap().verdict,
            Verdict::Inconclusive
        );
    }

    #[test]
    fn noisy_runs_inconclusive() {
        let before = [50.0, 150.0, 80.0, 120.0];
        let after = [60.0, 140.0, 90.0, 130.0];
        assert_eq!(
            compare(&before, &after, true).unwrap().verdict,
            Verdict::Inconclusive
        );
    }

    #[test]
    fn storage_bench_cleans_up() {
        let d = tempfile::tempdir().unwrap();
        let mbps = run_storage_write(d.path(), 8).unwrap();
        assert!(mbps > 0.0);
        assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
    }
}
