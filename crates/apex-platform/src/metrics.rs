//! Real resource measurements via `sysinfo` (works on every OS, so it is tested in CI on Linux too).

use apex_core::model::*;
use apex_core::safety;
use chrono::Utc;
use std::time::Instant;
use sysinfo::{
    CpuRefreshKind, DiskRefreshKind, Disks, MemoryRefreshKind, Networks, ProcessRefreshKind,
    ProcessesToUpdate, RefreshKind, System, UpdateKind,
};

/// Long-lived sampler. CPU and I/O rates are deltas between refreshes, so the
/// first sample after creation reports 0 for rates; callers sample periodically.
pub struct Monitor {
    sys: System,
    disks: Disks,
    nets: Networks,
    last: Instant,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Monitor {
    pub fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        let disks = Disks::new_with_refreshed_list_specifics(DiskRefreshKind::everything());
        let nets = Networks::new_with_refreshed_list();
        Self {
            sys,
            disks,
            nets,
            last: Instant::now(),
        }
    }

    fn proc_kind() -> ProcessRefreshKind {
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage()
            .with_exe(UpdateKind::OnlyIfNotSet)
    }

    /// Ensure at least the minimum CPU sampling interval has elapsed for meaningful CPU %.
    pub fn prime(&mut self) {
        self.sys.refresh_cpu_all();
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, Self::proc_kind());
        std::thread::sleep(
            sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(std::time::Duration::from_millis(400)),
        );
    }

    pub fn sample(&mut self) -> LiveSample {
        let elapsed = self.last.elapsed().as_secs_f64().max(0.001);
        self.last = Instant::now();
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.nets.refresh(true);
        self.disks
            .refresh_specifics(true, DiskRefreshKind::nothing().with_io_usage());
        let (rx, tx) = self.nets.list().values().fold((0u64, 0u64), |(r, t), n| {
            (r + n.received(), t + n.transmitted())
        });
        let (dr, dw) = self.disks.list().iter().fold((0u64, 0u64), |(r, w), d| {
            let u = d.usage();
            (r + u.read_bytes, w + u.written_bytes)
        });
        LiveSample {
            timestamp_ms: Utc::now().timestamp_millis(),
            cpu_percent: self.sys.global_cpu_usage(),
            per_core_percent: self.sys.cpus().iter().map(|c| c.cpu_usage()).collect(),
            memory_used_bytes: self
                .sys
                .total_memory()
                .saturating_sub(self.sys.available_memory()),
            memory_total_bytes: self.sys.total_memory(),
            swap_used_bytes: self.sys.used_swap(),
            net_rx_bytes_per_sec: (rx as f64 / elapsed) as u64,
            net_tx_bytes_per_sec: (tx as f64 / elapsed) as u64,
            disk_read_bytes_per_sec: (dr as f64 / elapsed) as u64,
            disk_write_bytes_per_sec: (dw as f64 / elapsed) as u64,
        }
    }

    /// All processes with CPU% since the previous call. Call `prime()` once first.
    pub fn processes(&mut self) -> Vec<ProcessInfo> {
        let since = self.last.elapsed().as_secs_f64().max(0.001);
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, Self::proc_kind());
        let me = std::process::id();
        let mut out: Vec<ProcessInfo> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, p)| {
                let name = p.name().to_string_lossy().into_owned();
                let pid_u = pid.as_u32();
                let du = p.disk_usage();
                ProcessInfo {
                    pid: pid_u,
                    parent_pid: p.parent().map(|x| x.as_u32()),
                    critical: safety::is_critical_process(&name, pid_u) || pid_u == me,
                    exe: p.exe().map(|e| e.display().to_string()),
                    name,
                    cpu_percent: p.cpu_usage(),
                    memory_bytes: p.memory(),
                    virtual_memory_bytes: p.virtual_memory(),
                    disk_read_bytes_per_sec: (du.read_bytes as f64 / since) as u64,
                    disk_write_bytes_per_sec: (du.written_bytes as f64 / since) as u64,
                    status: format!("{:?}", p.status()),
                    run_time_secs: p.run_time(),
                }
            })
            .collect();
        out.sort_by(|a, b| {
            b.cpu_percent
                .total_cmp(&a.cpu_percent)
                .then(b.memory_bytes.cmp(&a.memory_bytes))
        });
        out
    }

    /// Terminate a process after safety checks. Returns a user-facing message.
    pub fn terminate(&mut self, pid: u32) -> Result<String, String> {
        if pid == std::process::id() {
            return Err("APEX will not terminate itself.".into());
        }
        let spid = sysinfo::Pid::from_u32(pid);
        self.sys
            .refresh_processes(ProcessesToUpdate::Some(&[spid]), true);
        let Some(p) = self.sys.process(spid) else {
            return Err(format!("No process with PID {pid}."));
        };
        let name = p.name().to_string_lossy().into_owned();
        if safety::is_critical_process(&name, pid) {
            return Err(format!("{name} is a protected Windows or security process; terminating it could crash or expose the system."));
        }
        if p.kill() {
            Ok(format!(
                "Sent terminate to {name} (PID {pid}). Unsaved work in that app is lost."
            ))
        } else {
            Err(format!("Windows refused to terminate {name}. It may belong to another user or require administrator rights."))
        }
    }

    pub fn disks(&mut self) -> Vec<DiskInfo> {
        self.disks.refresh(true);
        let sys_mount = system_mount_point();
        self.disks
            .list()
            .iter()
            .map(|d| {
                let mount = d.mount_point().display().to_string();
                DiskInfo {
                    name: d.name().to_string_lossy().into_owned(),
                    is_system: mount.eq_ignore_ascii_case(&sys_mount),
                    mount_point: mount,
                    kind: match d.kind() {
                        sysinfo::DiskKind::SSD => DiskKind::Ssd,
                        sysinfo::DiskKind::HDD => DiskKind::Hdd,
                        _ => DiskKind::Unknown,
                    },
                    file_system: d.file_system().to_string_lossy().into_owned(),
                    total_bytes: d.total_space(),
                    available_bytes: d.available_space(),
                    removable: d.is_removable(),
                }
            })
            .collect()
    }

    pub fn base_snapshot(&mut self) -> SystemSnapshot {
        self.prime();
        let processes = self.processes();
        let _ = self.sample();
        let cpus = self.sys.cpus();
        let cpu = CpuInfo {
            brand: cpus
                .first()
                .map(|c| c.brand().trim().to_string())
                .unwrap_or_default(),
            vendor: cpus
                .first()
                .map(|c| c.vendor_id().to_string())
                .unwrap_or_default(),
            arch: System::cpu_arch(),
            physical_cores: System::physical_core_count(),
            logical_cores: cpus.len(),
            frequency_mhz: cpus.first().map(|c| c.frequency()).filter(|f| *f > 0),
            usage_percent: self.sys.global_cpu_usage(),
        };
        let memory = MemoryInfo {
            total_bytes: self.sys.total_memory(),
            used_bytes: self.sys.used_memory(),
            available_bytes: self.sys.available_memory(),
            swap_total_bytes: self.sys.total_swap(),
            swap_used_bytes: self.sys.used_swap(),
        };
        SystemSnapshot {
            captured_at: Utc::now(),
            os: OsInfo {
                name: System::name().unwrap_or_else(|| std::env::consts::OS.into()),
                version: System::os_version(),
                build: None,
                kernel: System::kernel_version(),
                hostname: System::host_name(),
            },
            cpu,
            memory,
            disks: self.disks(),
            gpus: vec![],
            power: None,
            game_mode_enabled: None,
            startup: vec![],
            top_processes: processes.into_iter().take(40).collect(),
            temp_cleanable_bytes: None,
            limitations: vec![],
        }
    }
}

