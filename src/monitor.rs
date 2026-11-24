//! Фоновый сбор системной статистики (не блокирует интерфейс).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use sysinfo::{DiskKind, Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

pub const HIST: usize = 60;

#[derive(Clone, Default)]
pub struct ProcInfo {
    pub pid: u32,
    pub parent: Option<u32>,
    pub name: String,
    pub exe: String,
    pub cmd: String,
    pub user: String,
    pub cpu: f32,
    pub mem: u64,
    pub vmem: u64,
    pub read: f64,
    pub write: f64,
    pub start: u64,
    pub run_time: u64,
}

#[derive(Clone, Default)]
pub struct NetInfo {
    pub name: String,
    pub rx: f64,
    pub tx: f64,
    pub total_rx: u64,
    pub total_tx: u64,
    pub hist_rx: Vec<f32>,
    pub hist_tx: Vec<f32>,
}

#[derive(Clone, Default)]
pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    pub fs: String,
    pub kind: String,
    pub total: u64,
    pub avail: u64,
    pub removable: bool,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub generation: u64,
    pub procs: Vec<ProcInfo>,
    pub cpu_total: f32,
    pub cores: Vec<f32>,
    pub cpu_hist: Vec<f32>,
    pub core_hist: Vec<Vec<f32>>,
    pub cpu_brand: String,
    pub cpu_freq: u64,
    pub mem_total: u64,
    pub mem_used: u64,
    pub mem_avail: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub mem_hist: Vec<f32>,
    pub disk_read: f64,
    pub disk_write: f64,
    pub disk_read_hist: Vec<f32>,
    pub disk_write_hist: Vec<f32>,
    pub nets: Vec<NetInfo>,
    pub disks: Vec<DiskInfo>,
    pub uptime: u64,
    pub os: String,
    pub host: String,
}

struct Ring(VecDeque<f32>);

impl Ring {
    fn new() -> Self {
        Ring(VecDeque::from(vec![0.0; HIST]))
    }
    fn push(&mut self, v: f32) {
        if self.0.len() >= HIST {
            self.0.pop_front();
        }
        self.0.push_back(v);
    }
    fn vec(&self) -> Vec<f32> {
        self.0.iter().copied().collect()
    }
}

pub struct Monitor {
    latest: Arc<Mutex<Arc<Snapshot>>>,
    interval: Arc<AtomicU64>,
    paused: Arc<AtomicBool>,
    force: Arc<AtomicBool>,
    thread: std::thread::Thread,
}

impl Monitor {
    pub fn start(ctx: egui::Context, interval_ms: u64, paused: bool) -> Self {
        let latest = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
        let interval = Arc::new(AtomicU64::new(interval_ms));
        let paused = Arc::new(AtomicBool::new(paused));
        let force = Arc::new(AtomicBool::new(false));
        let (l, i, p, f) = (latest.clone(), interval.clone(), paused.clone(), force.clone());
        let handle = std::thread::Builder::new()
            .name("monitor".into())
            .spawn(move || worker(ctx, l, i, p, f))
            .expect("не удалось запустить поток мониторинга");
        Self { latest, interval, paused, force, thread: handle.thread().clone() }
    }

    pub fn latest(&self) -> Arc<Snapshot> {
        self.latest.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn set_interval(&self, ms: u64) {
        self.interval.store(ms, Ordering::Relaxed);
        self.thread.unpark();
    }

    pub fn set_paused(&self, p: bool) {
        self.paused.store(p, Ordering::Relaxed);
        self.thread.unpark();
    }

    pub fn refresh_now(&self) {
        self.force.store(true, Ordering::Relaxed);
        self.thread.unpark();
    }
}

fn worker(
    ctx: egui::Context,
    latest: Arc<Mutex<Arc<Snapshot>>>,
    interval: Arc<AtomicU64>,
    paused: Arc<AtomicBool>,
    force: Arc<AtomicBool>,
) {
    let kind = ProcessRefreshKind::new()
        .with_cpu()
        .with_memory()
        .with_disk_usage()
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_cmd(UpdateKind::OnlyIfNotSet);

    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    let mut networks = Networks::new_with_refreshed_list();
    let mut disks = Disks::new_with_refreshed_list();

    let cpu_brand = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
    let ncpu = sys.cpus().len().max(1);
    let cpu_freq = sys.cpus().first().map(|c| c.frequency()).unwrap_or(0);
    let os = System::long_os_version().unwrap_or_default();
    let host = System::host_name().unwrap_or_default();

    let mut cpu_hist = Ring::new();
    let mut core_hist: Vec<Ring> = (0..ncpu).map(|_| Ring::new()).collect();
    let mut mem_hist = Ring::new();
    let mut dr_hist = Ring::new();
    let mut dw_hist = Ring::new();
    let mut net_hist: HashMap<String, (Ring, Ring)> = HashMap::new();
    // (pid, start) -> пользователь
    let mut user_cache: HashMap<(u32, u64), String> = HashMap::new();

    std::thread::sleep(Duration::from_millis(250));
    let mut last = Instant::now();
    let mut generation = 0u64;

    loop {
        let start = Instant::now();
        let dt = last.elapsed().as_secs_f64().max(0.05);
        last = Instant::now();

        // ВАЖНО: не вызывать refresh_cpu_frequency() в цикле — на Windows это
        // сбрасывает счётчики sysinfo и даёт ложные пики загрузки до 100%.
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        networks.refresh();
        if generation % 10 == 0 {
            disks.refresh_list();
        }

        // ── процессы
        let mut procs = Vec::with_capacity(sys.processes().len());
        let mut disk_r = 0.0;
        let mut disk_w = 0.0;
        let mut alive = std::collections::HashSet::new();
        for (pid, p) in sys.processes() {
            let pid = pid.as_u32();
            let du = p.disk_usage();
            let read = du.read_bytes as f64 / dt;
            let write = du.written_bytes as f64 / dt;
            disk_r += read;
            disk_w += write;
            let key = (pid, p.start_time());
            alive.insert(key);
            let user = user_cache
                .entry(key)
                .or_insert_with(|| {
                    crate::win::process_user(pid).unwrap_or_else(|| {
                        if pid == 0 || pid == 4 {
                            "SYSTEM".into()
                        } else {
                            String::new()
                        }
                    })
                })
                .clone();
            let name = p.name().to_string_lossy().into_owned();
            procs.push(ProcInfo {
                pid,
                parent: p.parent().map(|pp| pp.as_u32()),
                name: if pid == 0 && name.is_empty() { "System Idle Process".into() } else { name },
                exe: p.exe().map(|e| e.display().to_string()).unwrap_or_default(),
                cmd: p
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" "),
                user,
                cpu: p.cpu_usage() / ncpu as f32,
                mem: p.memory(),
                vmem: p.virtual_memory(),
                read,
                write,
                start: p.start_time(),
                run_time: p.run_time(),
            });
        }
        user_cache.retain(|k, _| alive.contains(k));

        // ── ЦП
        let cores: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
        let cpu_total = sys.global_cpu_usage();
        cpu_hist.push(cpu_total);
        for (i, c) in cores.iter().enumerate() {
            if let Some(h) = core_hist.get_mut(i) {
                h.push(*c);
            }
        }

        // ── память
        let mem_total = sys.total_memory();
        let mem_used = sys.used_memory();
        let mem_avail = sys.available_memory();
        mem_hist.push(if mem_total > 0 { mem_used as f32 / mem_total as f32 * 100.0 } else { 0.0 });
        dr_hist.push(disk_r as f32);
        dw_hist.push(disk_w as f32);

        // ── сеть
        let mut nets = Vec::new();
        for (name, data) in &networks {
            let rx = data.received() as f64 / dt;
            let tx = data.transmitted() as f64 / dt;
            let e = net_hist.entry(name.clone()).or_insert_with(|| (Ring::new(), Ring::new()));
            e.0.push(rx as f32);
            e.1.push(tx as f32);
            if data.total_received() == 0 && data.total_transmitted() == 0 {
                continue;
            }
            nets.push(NetInfo {
                name: name.clone(),
                rx,
                tx,
                total_rx: data.total_received(),
                total_tx: data.total_transmitted(),
                hist_rx: e.0.vec(),
                hist_tx: e.1.vec(),
            });
        }
        nets.sort_by(|a, b| (b.total_rx + b.total_tx).cmp(&(a.total_rx + a.total_tx)));

        let disks_v = disks
            .list()
            .iter()
            .map(|d| DiskInfo {
                name: d.name().to_string_lossy().into_owned(),
                mount: d.mount_point().display().to_string(),
                fs: d.file_system().to_string_lossy().into_owned(),
                kind: match d.kind() {
                    DiskKind::SSD => "SSD".into(),
                    DiskKind::HDD => "HDD".into(),
                    _ => String::new(),
                },
                total: d.total_space(),
                avail: d.available_space(),
                removable: d.is_removable(),
            })
            .collect();

        generation += 1;
        let snap = Snapshot {
            generation,
            procs,
            cpu_total,
            cores,
            cpu_hist: cpu_hist.vec(),
            core_hist: core_hist.iter().map(|h| h.vec()).collect(),
            cpu_brand: cpu_brand.clone(),
            cpu_freq,
            mem_total,
            mem_used,
            mem_avail,
            swap_total: sys.total_swap(),
            swap_used: sys.used_swap(),
            mem_hist: mem_hist.vec(),
            disk_read: disk_r,
            disk_write: disk_w,
            disk_read_hist: dr_hist.vec(),
            disk_write_hist: dw_hist.vec(),
            nets,
            disks: disks_v,
            uptime: System::uptime(),
            os: os.clone(),
            host: host.clone(),
        };
        if let Ok(mut g) = latest.lock() {
            *g = Arc::new(snap);
        }
        ctx.request_repaint();

        // ── ожидание следующего цикла
        loop {
            if force.swap(false, Ordering::Relaxed) {
                break;
            }
            let is_paused = paused.load(Ordering::Relaxed);
            let target = Duration::from_millis(interval.load(Ordering::Relaxed).max(250));
            let spent = start.elapsed();
            if !is_paused && spent >= target {
                break;
            }
            let wait = if is_paused { Duration::from_millis(500) } else { target - spent };
            std::thread::park_timeout(wait);
        }
    }
}