pub fn network_interfaces() -> Vec<NetInterface> {
    let nets = Networks::new_with_refreshed_list();
    let mut v: Vec<NetInterface> = nets
        .list()
        .iter()
        .map(|(name, n)| NetInterface {
            name: name.clone(),
            mac: n.mac_address().to_string(),
            addresses: n
                .ip_networks()
                .iter()
                .map(|ip| format!("{}/{}", ip.addr, ip.prefix))
                .collect(),
            mtu: n.mtu(),
            state: format!("{:?}", n.operational_state()),
            total_received_bytes: n.total_received(),
            total_transmitted_bytes: n.total_transmitted(),
            total_errors: n.total_errors_on_received() + n.total_errors_on_transmitted(),
        })
        .collect();
    v.sort_by_key(|n| std::cmp::Reverse(n.total_received_bytes));
    v
}

/// Temperatures reported by the OS, if any. On many Windows PCs this is empty
/// without vendor drivers; the UI says so rather than inventing values.
pub fn temperatures() -> Vec<(String, f32)> {
    sysinfo::Components::new_with_refreshed_list()
        .list()
        .iter()
        .filter_map(|c| c.temperature().map(|t| (c.label().to_string(), t)))
        .filter(|(_, t)| t.is_finite() && *t > 0.0)
        .collect()
}

pub fn system_mount_point() -> String {
    #[cfg(windows)]
    {
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        format!("{}\\", drive.trim_end_matches('\\'))
    }
    #[cfg(not(windows))]
    {
        "/".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_snapshot_has_plausible_values() {
        let mut m = Monitor::new();
        let s = m.base_snapshot();
        assert!(s.cpu.logical_cores >= 1);
        assert!(s.memory.total_bytes > 0);
        assert!(s.memory.available_bytes <= s.memory.total_bytes);
        assert!(!s.top_processes.is_empty());
        // our own process must be marked protected from termination
        let me = std::process::id();
        let all = m.processes();
        assert!(all
            .iter()
            .find(|p| p.pid == me)
            .map(|p| p.critical)
            .unwrap_or(true));
    }

    #[test]
    fn refuses_to_kill_self_and_pid_1() {
        let mut m = Monitor::new();
        assert!(m.terminate(std::process::id()).is_err());
        assert!(m.terminate(1).is_err());
    }

    #[test]
    fn live_samples_are_bounded() {
        let mut m = Monitor::new();
        m.prime();
        let s = m.sample();
        assert!((0.0..=100.0).contains(&s.cpu_percent));
        assert!(s.memory_used_bytes <= s.memory_total_bytes);
    }
}
