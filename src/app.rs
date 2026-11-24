use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, collapsing_header::CollapsingState, pos2, vec2, Align, Align2, Button, Color32, FontFamily, FontId,
    Frame, Key, Layout, Margin, Rect, RichText, Rounding, ScrollArea, Sense, Stroke, TextEdit, Ui, ViewportCommand,
};
use egui_extras::{Column, TableBuilder};

use crate::i18n::{self, tr, Lang};
use crate::monitor::{Monitor, ProcInfo, Snapshot, HIST};
use crate::settings::{BlurMode, Settings, DEFAULT_ACCENT, DEFAULT_BG};
use crate::theme::{card, TextColor, Theme, ThemeId};
use crate::util::*;
use crate::win::{self, Backdrop, ServiceInfo, StartupItem};
use crate::APP_TITLE;

// ─────────────────────────────── типы ───────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Processes,
    Performance,
    Users,
    Details,
    Startup,
    Services,
    Settings,
}

impl Tab {
    const MAIN: [Tab; 6] = [Tab::Processes, Tab::Performance, Tab::Users, Tab::Details, Tab::Startup, Tab::Services];
    fn icon(self) -> &'static str {
        match self {
            Tab::Processes => "☰",
            Tab::Performance => "📈",
            Tab::Users => "👤",
            Tab::Details => "📋",
            Tab::Startup => "🚀",
            Tab::Services => "🔧",
            Tab::Settings => "⚙",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Tab::Processes => tr("Processes", "Процессы", "Процеси"),
            Tab::Performance => tr("Performance", "Производительность", "Продуктивність"),
            Tab::Users => tr("Users", "Пользователи", "Користувачі"),
            Tab::Details => tr("Details", "Подробности", "Подробиці"),
            Tab::Startup => tr("Startup apps", "Автозагрузка", "Автозавантаження"),
            Tab::Services => tr("Services", "Службы", "Служби"),
            Tab::Settings => tr("Settings", "Параметры", "Параметри"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortCol {
    Name,
    Pid,
    Parent,
    Status,
    User,
    Cpu,
    Mem,
    VMem,
    Disk,
    Read,
    Write,
    Runtime,
    Path,
}

impl SortCol {
    fn default_desc(self) -> bool {
        !matches!(self, SortCol::Name | SortCol::User | SortCol::Path | SortCol::Status | SortCol::Pid | SortCol::Parent)
    }
}

#[derive(Clone, PartialEq)]
enum Sel {
    None,
    Proc(u32),
    Group(String),
}

#[derive(Clone, PartialEq)]
enum PerfItem {
    Cpu,
    Memory,
    Disk,
    Net(String),
}

enum Action {
    Kill(u32),
    KillTree(u32),
    KillMany(Vec<u32>, String),
    Suspend(u32),
    Resume(u32),
    Priority(u32, u32),
    OpenLocation(String),
    Search(String),
    Copy(String),
    Details(u32),
    Toggle(String),
}

struct RunDialog {
    text: String,
    admin: bool,
    first: bool,
}

struct Confirm {
    title: String,
    text: String,
    pids: Vec<u32>,
    label: String,
}

struct Group {
    key: String,
    label: String,
    items: Vec<usize>,
    cpu: f32,
    mem: u64,
    disk: f64,
}

enum Row {
    Group(usize),
    Proc(usize, bool),
}

#[derive(Clone, Copy, PartialEq)]
struct ThemeKey {
    theme: ThemeId,
    opacity: u32,
    bg: [u8; 3],
    accent: [u8; 3],
    text: TextColor,
}

impl ThemeKey {
    fn of(s: &Settings) -> Self {
        Self {
            theme: s.theme,
            opacity: (s.glass_opacity * 1000.0) as u32,
            bg: s.custom_bg,
            accent: s.custom_accent,
            text: s.text_color,
        }
    }
}

const CRITICAL: [&str; 9] = [
    "system",
    "registry",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "memory compression",
];

fn is_critical(name: &str) -> bool {
    let n = name.to_lowercase();
    CRITICAL.iter().any(|c| *c == n)
}

fn bold(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text.into()).size(size).family(FontFamily::Name("bold".into()))
}

fn ord(o: Ordering, desc: bool) -> Ordering {
    if desc {
        o.reverse()
    } else {
        o
    }
}

fn fcmp(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

fn cmp_proc(a: &ProcInfo, b: &ProcInfo, col: SortCol) -> Ordering {
    let o = match col {
        SortCol::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        SortCol::Pid => a.pid.cmp(&b.pid),
        SortCol::Parent => a.parent.cmp(&b.parent),
        SortCol::Status => Ordering::Equal,
        SortCol::User => a.user.to_lowercase().cmp(&b.user.to_lowercase()),
        SortCol::Cpu => fcmp(a.cpu as f64, b.cpu as f64),
        SortCol::Mem => a.mem.cmp(&b.mem),
        SortCol::VMem => a.vmem.cmp(&b.vmem),
        SortCol::Disk => fcmp(a.read + a.write, b.read + b.write),
        SortCol::Read => fcmp(a.read, b.read),
        SortCol::Write => fcmp(a.write, b.write),
        SortCol::Runtime => a.run_time.cmp(&b.run_time),
        SortCol::Path => a.exe.to_lowercase().cmp(&b.exe.to_lowercase()),
    };
    o.then(a.pid.cmp(&b.pid))
}

fn cmp_group(a: &Group, b: &Group, col: SortCol, procs: &[ProcInfo]) -> Ordering {
    let o = match col {
        SortCol::Cpu => fcmp(a.cpu as f64, b.cpu as f64),
        SortCol::Mem => a.mem.cmp(&b.mem),
        SortCol::Disk | SortCol::Read | SortCol::Write => fcmp(a.disk, b.disk),
        SortCol::Pid => {
            let pa = a.items.iter().map(|i| procs[*i].pid).min();
            let pb = b.items.iter().map(|i| procs[*i].pid).min();
            pa.cmp(&pb)
        }
        SortCol::User => {
            let ua = a.items.first().map(|i| procs[*i].user.to_lowercase());
            let ub = b.items.first().map(|i| procs[*i].user.to_lowercase());
            ua.cmp(&ub)
        }
        _ => a.label.to_lowercase().cmp(&b.label.to_lowercase()),
    };
    o.then_with(|| a.key.cmp(&b.key))
}

fn make_group(key: String, label: String, items: Vec<usize>, procs: &[ProcInfo]) -> Group {
    let mut cpu = 0.0;
    let mut mem = 0;
    let mut disk = 0.0;
    for &i in &items {
        cpu += procs[i].cpu;
        mem += procs[i].mem;
        disk += procs[i].read + procs[i].write;
    }
    Group { key, label, items, cpu, mem, disk }
}

/// Дочерние процессы (с проверкой времени старта, чтобы не задеть
/// процессы с переиспользованным PID). Порядок: сначала самые глубокие.
fn descendants(snap: &Snapshot, root: u32) -> Vec<u32> {
    let mut order = vec![root];
    let mut i = 0;
    while i < order.len() {
        let cur = order[i];
        let cur_start = snap.procs.iter().find(|p| p.pid == cur).map(|p| p.start).unwrap_or(0);
        for p in &snap.procs {
            if p.parent == Some(cur) && p.pid != cur && p.start >= cur_start && !order.contains(&p.pid) {
                order.push(p.pid);
            }
        }
        i += 1;
    }
    order.reverse();
    order
}

fn nice_max(v: f32) -> f32 {
    let v = (v * 1.15).max(1024.0);
    let mut step = 1024.0f32;
    loop {
        for m in [1.0, 2.0, 5.0] {
            if step * m >= v {
                return step * m;
            }
        }
        step *= 10.0;
    }
}

fn max_of(series: &[&[f32]]) -> f32 {
    series.iter().flat_map(|s| s.iter()).copied().fold(0.0, f32::max)
}

// ──────────────────────────── отрисовка ────────────────────────────

fn paint_graph(p: &egui::Painter, rect: Rect, series: &[(&[f32], Color32)], max: f32, t: &Theme, grid: bool) {
    let rounding = Rounding::same(6.0);
    p.rect_filled(rect, rounding, t.graph_bg);
    if grid {
        for i in 1..4 {
            let y = rect.top() + rect.height() * i as f32 / 4.0;
            p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0_f32, t.grid));
        }
        for i in 1..6 {
            let x = rect.left() + rect.width() * i as f32 / 6.0;
            p.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0_f32, t.grid));
        }
    }
    let max = max.max(1e-6);
    let inner = rect.shrink(1.0);
    for (data, color) in series {
        let n = data.len();
        if n < 2 {
            continue;
        }
        let step = inner.width() / (HIST.max(n) - 1) as f32;
        let pts: Vec<egui::Pos2> = data
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let x = inner.right() - (n - 1 - i) as f32 * step;
                let y = inner.bottom() - (v / max).clamp(0.0, 1.0) * inner.height();
                pos2(x, y)
            })
            .collect();
        let top_c = with_alpha(*color, 90);
        let bot_c = with_alpha(*color, 12);
        let mut mesh = egui::Mesh::default();
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(a, top_c);
            mesh.colored_vertex(b, top_c);
            mesh.colored_vertex(pos2(b.x, inner.bottom()), bot_c);
            mesh.colored_vertex(pos2(a.x, inner.bottom()), bot_c);
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
        p.add(egui::Shape::mesh(mesh));
        p.add(egui::Shape::line(pts, Stroke::new(1.6_f32, *color)));
    }
    let border = series.first().map(|s| with_alpha(s.1, 120)).unwrap_or(t.card_stroke);
    p.rect_stroke(rect, rounding, Stroke::new(1.0_f32, border));
}

fn graph(ui: &mut Ui, size: egui::Vec2, series: &[(&[f32], Color32)], max: f32, t: &Theme) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    paint_graph(ui.painter(), rect, series, max, t, true);
}

fn avatar(ui: &mut Ui, name: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
    ui.painter().rect_filled(r, Rounding::same(5.0), name_color(name));
    let ch: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    ui.painter().text(r.center(), Align2::CENTER_CENTER, ch, FontId::proportional(11.0), Color32::WHITE);
}

/// Настоящая иконка exe, если уже загружена, иначе цветная буква.
fn proc_icon(ui: &mut Ui, tex: Option<&egui::TextureHandle>, name: &str) {
    match tex {
        Some(t) => {
            let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            ui.painter().image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => avatar(ui, name),
    }
}

type IconResult = (String, Option<(usize, usize, Vec<u8>)>);

/// Логотип: чёрный скруглённый квадрат с белой линией «пульса».
fn logo(ui: &mut Ui, size: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, Rounding::same(size * 0.29), Color32::from_rgb(14, 14, 16));
    p.rect_stroke(r, Rounding::same(size * 0.29), Stroke::new(1.0_f32, Color32::from_white_alpha(40)));
    let k = size / 34.0;
    let pts = [(-11.0, 2.0), (-5.0, 2.0), (-2.0, -7.0), (2.0, 8.0), (5.0, -2.0), (11.0, -2.0)];
    let pts: Vec<_> = pts.iter().map(|(x, y)| r.center() + vec2(*x * k, *y * k)).collect();
    p.add(egui::Shape::line(pts, Stroke::new(2.0_f32, Color32::WHITE)));
}

fn heat_cell(ui: &mut Ui, text: &str, frac: f32, color: Color32, t: &Theme) {
    let f = frac.clamp(0.0, 1.0);
    if f > 0.003 {
        let rect = ui.max_rect().shrink2(vec2(1.0, 1.5));
        let a = match (t.glass, t.dark) {
            (true, true) => 8.0 + f * 70.0,
            (true, false) => 26.0 + f * 130.0,
            _ => 16.0 + f * 120.0,
        };
        ui.painter().rect_filled(rect, Rounding::same(4.0), with_alpha(color, a as u8));
    }
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(6.0);
        ui.label(RichText::new(text).color(t.text));
    });
}

fn right_label(ui: &mut Ui, text: impl Into<String>, color: Color32) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(6.0);
        ui.label(RichText::new(text.into()).color(color));
    });
}

fn sort_header(ui: &mut Ui, t: &Theme, label: &str, total: Option<String>, col: SortCol, sort: &mut (SortCol, bool), right: bool) {
    let active = sort.0 == col;
    let arrow = if active {
        if sort.1 {
            " ⏷"
        } else {
            " ⏶"
        }
    } else {
        ""
    };
    let rect = ui.max_rect();
    let resp = ui.interact(rect, ui.id().with(("hdr", label)), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, Rounding::same(4.0), t.hover);
    }
    let layout = if right { Layout::top_down(Align::Max) } else { Layout::top_down(Align::Min) };
    ui.with_layout(layout, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        if right {
            ui.add_space(2.0);
        }
        match total {
            Some(tot) => {
                ui.label(bold(tot, 15.0).color(t.text));
            }
            None => {
                ui.add_space(17.0);
            }
        }
        ui.horizontal(|ui| {
            if right {
                ui.add_space(6.0);
            }
            ui.label(RichText::new(format!("{label}{arrow}")).size(12.5).color(if active { t.accent } else { t.text_weak }));
        });
    });
    if resp.clicked() {
        if active {
            sort.1 = !sort.1;
        } else {
            *sort = (col, col.default_desc());
        }
    }
}

fn simple_header(ui: &mut Ui, t: &Theme, label: &str, active: bool, desc: bool) -> bool {
    let rect = ui.max_rect();
    let resp = ui.interact(rect, ui.id().with(("shdr", label)), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, Rounding::same(4.0), t.hover);
    }
    let arrow = if active {
        if desc {
            " ⏷"
        } else {
            " ⏶"
        }
    } else {
        ""
    };
    ui.label(RichText::new(format!("{label}{arrow}")).size(12.5).color(if active { t.accent } else { t.text_weak }));
    resp.clicked()
}

fn nav_button(ui: &mut Ui, t: &Theme, icon: &str, label: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
    let p = ui.painter();
    let bg = if selected {
        t.selected
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    p.rect_filled(rect, Rounding::same(9.0), bg);
    if selected {
        p.rect_filled(
            Rect::from_min_size(rect.left_top() + vec2(0.0, 10.0), vec2(3.0, rect.height() - 20.0)),
            Rounding::same(2.0),
            t.accent,
        );
    }
    let color = if selected { t.text } else { t.text_weak };
    p.text(rect.left_center() + vec2(14.0, 0.0), Align2::LEFT_CENTER, icon, FontId::proportional(16.0), color);
    p.text(
        rect.left_center() + vec2(42.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::new(14.5, if selected { FontFamily::Name("bold".into()) } else { FontFamily::Proportional }),
        color,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn mini_meter(ui: &mut Ui, t: &Theme, label: &str, value: &str, frac: f32, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.0).color(t.text_weak));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).size(12.0).color(t.text));
        });
    });
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
    ui.painter().rect_filled(rect, Rounding::same(3.0), t.graph_bg);
    let mut fill = rect;
    fill.set_width(rect.width() * frac.clamp(0.0, 1.0));
    ui.painter().rect_filled(fill, Rounding::same(3.0), color);
}

fn stat(ui: &mut Ui, t: &Theme, label: &str, value: impl Into<String>) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(RichText::new(label).size(12.0).color(t.text_weak));
        ui.label(bold(value, 18.0).color(t.text));
    });
    ui.add_space(22.0);
}

fn legend(ui: &mut Ui, t: &Theme, color: Color32, label: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().rect_filled(r, Rounding::same(3.0), color);
    ui.label(RichText::new(label).size(12.5).color(t.text_weak));
    ui.add_space(10.0);
}

fn proc_menu(ui: &mut Ui, p: &ProcInfo, suspended: bool, in_details: bool, actions: &mut Vec<Action>) {
    ui.set_min_width(240.0);
    if ui.button(format!("✖  {}", tr("End task", "Снять задачу", "Зняти завдання"))).clicked() {
        actions.push(Action::Kill(p.pid));
        ui.close_menu();
    }
    if ui
        .button(format!("🌳  {}", tr("End process tree", "Завершить дерево процессов", "Завершити дерево процесів")))
        .clicked()
    {
        actions.push(Action::KillTree(p.pid));
        ui.close_menu();
    }
    ui.separator();
    if suspended {
        if ui.button(format!("▶  {}", tr("Resume", "Возобновить", "Відновити"))).clicked() {
            actions.push(Action::Resume(p.pid));
            ui.close_menu();
        }
    } else if ui.button(format!("⏸  {}", tr("Suspend", "Приостановить", "Призупинити"))).clicked() {
        actions.push(Action::Suspend(p.pid));
        ui.close_menu();
    }
    ui.menu_button(format!("⚡  {}", tr("Set priority", "Задать приоритет", "Задати пріоритет")), |ui| {
        let cur = win::get_priority(p.pid);
        for cls in win::PRIORITIES {
            if ui.selectable_label(cur == Some(cls), win::priority_name(cls)).clicked() {
                actions.push(Action::Priority(p.pid, cls));
                ui.close_menu();
            }
        }
    });
    ui.separator();
    if ui
        .add_enabled(
            !p.exe.is_empty(),
            Button::new(format!("📂  {}", tr("Open file location", "Открыть расположение файла", "Відкрити розташування файлу"))),
        )
        .clicked()
    {
        actions.push(Action::OpenLocation(p.exe.clone()));
        ui.close_menu();
    }
    if ui.button(format!("🔎  {}", tr("Search online", "Поиск в Интернете", "Пошук в Інтернеті"))).clicked() {
        actions.push(Action::Search(p.name.clone()));
        ui.close_menu();
    }
    if !in_details && ui.button(format!("📋  {}", tr("Go to details", "Подробно", "Докладно"))).clicked() {
        actions.push(Action::Details(p.pid));
        ui.close_menu();
    }
    ui.menu_button(format!("📄  {}", tr("Copy", "Копировать", "Копіювати")), |ui| {
        let items = [
            (tr("Name", "Имя", "Ім'я"), p.name.clone()),
            ("PID", p.pid.to_string()),
            (tr("Path", "Путь", "Шлях"), p.exe.clone()),
            (tr("Command line", "Командную строку", "Командний рядок"), p.cmd.clone()),
        ];
        for (l, v) in items {
            if ui.add_enabled(!v.is_empty(), Button::new(l)).clicked() {
                actions.push(Action::Copy(v));
                ui.close_menu();
            }
        }
    });
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let load = |f: &str| std::fs::read(format!("{windir}\\Fonts\\{f}")).ok();

    if let Some(b) = load("segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), egui::FontData::from_owned(b));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "segoe".into());
    }
    if let Some(b) = load("seguisym.ttf") {
        fonts.font_data.insert("segoe_sym".into(), egui::FontData::from_owned(b));
        fonts.families.entry(FontFamily::Proportional).or_default().push("segoe_sym".into());
        fonts.families.entry(FontFamily::Monospace).or_default().push("segoe_sym".into());
    }
    let mut bold_fam = Vec::new();
    if let Some(b) = load("seguisb.ttf") {
        fonts.font_data.insert("segoe_sb".into(), egui::FontData::from_owned(b));
        bold_fam.push("segoe_sb".to_string());
    }
    bold_fam.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Name("bold".into()), bold_fam);
    ctx.set_fonts(fonts);
}

// ─────────────────────────────── приложение ───────────────────────────────

pub struct TaskApp {
    ctx: egui::Context,
    settings: Settings,
    saved: Settings,
    theme: Theme,
    theme_key: ThemeKey,
    applied_backdrop: Option<(ThemeId, BlurMode, u32, bool)>,
    applied_top: Option<bool>,
    applied_refresh: (u64, bool),
    applied_zoom: f32,
    hwnd: Option<isize>,

    monitor: Monitor,
    snap: Arc<Snapshot>,

    tab: Tab,
    search: String,
    focus_search: bool,
    selection: Sel,
    expanded: HashSet<String>,
    sort: (SortCol, bool),
    detail_sort: (SortCol, bool),
    scroll_to_sel: bool,
    suspended: HashSet<u32>,

    perf_item: PerfItem,

    services: Vec<ServiceInfo>,
    services_at: Option<Instant>,
    services_err: Option<String>,
    svc_sel: Option<String>,
    svc_sort: (u8, bool),

    startup: Vec<StartupItem>,
    startup_loaded: bool,
    startup_sel: Option<usize>,

    status: Option<(String, Instant)>,
    tx: Sender<String>,
    rx: Receiver<String>,

    run_dialog: Option<RunDialog>,
    confirm: Option<Confirm>,

    hook_ours: bool,
    hook_other: Option<String>,
    hook_checked: Option<Instant>,
    is_admin: bool,

    uninstall_confirm: Option<Instant>,

    icons: HashMap<String, egui::TextureHandle>,
    icon_req: HashSet<String>,
    icon_tx: Sender<String>,
    icon_rx: Receiver<IconResult>,
}

impl TaskApp {
    pub fn new(cc: &eframe::CreationContext<'_>, settings: Settings) -> Self {
        setup_fonts(&cc.egui_ctx);
        i18n::set(settings.lang);
        let theme = Theme::build(&settings);
        theme.apply(&cc.egui_ctx);
        cc.egui_ctx.set_zoom_factor(settings.ui_scale);
        let monitor = Monitor::start(cc.egui_ctx.clone(), settings.refresh_ms, settings.paused);
        let (tx, rx) = channel();

        // Фоновый поток извлечения иконок из exe.
        let (icon_tx, req_rx) = channel::<String>();
        let (res_tx, icon_rx) = channel::<IconResult>();
        let ictx = cc.egui_ctx.clone();
        let _ = std::thread::Builder::new().name("icons".into()).spawn(move || {
            win::init_icon_thread();
            while let Ok(path) = req_rx.recv() {
                let img = win::file_icon_rgba(&path);
                if res_tx.send((path, img)).is_err() {
                    break;
                }
                ictx.request_repaint();
            }
        });

        let tab = std::env::args()
            .find_map(|a| a.strip_prefix("--tab=").map(|s| s.to_lowercase()))
            .map(|s| match s.as_str() {
                "performance" | "perf" => Tab::Performance,
                "users" => Tab::Users,
                "details" => Tab::Details,
                "startup" => Tab::Startup,
                "services" => Tab::Services,
                "settings" => Tab::Settings,
                _ => Tab::Processes,
            })
            .unwrap_or(Tab::Processes);
        Self {
            ctx: cc.egui_ctx.clone(),
            theme_key: ThemeKey::of(&settings),
            applied_refresh: (settings.refresh_ms, settings.paused),
            applied_zoom: settings.ui_scale,
            saved: settings.clone(),
            settings,
            theme,
            applied_backdrop: None,
            applied_top: None,
            hwnd: None,
            monitor,
            snap: Arc::new(Snapshot::default()),
            tab,
            search: String::new(),
            focus_search: false,
            selection: Sel::None,
            expanded: HashSet::new(),
            sort: (SortCol::Cpu, true),
            detail_sort: (SortCol::Name, false),
            scroll_to_sel: false,
            suspended: HashSet::new(),
            perf_item: PerfItem::Cpu,
            services: Vec::new(),
            services_at: None,
            services_err: None,
            svc_sel: None,
            svc_sort: (0, false),
            startup: Vec::new(),
            startup_loaded: false,
            startup_sel: None,
            status: None,
            tx,
            rx,
            run_dialog: None,
            confirm: None,
            hook_ours: false,
            hook_other: None,
            hook_checked: None,
            is_admin: win::is_admin(),
            uninstall_confirm: None,
            icons: HashMap::new(),
            icon_req: HashSet::new(),
            icon_tx,
            icon_rx,
        }
    }

    fn request_icon(&mut self, path: &str) {
        if path.is_empty() || self.icon_req.contains(path) {
            return;
        }
        self.icon_req.insert(path.to_string());
        let _ = self.icon_tx.send(path.to_string());
    }

    fn pump_icons(&mut self, ctx: &egui::Context) {
        let snap = self.snap.clone();
        for p in &snap.procs {
            self.request_icon(&p.exe);
        }
        while let Ok((path, img)) = self.icon_rx.try_recv() {
            if let Some((w, h, rgba)) = img {
                let tex = ctx.load_texture(
                    format!("icon:{path}"),
                    egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba),
                    egui::TextureOptions::LINEAR,
                );
                self.icons.insert(path, tex);
            }
        }
    }

    fn msg(&mut self, m: impl Into<String>) {
        self.status = Some((m.into(), Instant::now()));
    }

    fn sync_window(&mut self, ctx: &egui::Context) {
        i18n::set(self.settings.lang);
        let key = ThemeKey::of(&self.settings);
        if key != self.theme_key {
            self.theme = Theme::build(&self.settings);
            self.theme.apply(ctx);
            self.theme_key = key;
        }
        if (self.settings.ui_scale - self.applied_zoom).abs() > 0.001 {
            ctx.set_zoom_factor(self.settings.ui_scale);
            self.applied_zoom = self.settings.ui_scale;
        }
        if self.hwnd.is_none() {
            self.hwnd = win::find_own_window(APP_TITLE);
        }
        if let Some(h) = self.hwnd {
            let bk = (
                self.settings.theme,
                self.settings.blur,
                (self.settings.glass_opacity * 20.0) as u32,
                self.theme.dark,
            );
            if self.applied_backdrop != Some(bk) {
                let mode = if self.theme.glass {
                    match self.settings.blur {
                        BlurMode::Acrylic => Backdrop::Acrylic,
                        BlurMode::Blur => Backdrop::Blur,
                        BlurMode::Clear => Backdrop::Transparent,
                    }
                } else {
                    Backdrop::None
                };
                win::set_backdrop(h, mode, self.theme.acrylic_tint(self.settings.glass_opacity), self.theme.dark);
                self.applied_backdrop = Some(bk);
            }
        }
        if self.applied_top != Some(self.settings.always_on_top) {
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if self.settings.always_on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            }));
            self.applied_top = Some(self.settings.always_on_top);
        }
        let rk = (self.settings.refresh_ms, self.settings.paused);
        if rk != self.applied_refresh {
            self.monitor.set_interval(rk.0);
            self.monitor.set_paused(rk.1);
            self.applied_refresh = rk;
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let typing = ctx.wants_keyboard_input();
        let (del, find, f5, esc) = ctx.input(|i| {
            (
                i.key_pressed(Key::Delete),
                i.modifiers.command && i.key_pressed(Key::F),
                i.key_pressed(Key::F5),
                i.key_pressed(Key::Escape),
            )
        });
        if find {
            self.focus_search = true;
        }
        if f5 {
            self.monitor.refresh_now();
            self.services_at = None;
            self.startup_loaded = false;
        }
        if esc && !self.search.is_empty() && typing {
            self.search.clear();
        }
        if del && !typing && matches!(self.tab, Tab::Processes | Tab::Details | Tab::Users) {
            let pids = self.selected_pids();
            if !pids.is_empty() {
                actions.push(Action::KillMany(pids, tr("selected", "выбранное", "вибране").into()));
            }
        }
    }

    fn selected_pids(&self) -> Vec<u32> {
        match &self.selection {
            Sel::None => vec![],
            Sel::Proc(p) => vec![*p],
            Sel::Group(key) => {
                let (table, k) = key.split_once(':').unwrap_or(("", ""));
                self.snap
                    .procs
                    .iter()
                    .filter(|p| p.pid != 0)
                    .filter(|p| match table {
                        "proc" => p.name.to_lowercase() == k,
                        "user" => p.user.to_lowercase() == k,
                        _ => false,
                    })
                    .map(|p| p.pid)
                    .collect()
            }
        }
    }

    fn search_box(&mut self, ui: &mut Ui, hint: &str) {
        let r = ui.add(TextEdit::singleline(&mut self.search).hint_text(hint).desired_width(230.0).margin(vec2(8.0, 4.0)));
        if self.focus_search {
            r.request_focus();
            self.focus_search = false;
        }
    }

    fn kill_pids(&mut self, pids: &[u32], label: &str) {
        let mut errs = Vec::new();
        for &p in pids {
            if let Err(e) = win::kill(p) {
                errs.push(format!("{p}: {e}"));
            }
            self.suspended.remove(&p);
        }
        if errs.is_empty() {
            self.msg(format!("{}: {label}", tr("Ended", "Завершено", "Завершено")));
        } else {
            let hint = if self.is_admin {
                ""
            } else {
                tr(" (try running as administrator)", " (попробуйте запустить от имени администратора)", " (спробуйте запустити від імені адміністратора)")
            };
            self.msg(format!("{} {}{}", tr("Failed to end", "Не удалось завершить", "Не вдалося завершити"), errs.join("; "), hint));
        }
        self.monitor.refresh_now();
    }

    fn proc_name(&self, pid: u32) -> String {
        self.snap.procs.iter().find(|p| p.pid == pid).map(|p| p.name.clone()).unwrap_or_else(|| pid.to_string())
    }

    fn run_actions(&mut self, ctx: &egui::Context, actions: Vec<Action>) {
        let snap = self.snap.clone();
        for a in actions {
            match a {
                Action::Kill(pid) => {
                    let name = self.proc_name(pid);
                    if is_critical(&name) {
                        self.confirm = Some(Confirm {
                            title: tr("End system process?", "Завершить системный процесс?", "Завершити системний процес?").into(),
                            text: format!(
                                "«{name}» — {}",
                                tr(
                                    "a critical Windows process.\nEnding it may make the system unstable or restart it.",
                                    "критически важный процесс Windows.\nЕго завершение может привести к нестабильной работе или перезагрузке системы.",
                                    "критично важливий процес Windows.\nЙого завершення може призвести до нестабільної роботи або перезавантаження системи."
                                )
                            ),
                            pids: vec![pid],
                            label: name,
                        });
                    } else {
                        self.kill_pids(&[pid], &name);
                    }
                }
                Action::KillMany(pids, label) => {
                    let critical = pids.iter().any(|p| is_critical(&self.proc_name(*p)));
                    let label = if pids.len() == 1 { self.proc_name(pids[0]) } else { label };
                    if critical || pids.len() > 1 {
                        let text = if critical {
                            format!(
                                "«{label}»: {} ({}).",
                                tr(
                                    "contains critical system processes. End anyway",
                                    "среди процессов есть критически важные системные. Всё равно завершить",
                                    "серед процесів є критично важливі системні. Все одно завершити"
                                ),
                                pids.len()
                            )
                        } else {
                            format!(
                                "{} «{label}» ({}).",
                                tr("All processes will be ended:", "Будут завершены все процессы", "Буде завершено всі процеси"),
                                pids.len()
                            )
                        };
                        self.confirm = Some(Confirm {
                            title: tr("End task?", "Снять задачу?", "Зняти завдання?").into(),
                            text,
                            pids,
                            label,
                        });
                    } else {
                        self.kill_pids(&pids, &label);
                    }
                }
                Action::KillTree(pid) => {
                    let pids = descendants(&snap, pid);
                    let name = self.proc_name(pid);
                    self.confirm = Some(Confirm {
                        title: tr("End process tree?", "Завершить дерево процессов?", "Завершити дерево процесів?").into(),
                        text: format!(
                            "«{name}» + {} ({}).\n{}",
                            tr("all its child processes", "все его дочерние процессы", "усі його дочірні процеси"),
                            pids.len(),
                            tr("Unsaved data will be lost.", "Несохранённые данные будут потеряны.", "Незбережені дані буде втрачено.")
                        ),
                        pids,
                        label: name,
                    });
                }
                Action::Suspend(pid) => match win::suspend(pid) {
                    Ok(_) => {
                        self.suspended.insert(pid);
                        let n = self.proc_name(pid);
                        self.msg(format!("{}: {n}", tr("Suspended", "Приостановлен", "Призупинено")));
                    }
                    Err(e) => self.msg(format!("{}: {e}", tr("Failed to suspend", "Не удалось приостановить", "Не вдалося призупинити"))),
                },
                Action::Resume(pid) => match win::resume(pid) {
                    Ok(_) => {
                        self.suspended.remove(&pid);
                        let n = self.proc_name(pid);
                        self.msg(format!("{}: {n}", tr("Resumed", "Возобновлён", "Відновлено")));
                    }
                    Err(e) => self.msg(format!("{}: {e}", tr("Failed to resume", "Не удалось возобновить", "Не вдалося відновити"))),
                },
                Action::Priority(pid, cls) => match win::set_priority(pid, cls) {
                    Ok(_) => {
                        let n = self.proc_name(pid);
                        self.msg(format!("{} «{n}»: {}", tr("Priority", "Приоритет", "Пріоритет"), win::priority_name(cls)));
                    }
                    Err(e) => self.msg(format!(
                        "{}: {e}",
                        tr("Failed to change priority", "Не удалось изменить приоритет", "Не вдалося змінити пріоритет")
                    )),
                },
                Action::OpenLocation(p) => win::open_location(&p),
                Action::Search(q) => win::open_url(&format!("https://www.bing.com/search?q={}", url_encode(&q))),
                Action::Copy(s) => {
                    ctx.output_mut(|o| o.copied_text = s);
                    self.msg(tr("Copied to clipboard", "Скопировано в буфер обмена", "Скопійовано в буфер обміну"));
                }
                Action::Details(pid) => {
                    self.tab = Tab::Details;
                    self.selection = Sel::Proc(pid);
                    self.scroll_to_sel = true;
                }
                Action::Toggle(k) => {
                    if !self.expanded.remove(&k) {
                        self.expanded.insert(k);
                    }
                }
            }
        }
    }

    // ─────────────────────────── каркас ───────────────────────────

    fn nav(&mut self, ui: &mut Ui) {
        let t = self.theme.clone();
        let snap = self.snap.clone();
        card(ui, &t, |ui| {
            ui.set_min_height(ui.available_height());
            ui.horizontal(|ui| {
                logo(ui, 34.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(bold("SwTsM", 17.0).color(t.text));
                    ui.label(RichText::new(tr("Task Manager", "Диспетчер задач", "Диспетчер завдань")).size(12.0).color(t.text_weak));
                });
            });
            ui.add_space(16.0);
            for tab in Tab::MAIN {
                if nav_button(ui, &t, tab.icon(), tab.label(), self.tab == tab).clicked() {
                    self.tab = tab;
                }
                ui.add_space(2.0);
            }
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                if nav_button(ui, &t, Tab::Settings.icon(), Tab::Settings.label(), self.tab == Tab::Settings).clicked() {
                    self.tab = Tab::Settings;
                }
                ui.add_space(10.0);
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.allocate_ui(vec2(ui.available_width(), 90.0), |ui| {
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            mini_meter(ui, &t, tr("CPU", "ЦП", "ЦП"), &format!("{:.0}%", snap.cpu_total), snap.cpu_total / 100.0, t.cpu);
                            let mf = if snap.mem_total > 0 { snap.mem_used as f32 / snap.mem_total as f32 } else { 0.0 };
                            mini_meter(ui, &t, tr("Memory", "Память", "Пам'ять"), &format!("{:.0}%", mf * 100.0), mf, t.mem);
                            let dr = snap.disk_read + snap.disk_write;
                            mini_meter(ui, &t, tr("Disk", "Диск", "Диск"), &fmt_rate(dr), (dr / (50.0 * 1048576.0)) as f32, t.disk);
                        });
                    });
                });
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let t = self.theme.clone();
        let snap = self.snap.clone();
        ui.horizontal(|ui| {
            let small = |s: String| RichText::new(s).size(12.0).color(t.text_weak);
            let mf = if snap.mem_total > 0 { snap.mem_used as f32 / snap.mem_total as f32 * 100.0 } else { 0.0 };
            ui.label(small(format!("{}: {}", tr("Processes", "Процессов", "Процесів"), snap.procs.len())));
            ui.label(small("•".into()));
            ui.label(small(format!("{}: {:.0}%", tr("CPU", "ЦП", "ЦП"), snap.cpu_total)));
            ui.label(small("•".into()));
            ui.label(small(format!("{}: {:.0}%", tr("Memory", "Память", "Пам'ять"), mf)));
            ui.label(small("•".into()));
            ui.label(small(format!("{}: {}", tr("Uptime", "Работает", "Працює"), fmt_duration(snap.uptime))));
            if self.settings.paused {
                ui.label(
                    RichText::new(format!("  ⏸ {}", tr("updates paused", "обновление приостановлено", "оновлення призупинено")))
                        .size(12.0)
                        .color(t.accent),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let recent = self.status.as_ref().filter(|(_, at)| at.elapsed() < Duration::from_secs(6));
                if let Some((m, _)) = recent {
                    ui.label(RichText::new(m).size(12.0).color(t.accent));
                    ui.ctx().request_repaint_after(Duration::from_millis(500));
                } else {
                    let role = if self.is_admin {
                        format!("🛡 {}", tr("Administrator", "Администратор", "Адміністратор"))
                    } else {
                        tr("Standard user", "Обычные права", "Звичайні права").to_string()
                    };
                    ui.label(small(role));
                }
            });
        });
    }

    // ─────────────────────── таблица процессов ───────────────────────

    #[allow(clippy::too_many_arguments)]
    fn grouped_table(
        &mut self,
        ui: &mut Ui,
        id: &str,
        snap: &Snapshot,
        mut groups: Vec<Group>,
        force_expander: bool,
        show_user: bool,
        actions: &mut Vec<Action>,
    ) {
        let t = self.theme.clone();
        let procs = &snap.procs;
        let (col, desc) = self.sort;
        for g in groups.iter_mut() {
            g.items.sort_by(|a, b| ord(cmp_proc(&procs[*a], &procs[*b], col), desc));
        }
        groups.sort_by(|a, b| ord(cmp_group(a, b, col, procs), desc));

        let searching = !self.search.is_empty();
        let mut rows = Vec::new();
        for (gi, g) in groups.iter().enumerate() {
            if g.items.len() == 1 && !force_expander {
                rows.push(Row::Proc(g.items[0], false));
                continue;
            }
            rows.push(Row::Group(gi));
            let fk = format!("{id}:{}", g.key);
            if self.expanded.contains(&fk) || (searching && !force_expander) {
                for &i in &g.items {
                    rows.push(Row::Proc(i, true));
                }
            }
        }

        let total_mem = snap.mem_total.max(1) as f32;
        let mem_pct = snap.mem_used as f32 / total_mem * 100.0;
        let disk_total = snap.disk_read + snap.disk_write;
        let mut sort = self.sort;
        let mut click: Option<Sel> = None;
        let susp_short = tr("susp.", "приост.", "призуп.");
        let susp_long = tr("Suspended", "Приостановлен", "Призупинено");

        ui.push_id(id, |ui| {
            let mut tb = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .sense(Sense::click())
                .auto_shrink(false)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::initial(300.0).at_least(150.0).clip(true))
                .column(Column::initial(120.0).at_least(60.0).clip(true))
                .column(Column::initial(80.0).at_least(60.0))
                .column(Column::initial(105.0).at_least(70.0))
                .column(Column::initial(100.0).at_least(70.0));
            if show_user {
                tb = tb.column(Column::remainder().at_least(80.0).clip(true));
            }
            tb.header(44.0, |mut h| {
                h.col(|ui| sort_header(ui, &t, tr("Name", "Имя", "Ім'я"), None, SortCol::Name, &mut sort, false));
                h.col(|ui| sort_header(ui, &t, tr("Status", "Состояние", "Стан"), None, SortCol::Status, &mut sort, false));
                h.col(|ui| sort_header(ui, &t, tr("CPU", "ЦП", "ЦП"), Some(format!("{:.0}%", snap.cpu_total)), SortCol::Cpu, &mut sort, true));
                h.col(|ui| sort_header(ui, &t, tr("Memory", "Память", "Пам'ять"), Some(format!("{:.0}%", mem_pct)), SortCol::Mem, &mut sort, true));
                h.col(|ui| sort_header(ui, &t, tr("Disk", "Диск", "Диск"), Some(fmt_rate(disk_total)), SortCol::Disk, &mut sort, true));
                if show_user {
                    h.col(|ui| sort_header(ui, &t, tr("User", "Пользователь", "Користувач"), None, SortCol::User, &mut sort, false));
                }
            })
            .body(|body| {
                body.rows(28.0, rows.len(), |mut row| match &rows[row.index()] {
                    Row::Group(gi) => {
                        let g = &groups[*gi];
                        let fk = format!("{id}:{}", g.key);
                        let expanded = self.expanded.contains(&fk) || (searching && !force_expander);
                        row.set_selected(self.selection == Sel::Group(fk.clone()));
                        let n_susp = g.items.iter().filter(|i| self.suspended.contains(&procs[**i].pid)).count();
                        row.col(|ui| {
                            let arrow = if expanded { "⏷" } else { "⏵" };
                            if ui.add(egui::Label::new(RichText::new(arrow).color(t.text_weak)).sense(Sense::click())).clicked() {
                                actions.push(Action::Toggle(fk.clone()));
                            }
                            if force_expander {
                                avatar(ui, &g.label);
                            } else {
                                let gexe = g.items.first().map(|i| procs[*i].exe.as_str()).unwrap_or("");
                                proc_icon(ui, self.icons.get(gexe), &g.label);
                            }
                            ui.label(RichText::new(&g.label).color(t.text));
                            ui.label(RichText::new(format!("({})", g.items.len())).color(t.text_weak));
                        });
                        row.col(|ui| {
                            if n_susp > 0 {
                                ui.label(RichText::new(format!("⏸ {n_susp} {susp_short}")).color(t.text_weak));
                            }
                        });
                        row.col(|ui| heat_cell(ui, &format!("{:.1}%", g.cpu), g.cpu / 40.0, t.cpu, &t));
                        row.col(|ui| heat_cell(ui, &fmt_bytes(g.mem as f64), g.mem as f32 / total_mem * 6.0, t.mem, &t));
                        row.col(|ui| heat_cell(ui, &fmt_rate(g.disk), (g.disk / 10_485_760.0) as f32, t.disk, &t));
                        if show_user {
                            let u = g.items.first().map(|i| procs[*i].user.clone()).unwrap_or_default();
                            row.col(|ui| {
                                ui.label(RichText::new(u).color(t.text_weak));
                            });
                        }
                        let resp = row.response();
                        if resp.clicked() {
                            click = Some(Sel::Group(fk.clone()));
                        }
                        if resp.double_clicked() {
                            actions.push(Action::Toggle(fk.clone()));
                        }
                        resp.context_menu(|ui| {
                            ui.set_min_width(240.0);
                            if ui
                                .button(format!("✖  {} ({})", tr("End task", "Снять задачу", "Зняти завдання"), g.items.len()))
                                .clicked()
                            {
                                let pids = g.items.iter().map(|i| procs[*i].pid).collect();
                                actions.push(Action::KillMany(pids, g.label.clone()));
                                ui.close_menu();
                            }
                            let tl = if expanded {
                                format!("⏶  {}", tr("Collapse", "Свернуть", "Згорнути"))
                            } else {
                                format!("⏷  {}", tr("Expand", "Развернуть", "Розгорнути"))
                            };
                            if ui.button(tl).clicked() {
                                actions.push(Action::Toggle(fk.clone()));
                                ui.close_menu();
                            }
                            if let Some(p) = g.items.first().map(|i| &procs[*i]) {
                                if !force_expander {
                                    ui.separator();
                                    if ui
                                        .add_enabled(
                                            !p.exe.is_empty(),
                                            Button::new(format!(
                                                "📂  {}",
                                                tr("Open file location", "Открыть расположение файла", "Відкрити розташування файлу")
                                            )),
                                        )
                                        .clicked()
                                    {
                                        actions.push(Action::OpenLocation(p.exe.clone()));
                                        ui.close_menu();
                                    }
                                    if ui.button(format!("🔎  {}", tr("Search online", "Поиск в Интернете", "Пошук в Інтернеті"))).clicked() {
                                        actions.push(Action::Search(p.name.clone()));
                                        ui.close_menu();
                                    }
                                }
                            }
                        });
                    }
                    Row::Proc(i, child) => {
                        let p = &procs[*i];
                        let susp = self.suspended.contains(&p.pid);
                        row.set_selected(self.selection == Sel::Proc(p.pid));
                        row.col(|ui| {
                            ui.add_space(if *child { 34.0 } else { 18.0 });
                            proc_icon(ui, self.icons.get(&p.exe), &p.name);
                            ui.label(RichText::new(&p.name).color(t.text));
                            if *child || force_expander {
                                ui.label(RichText::new(p.pid.to_string()).size(11.5).color(t.text_weak));
                            }
                        });
                        row.col(|ui| {
                            if susp {
                                ui.label(RichText::new(format!("⏸ {susp_long}")).color(t.text_weak));
                            }
                        });
                        row.col(|ui| heat_cell(ui, &format!("{:.1}%", p.cpu), p.cpu / 40.0, t.cpu, &t));
                        row.col(|ui| heat_cell(ui, &fmt_bytes(p.mem as f64), p.mem as f32 / total_mem * 6.0, t.mem, &t));
                        row.col(|ui| heat_cell(ui, &fmt_rate(p.read + p.write), ((p.read + p.write) / 10_485_760.0) as f32, t.disk, &t));
                        if show_user {
                            row.col(|ui| {
                                ui.label(RichText::new(&p.user).color(t.text_weak));
                            });
                        }
                        let resp = row.response();
                        if resp.clicked() {
                            click = Some(Sel::Proc(p.pid));
                        }
                        resp.context_menu(|ui| proc_menu(ui, p, susp, false, actions));
                    }
                });
            });
        });
        self.sort = sort;
        if let Some(c) = click {
            self.selection = c;
        }
    }

    // ─────────────────────────── вкладки ───────────────────────────

    fn run_task_button(&mut self, ui: &mut Ui) {
        if ui.button(format!("➕  {}", tr("Run new task", "Запустить задачу", "Запустити завдання"))).clicked() {
            self.run_dialog = Some(RunDialog { text: String::new(), admin: false, first: true });
        }
    }

    fn end_task_button(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let pids = self.selected_pids();
        if ui
            .add_enabled(!pids.is_empty(), Button::new(format!("✖  {}", tr("End task", "Снять задачу", "Зняти завдання"))))
            .clicked()
        {
            actions.push(Action::KillMany(pids, tr("selected", "выбранное", "вибране").into()));
        }
    }

    fn processes_tab(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let snap = self.snap.clone();
        let filter = self.search.to_lowercase();
        let mut map: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, p) in snap.procs.iter().enumerate() {
            if p.pid == 0 {
                continue;
            }
            if !filter.is_empty() && !p.name.to_lowercase().contains(&filter) && !p.pid.to_string().contains(&filter) {
                continue;
            }
            let key = if self.settings.group_by_name { p.name.to_lowercase() } else { format!("#{}", p.pid) };
            map.entry(key).or_default().push(i);
        }
        let groups: Vec<Group> = map
            .into_iter()
            .map(|(k, items)| {
                let label = snap.procs[items[0]].name.clone();
                make_group(k, label, items, &snap.procs)
            })
            .collect();

        ui.horizontal(|ui| {
            ui.label(bold(Tab::Processes.label(), 22.0));
            ui.label(RichText::new(format!("{}", snap.procs.len())).color(self.theme.text_weak));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.end_task_button(ui, actions);
                self.run_task_button(ui);
                ui.checkbox(&mut self.settings.group_by_name, tr("Group", "Группировать", "Групувати"));
                self.search_box(ui, &format!("🔎  {}", tr("Search (Ctrl+F)", "Поиск (Ctrl+F)", "Пошук (Ctrl+F)")));
            });
        });
        ui.add_space(8.0);
        self.grouped_table(ui, "proc", &snap, groups, false, true, actions);
    }

    fn users_tab(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let snap = self.snap.clone();
        let filter = self.search.to_lowercase();
        let mut map: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, p) in snap.procs.iter().enumerate() {
            if p.pid == 0 {
                continue;
            }
            if !filter.is_empty() && !p.name.to_lowercase().contains(&filter) && !p.user.to_lowercase().contains(&filter) {
                continue;
            }
            map.entry(p.user.to_lowercase()).or_default().push(i);
        }
        let groups: Vec<Group> = map
            .into_iter()
            .map(|(k, items)| {
                let u = snap.procs[items[0]].user.clone();
                let label = if u.is_empty() {
                    tr("Access denied / unknown", "Нет доступа / неизвестно", "Немає доступу / невідомо").to_string()
                } else {
                    u
                };
                make_group(k, label, items, &snap.procs)
            })
            .collect();
        ui.horizontal(|ui| {
            ui.label(bold(Tab::Users.label(), 22.0));
            ui.label(RichText::new(format!("{}", groups.len())).color(self.theme.text_weak));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.search_box(ui, &format!("🔎  {}", tr("User or process", "Пользователь или процесс", "Користувач або процес")));
            });
        });
        if !self.is_admin {
            ui.label(
                RichText::new(tr(
                    "Without administrator rights, owners of system processes can't be determined.",
                    "Без прав администратора владельцы системных процессов не определяются.",
                    "Без прав адміністратора власники системних процесів не визначаються.",
                ))
                .size(12.0)
                .color(self.theme.text_weak),
            );
        }
        ui.add_space(8.0);
        self.grouped_table(ui, "user", &snap, groups, true, false, actions);
    }

    fn details_tab(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let t = self.theme.clone();
        let snap = self.snap.clone();
        let procs = &snap.procs;
        let filter = self.search.to_lowercase();
        let mut list: Vec<usize> = (0..procs.len())
            .filter(|&i| {
                let p = &procs[i];
                filter.is_empty()
                    || p.name.to_lowercase().contains(&filter)
                    || p.pid.to_string().contains(&filter)
                    || p.user.to_lowercase().contains(&filter)
                    || p.exe.to_lowercase().contains(&filter)
            })
            .collect();
        let (col, desc) = self.detail_sort;
        list.sort_by(|a, b| ord(cmp_proc(&procs[*a], &procs[*b], col), desc));

        ui.horizontal(|ui| {
            ui.label(bold(Tab::Details.label(), 22.0));
            ui.label(RichText::new(format!("{}", list.len())).color(t.text_weak));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.end_task_button(ui, actions);
                self.run_task_button(ui);
                self.search_box(ui, &format!("🔎  {}", tr("Name, PID, user, path", "Имя, PID, пользователь, путь", "Ім'я, PID, користувач, шлях")));
            });
        });
        ui.add_space(8.0);

        let total_mem = snap.mem_total.max(1) as f32;
        let mut sort = self.detail_sort;
        let mut click = None;
        let scroll_row = if self.scroll_to_sel {
            self.scroll_to_sel = false;
            match self.selection {
                Sel::Proc(pid) => list.iter().position(|i| procs[*i].pid == pid),
                _ => None,
            }
        } else {
            None
        };

        let cols: [(&str, SortCol, f32); 13] = [
            (tr("Name", "Имя", "Ім'я"), SortCol::Name, 220.0),
            (tr("PID", "ИД процесса", "ІД процесу"), SortCol::Pid, 90.0),
            (tr("Parent", "Родитель", "Батьківський"), SortCol::Parent, 90.0),
            (tr("Status", "Состояние", "Стан"), SortCol::Status, 110.0),
            (tr("User", "Пользователь", "Користувач"), SortCol::User, 120.0),
            (tr("CPU", "ЦП", "ЦП"), SortCol::Cpu, 60.0),
            (tr("Memory", "Память", "Пам'ять"), SortCol::Mem, 95.0),
            (tr("Virtual memory", "Вирт. память", "Вірт. пам'ять"), SortCol::VMem, 110.0),
            (tr("Read", "Чтение", "Читання"), SortCol::Read, 90.0),
            (tr("Write", "Запись", "Запис"), SortCol::Write, 90.0),
            (tr("Uptime", "Время работы", "Час роботи"), SortCol::Runtime, 100.0),
            (tr("Path", "Путь", "Шлях"), SortCol::Path, 300.0),
            (tr("Command line", "Командная строка", "Командний рядок"), SortCol::Path, 500.0),
        ];
        let running = tr("Running", "Выполняется", "Виконується");
        let suspended_s = tr("Suspended", "Приостановлен", "Призупинено");

        ui.push_id("details", |ui| {
            ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
                let mut tb = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .sense(Sense::click())
                    .auto_shrink(false)
                    .cell_layout(Layout::left_to_right(Align::Center));
                for (_, _, w) in cols.iter() {
                    tb = tb.column(Column::initial(*w).at_least(40.0).clip(true));
                }
                if let Some(r) = scroll_row {
                    tb = tb.scroll_to_row(r, Some(Align::Center));
                }
                tb.header(30.0, |mut h| {
                    for (i, (label, c, _)) in cols.iter().enumerate() {
                        h.col(|ui| {
                            if i == 12 {
                                ui.label(RichText::new(*label).size(12.5).color(t.text_weak));
                                return;
                            }
                            if simple_header(ui, &t, label, sort.0 == *c, sort.1) {
                                if sort.0 == *c {
                                    sort.1 = !sort.1;
                                } else {
                                    sort = (*c, c.default_desc());
                                }
                            }
                        });
                    }
                })
                .body(|body| {
                    body.rows(26.0, list.len(), |mut row| {
                        let p = &procs[list[row.index()]];
                        let susp = self.suspended.contains(&p.pid);
                        row.set_selected(self.selection == Sel::Proc(p.pid));
                        let weak = t.text_weak;
                        row.col(|ui| {
                            proc_icon(ui, self.icons.get(&p.exe), &p.name);
                            ui.label(RichText::new(&p.name).color(t.text));
                        });
                        row.col(|ui| right_label(ui, p.pid.to_string(), t.text));
                        row.col(|ui| right_label(ui, p.parent.map(|x| x.to_string()).unwrap_or_default(), weak));
                        row.col(|ui| {
                            ui.label(RichText::new(if susp { suspended_s } else { running }).color(weak));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(&p.user).color(weak));
                        });
                        row.col(|ui| heat_cell(ui, &format!("{:.0}", p.cpu), p.cpu / 40.0, t.cpu, &t));
                        row.col(|ui| heat_cell(ui, &fmt_bytes(p.mem as f64), p.mem as f32 / total_mem * 6.0, t.mem, &t));
                        row.col(|ui| right_label(ui, fmt_bytes(p.vmem as f64), weak));
                        row.col(|ui| right_label(ui, fmt_rate(p.read), weak));
                        row.col(|ui| right_label(ui, fmt_rate(p.write), weak));
                        row.col(|ui| right_label(ui, fmt_duration(p.run_time), weak));
                        row.col(|ui| {
                            ui.label(RichText::new(&p.exe).color(weak));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(&p.cmd).color(weak));
                        });
                        let resp = row.response();
                        if resp.clicked() {
                            click = Some(p.pid);
                        }
                        resp.context_menu(|ui| proc_menu(ui, p, susp, true, actions));
                    });
                });
            });
        });
        self.detail_sort = sort;
        if let Some(pid) = click {
            self.selection = Sel::Proc(pid);
        }
    }

    fn perf_item_widget(&mut self, ui: &mut Ui, item: PerfItem, title: &str, sub: &str, series: &[(&[f32], Color32)], max: f32) {
        let t = self.theme.clone();
        let selected = self.perf_item == item;
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 66.0), Sense::click());
        let p = ui.painter_at(rect);
        let bg = if selected {
            t.selected
        } else if resp.hovered() {
            t.hover
        } else {
            Color32::TRANSPARENT
        };
        p.rect_filled(rect, Rounding::same(10.0), bg);
        let g = Rect::from_min_size(rect.min + vec2(10.0, 11.0), vec2(72.0, 44.0));
        paint_graph(&p, g, series, max, &t, false);
        p.text(pos2(g.right() + 12.0, rect.top() + 13.0), Align2::LEFT_TOP, title, FontId::new(15.0, FontFamily::Name("bold".into())), t.text);
        p.text(pos2(g.right() + 12.0, rect.top() + 36.0), Align2::LEFT_TOP, sub, FontId::proportional(12.5), t.text_weak);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.perf_item = item;
        }
    }

    fn performance_tab(&mut self, ui: &mut Ui) {
        let t = self.theme.clone();
        let snap = self.snap.clone();
        ui.label(bold(Tab::Performance.label(), 22.0));
        ui.add_space(8.0);

        if let PerfItem::Net(n) = &self.perf_item {
            if !snap.nets.iter().any(|x| &x.name == n) && snap.generation > 0 {
                self.perf_item = PerfItem::Cpu;
            }
        }
        let ghz = tr("GHz", "ГГц", "ГГц");
        let gb = tr("GB", "ГБ", "ГБ");
        let (r_l, w_l) = (tr("R", "Ч", "Ч"), tr("W", "З", "З"));
        let (s_l, rx_l) = (tr("S", "О", "Н"), tr("R", "П", "О"));

        let h = ui.available_height();
        let mem_frac = if snap.mem_total > 0 { snap.mem_used as f32 / snap.mem_total as f32 } else { 0.0 };
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(272.0, h), Layout::top_down(Align::Min), |ui| {
                ui.push_id("perf_list", |ui| {
                    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                        self.perf_item_widget(
                            ui,
                            PerfItem::Cpu,
                            tr("CPU", "ЦП", "ЦП"),
                            &format!("{:.0}%  {:.2} {ghz}", snap.cpu_total, snap.cpu_freq as f32 / 1000.0),
                            &[(&snap.cpu_hist, t.cpu)],
                            100.0,
                        );
                        self.perf_item_widget(
                            ui,
                            PerfItem::Memory,
                            tr("Memory", "Память", "Пам'ять"),
                            &format!(
                                "{:.1}/{:.1} {gb} ({:.0}%)",
                                snap.mem_used as f64 / 1073741824.0,
                                snap.mem_total as f64 / 1073741824.0,
                                mem_frac * 100.0
                            ),
                            &[(&snap.mem_hist, t.mem)],
                            100.0,
                        );
                        let dmax = nice_max(max_of(&[&snap.disk_read_hist, &snap.disk_write_hist]));
                        self.perf_item_widget(
                            ui,
                            PerfItem::Disk,
                            tr("Disk", "Диск", "Диск"),
                            &format!("{r_l}: {}  {w_l}: {}", fmt_rate(snap.disk_read), fmt_rate(snap.disk_write)),
                            &[(&snap.disk_read_hist, t.disk), (&snap.disk_write_hist, t.accent)],
                            dmax,
                        );
                        for n in &snap.nets {
                            let nmax = nice_max(max_of(&[&n.hist_rx, &n.hist_tx]));
                            self.perf_item_widget(
                                ui,
                                PerfItem::Net(n.name.clone()),
                                &n.name,
                                &format!("{s_l}: {}  {rx_l}: {}", fmt_bits(n.tx), fmt_bits(n.rx)),
                                &[(&n.hist_rx, t.net), (&n.hist_tx, t.accent)],
                                nmax,
                            );
                        }
                    });
                });
            });
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.push_id("perf_detail", |ui| {
                    ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                        let item = self.perf_item.clone();
                        match item {
                            PerfItem::Cpu => self.perf_cpu(ui, &snap, &t),
                            PerfItem::Memory => self.perf_mem(ui, &snap, &t),
                            PerfItem::Disk => self.perf_disk(ui, &snap, &t),
                            PerfItem::Net(n) => self.perf_net(ui, &snap, &t, &n),
                        }
                    });
                });
            });
        });
    }

    fn detail_title(ui: &mut Ui, t: &Theme, title: &str, right: &str) {
        ui.horizontal(|ui| {
            ui.label(bold(title, 26.0).color(t.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(right).size(14.0).color(t.text_weak));
            });
        });
    }

    fn axis_row(ui: &mut Ui, t: &Theme, left: &str, right: &str) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(left).size(12.0).color(t.text_weak));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(right).size(12.0).color(t.text_weak));
            });
        });
    }

    fn graph_height(ui: &Ui) -> f32 {
        (ui.available_height() * 0.5).clamp(160.0, 320.0)
    }

    fn seconds_label() -> String {
        format!("{HIST} {}", tr("seconds", "секунд", "секунд"))
    }

    fn perf_cpu(&mut self, ui: &mut Ui, snap: &Snapshot, t: &Theme) {
        Self::detail_title(ui, t, tr("CPU", "ЦП", "ЦП"), &snap.cpu_brand);
        Self::axis_row(ui, t, tr("% utilization", "% использования", "% використання"), "100%");
        let gh = Self::graph_height(ui);
        let w = ui.available_width();
        if self.settings.per_core && !snap.core_hist.is_empty() {
            let n = snap.core_hist.len();
            let cols = (n as f32).sqrt().ceil() as usize;
            let rows = n.div_ceil(cols);
            let gap = 6.0;
            let cw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
            let ch = (gh - gap * (rows as f32 - 1.0)) / rows as f32;
            let (rect, _) = ui.allocate_exact_size(vec2(w, gh), Sense::hover());
            let p = ui.painter();
            for i in 0..n {
                let (r, c) = (i / cols, i % cols);
                let cell = Rect::from_min_size(rect.min + vec2(c as f32 * (cw + gap), r as f32 * (ch + gap)), vec2(cw, ch));
                paint_graph(p, cell, &[(&snap.core_hist[i], t.cpu)], 100.0, t, false);
                p.text(
                    cell.left_top() + vec2(6.0, 4.0),
                    Align2::LEFT_TOP,
                    format!("{} · {:.0}%", i, snap.cores.get(i).copied().unwrap_or(0.0)),
                    FontId::proportional(11.0),
                    t.text_weak,
                );
            }
        } else {
            graph(ui, vec2(w, gh), &[(&snap.cpu_hist, t.cpu)], 100.0, t);
        }
        Self::axis_row(ui, t, &Self::seconds_label(), "0");
        ui.checkbox(&mut self.settings.per_core, tr("Show logical processors", "Показать логические процессоры", "Показати логічні процесори"));
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            stat(ui, t, tr("Utilization", "Использование", "Використання"), format!("{:.0}%", snap.cpu_total));
            stat(ui, t, tr("Speed", "Скорость", "Швидкість"), format!("{:.2} {}", snap.cpu_freq as f32 / 1000.0, tr("GHz", "ГГц", "ГГц")));
            stat(ui, t, tr("Processes", "Процессы", "Процеси"), snap.procs.len().to_string());
            stat(ui, t, tr("Uptime", "Время работы", "Час роботи"), fmt_duration(snap.uptime));
            stat(ui, t, tr("Logical processors", "Логических процессоров", "Логічних процесорів"), snap.cores.len().to_string());
        });
        ui.add_space(6.0);
        ui.label(RichText::new(format!("{} • {}", snap.host, snap.os)).size(12.0).color(t.text_weak));
    }

    fn perf_mem(&mut self, ui: &mut Ui, snap: &Snapshot, t: &Theme) {
        let gbf = |b: u64| b as f64 / 1073741824.0;
        let gb = tr("GB", "ГБ", "ГБ");
        Self::detail_title(ui, t, tr("Memory", "Память", "Пам'ять"), &format!("{:.1} {gb}", gbf(snap.mem_total)));
        Self::axis_row(ui, t, tr("Memory usage", "Использование памяти", "Використання пам'яті"), "100%");
        let gh = Self::graph_height(ui);
        graph(ui, vec2(ui.available_width(), gh), &[(&snap.mem_hist, t.mem)], 100.0, t);
        Self::axis_row(ui, t, &Self::seconds_label(), "0");
        ui.add_space(6.0);
        ui.label(RichText::new(tr("Memory composition", "Структура памяти", "Структура пам'яті")).size(12.0).color(t.text_weak));
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, Rounding::same(6.0), t.graph_bg);
        let frac = if snap.mem_total > 0 { snap.mem_used as f32 / snap.mem_total as f32 } else { 0.0 };
        let mut used = rect;
        used.set_width(rect.width() * frac);
        p.rect_filled(used, Rounding::same(6.0), with_alpha(t.mem, 150));
        p.rect_stroke(rect, Rounding::same(6.0), Stroke::new(1.0_f32, with_alpha(t.mem, 140)));
        let in_use = tr("In use", "Используется", "Використовується");
        let avail = tr("Available", "Доступно", "Доступно");
        p.text(
            used.left_center() + vec2(10.0, 0.0),
            Align2::LEFT_CENTER,
            format!("{in_use} {:.1} {gb}", gbf(snap.mem_used)),
            FontId::proportional(12.5),
            t.text,
        );
        p.text(
            rect.right_center() - vec2(10.0, 0.0),
            Align2::RIGHT_CENTER,
            format!("{avail} {:.1} {gb}", gbf(snap.mem_avail)),
            FontId::proportional(12.5),
            t.text_weak,
        );
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            stat(ui, t, in_use, format!("{:.1} {gb}", gbf(snap.mem_used)));
            stat(ui, t, avail, format!("{:.1} {gb}", gbf(snap.mem_avail)));
            stat(ui, t, tr("Total", "Всего", "Усього"), format!("{:.1} {gb}", gbf(snap.mem_total)));
            stat(ui, t, tr("Load", "Загрузка", "Завантаження"), format!("{:.0}%", frac * 100.0));
            stat(
                ui,
                t,
                tr("Page file", "Файл подкачки", "Файл підкачки"),
                format!("{:.1} / {:.1} {gb}", gbf(snap.swap_used), gbf(snap.swap_total)),
            );
        });
    }

    fn perf_disk(&mut self, ui: &mut Ui, snap: &Snapshot, t: &Theme) {
        let max = nice_max(max_of(&[&snap.disk_read_hist, &snap.disk_write_hist]));
        let read = tr("Read", "Чтение", "Читання");
        let write = tr("Write", "Запись", "Запис");
        Self::detail_title(ui, t, tr("Disk", "Диск", "Диск"), tr("total process I/O", "суммарный ввод/вывод процессов", "сумарне введення/виведення процесів"));
        Self::axis_row(ui, t, tr("Transfer rate", "Скорость передачи", "Швидкість передачі"), &fmt_rate(max as f64));
        let gh = Self::graph_height(ui);
        graph(ui, vec2(ui.available_width(), gh), &[(&snap.disk_read_hist, t.disk), (&snap.disk_write_hist, t.accent)], max, t);
        ui.horizontal(|ui| {
            legend(ui, t, t.disk, read);
            legend(ui, t, t.accent, write);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new("0").size(12.0).color(t.text_weak));
            });
        });
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            stat(ui, t, read, fmt_rate(snap.disk_read));
            stat(ui, t, write, fmt_rate(snap.disk_write));
        });
        ui.add_space(10.0);
        ui.label(bold(tr("Volumes", "Тома", "Томи"), 16.0).color(t.text));
        ui.add_space(4.0);
        for d in &snap.disks {
            let used = d.total.saturating_sub(d.avail);
            let frac = if d.total > 0 { used as f32 / d.total as f32 } else { 0.0 };
            ui.horizontal(|ui| {
                ui.label(bold(&d.mount, 14.0).color(t.text));
                let mut info = vec![];
                for s in [&d.name, &d.fs, &d.kind] {
                    if !s.is_empty() {
                        info.push(s.clone());
                    }
                }
                if d.removable {
                    info.push(tr("removable", "съёмный", "знімний").into());
                }
                ui.label(RichText::new(info.join(" • ")).size(12.0).color(t.text_weak));
            });
            ui.add(
                egui::ProgressBar::new(frac).fill(if frac > 0.9 { t.danger } else { with_alpha(t.disk, 200) }).text(
                    RichText::new(format!(
                        "{} {} {}",
                        fmt_bytes(d.avail as f64),
                        tr("free of", "свободно из", "вільно з"),
                        fmt_bytes(d.total as f64)
                    ))
                    .color(t.text),
                ),
            );
            ui.add_space(6.0);
        }
    }

    fn perf_net(&mut self, ui: &mut Ui, snap: &Snapshot, t: &Theme, name: &str) {
        let Some(n) = snap.nets.iter().find(|x| x.name == name) else {
            ui.label(tr("Adapter not found", "Адаптер не найден", "Адаптер не знайдено"));
            return;
        };
        let recv = tr("Receive", "Получение", "Отримання");
        let send = tr("Send", "Отправка", "Надсилання");
        let max = nice_max(max_of(&[&n.hist_rx, &n.hist_tx]));
        Self::detail_title(ui, t, tr("Network", "Сеть", "Мережа"), &n.name);
        Self::axis_row(ui, t, tr("Throughput", "Пропускная способность", "Пропускна здатність"), &fmt_bits(max as f64));
        let gh = Self::graph_height(ui);
        graph(ui, vec2(ui.available_width(), gh), &[(&n.hist_rx, t.net), (&n.hist_tx, t.accent)], max, t);
        ui.horizontal(|ui| {
            legend(ui, t, t.net, recv);
            legend(ui, t, t.accent, send);
        });
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            stat(ui, t, send, fmt_bits(n.tx));
            stat(ui, t, recv, fmt_bits(n.rx));
            stat(ui, t, tr("Total sent", "Всего отправлено", "Усього надіслано"), fmt_bytes(n.total_tx as f64));
            stat(ui, t, tr("Total received", "Всего получено", "Усього отримано"), fmt_bytes(n.total_rx as f64));
        });
    }

    fn startup_tab(&mut self, ui: &mut Ui) {
        let t = self.theme.clone();
        if !self.startup_loaded {
            self.startup = win::list_startup();
            self.startup_loaded = true;
            self.startup_sel = None;
            let paths: Vec<String> = self.startup.iter().map(|s| exe_from_command(&s.command)).collect();
            for p in paths {
                self.request_icon(&p);
            }
        }
        let enable_s = tr("Enable", "Включить", "Увімкнути");
        let disable_s = tr("Disable", "Отключить", "Вимкнути");
        let mut toggle: Option<(usize, bool)> = None;
        let mut open: Option<String> = None;
        let mut copy: Option<String> = None;
        ui.horizontal(|ui| {
            ui.label(bold(Tab::Startup.label(), 22.0));
            let enabled = self.startup.iter().filter(|s| s.enabled).count();
            ui.label(
                RichText::new(format!("{} {} {} {}", enabled, tr("of", "из", "з"), self.startup.len(), tr("enabled", "включено", "увімкнено")))
                    .color(t.text_weak),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let sel = self.startup_sel.and_then(|i| self.startup.get(i).map(|s| (i, s.enabled)));
                match sel {
                    Some((i, true)) => {
                        if ui.button(format!("⏹  {disable_s}")).clicked() {
                            toggle = Some((i, false));
                        }
                    }
                    Some((i, false)) => {
                        if ui.button(format!("▶  {enable_s}")).clicked() {
                            toggle = Some((i, true));
                        }
                    }
                    None => {
                        ui.add_enabled(false, Button::new(disable_s));
                    }
                }
                if ui.button(format!("⟳  {}", tr("Refresh", "Обновить", "Оновити"))).clicked() {
                    self.startup_loaded = false;
                }
            });
        });
        ui.add_space(8.0);
        let mut click = None;
        let heads = [
            tr("Name", "Имя", "Ім'я"),
            tr("Status", "Состояние", "Стан"),
            tr("Source", "Источник", "Джерело"),
            tr("Command", "Команда", "Команда"),
        ];
        let (on_s, off_s) = (tr("Enabled", "Включено", "Увімкнено"), tr("Disabled", "Отключено", "Вимкнено"));
        ui.push_id("startup", |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .sense(Sense::click())
                .auto_shrink(false)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::initial(250.0).at_least(100.0).clip(true))
                .column(Column::initial(110.0).at_least(80.0))
                .column(Column::initial(200.0).at_least(80.0).clip(true))
                .column(Column::remainder().at_least(100.0).clip(true))
                .header(30.0, |mut h| {
                    for l in heads {
                        h.col(|ui| {
                            ui.label(RichText::new(l).size(12.5).color(t.text_weak));
                        });
                    }
                })
                .body(|body| {
                    body.rows(28.0, self.startup.len(), |mut row| {
                        let i = row.index();
                        let s = &self.startup[i];
                        row.set_selected(self.startup_sel == Some(i));
                        row.col(|ui| {
                            proc_icon(ui, self.icons.get(&exe_from_command(&s.command)), &s.name);
                            ui.label(RichText::new(&s.name).color(t.text));
                        });
                        row.col(|ui| {
                            let (txt, c) = if s.enabled { (on_s, t.accent) } else { (off_s, t.text_weak) };
                            ui.label(RichText::new(txt).color(c));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(s.source.label()).color(t.text_weak));
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(&s.command).color(t.text_weak));
                        });
                        let resp = row.response();
                        if resp.clicked() {
                            click = Some(i);
                        }
                        resp.context_menu(|ui| {
                            ui.set_min_width(220.0);
                            let label = if s.enabled { format!("⏹  {disable_s}") } else { format!("▶  {enable_s}") };
                            if ui.button(label).clicked() {
                                toggle = Some((i, !s.enabled));
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui
                                .button(format!("📂  {}", tr("Open file location", "Открыть расположение файла", "Відкрити розташування файлу")))
                                .clicked()
                            {
                                open = Some(exe_from_command(&s.command));
                                ui.close_menu();
                            }
                            if ui.button(format!("📄  {}", tr("Copy command", "Копировать команду", "Копіювати команду"))).clicked() {
                                copy = Some(s.command.clone());
                                ui.close_menu();
                            }
                        });
                    });
                });
        });
        if let Some(i) = click {
            self.startup_sel = Some(i);
        }
        if let Some(p) = open {
            win::open_location(&p);
        }
        if let Some(c) = copy {
            ui.ctx().output_mut(|o| o.copied_text = c);
            self.msg(tr("Copied to clipboard", "Скопировано в буфер обмена", "Скопійовано в буфер обміну"));
        }
        if let Some((i, en)) = toggle {
            if let Some(item) = self.startup.get(i).cloned() {
                match win::set_startup_enabled(&item, en) {
                    Ok(_) => {
                        self.msg(format!("«{}»: {}", item.name, if en { on_s } else { off_s }));
                        if let Some(s) = self.startup.get_mut(i) {
                            s.enabled = en;
                        }
                    }
                    Err(e) => {
                        let hint = if self.is_admin {
                            ""
                        } else {
                            tr(
                                " — machine-wide entries require administrator rights",
                                " — для записей компьютера нужны права администратора",
                                " — для записів комп'ютера потрібні права адміністратора",
                            )
                        };
                        self.msg(format!("{} «{}»: {e}{hint}", tr("Failed to change", "Не удалось изменить", "Не вдалося змінити"), item.name));
                    }
                }
            }
        }
    }

    fn svc_action(&mut self, name: String, kind: u8) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let service = tr("Service", "Служба", "Служба");
        self.msg(format!("{service} «{name}»: {}", tr("working…", "выполняется…", "виконується…")));
        let done = tr("done", "готово", "готово");
        std::thread::spawn(move || {
            let r = match kind {
                0 => win::start_service(&name),
                1 => win::stop_service(&name),
                _ => win::stop_service(&name).and_then(|_| win::start_service(&name)),
            };
            let m = match r {
                Ok(_) => format!("{service} «{name}»: {done}"),
                Err(e) => format!("{service} «{name}»: {e}"),
            };
            let _ = tx.send(m);
            ctx.request_repaint();
        });
    }

    fn services_tab(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let t = self.theme.clone();
        if self.services_at.map(|a| a.elapsed() > Duration::from_secs(3)).unwrap_or(true) {
            match win::list_services() {
                Ok(v) => {
                    self.services = v;
                    self.services_err = None;
                }
                Err(e) => self.services_err = Some(e),
            }
            self.services_at = Some(Instant::now());
        }
        ui.ctx().request_repaint_after(Duration::from_secs(3));

        let filter = self.search.to_lowercase();
        let mut list: Vec<&ServiceInfo> = self
            .services
            .iter()
            .filter(|s| {
                filter.is_empty()
                    || s.name.to_lowercase().contains(&filter)
                    || s.display.to_lowercase().contains(&filter)
                    || s.pid.to_string() == filter
            })
            .collect();
        let (sc, sd) = self.svc_sort;
        list.sort_by(|a, b| {
            let o = match sc {
                1 => a.pid.cmp(&b.pid),
                2 => a.display.to_lowercase().cmp(&b.display.to_lowercase()),
                3 => b.state.cmp(&a.state),
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            };
            ord(o.then_with(|| a.name.cmp(&b.name)), sd)
        });
        let list: Vec<ServiceInfo> = list.into_iter().cloned().collect();
        let running = self.services.iter().filter(|s| s.state == 4).count();
        let (start_s, stop_s, restart_s) = (
            tr("Start", "Запустить", "Запустити"),
            tr("Stop", "Остановить", "Зупинити"),
            tr("Restart", "Перезапустить", "Перезапустити"),
        );

        let mut act: Option<(String, u8)> = None;
        ui.horizontal(|ui| {
            ui.label(bold(Tab::Services.label(), 22.0));
            ui.label(
                RichText::new(format!("{} {} {}", running, tr("running of", "выполняется из", "виконується з"), self.services.len()))
                    .color(t.text_weak),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(format!("🔧  {}", tr("Open Services", "Открыть «Службы»", "Відкрити «Служби»"))).clicked() {
                    let _ = win::shell_execute("open", "services.msc", "", 1);
                }
                let sel = self.svc_sel.clone().and_then(|n| self.services.iter().find(|s| s.name == n).cloned());
                let st = sel.as_ref().map(|s| s.state).unwrap_or(0);
                if ui.add_enabled(st == 4, Button::new(format!("⟳  {restart_s}"))).clicked() {
                    act = sel.as_ref().map(|s| (s.name.clone(), 2));
                }
                if ui.add_enabled(st == 4, Button::new(format!("⏹  {stop_s}"))).clicked() {
                    act = sel.as_ref().map(|s| (s.name.clone(), 1));
                }
                if ui.add_enabled(st == 1, Button::new(format!("▶  {start_s}"))).clicked() {
                    act = sel.as_ref().map(|s| (s.name.clone(), 0));
                }
                self.search_box(ui, &format!("🔎  {}", tr("Search services", "Поиск службы", "Пошук служби")));
            });
        });
        if let Some(e) = &self.services_err {
            ui.label(
                RichText::new(format!("{}: {e}", tr("Failed to get services", "Ошибка получения списка служб", "Помилка отримання списку служб")))
                    .color(t.danger),
            );
        }
        if !self.is_admin {
            ui.label(
                RichText::new(tr(
                    "Administrator rights are required to start and stop most services.",
                    "Для запуска и остановки большинства служб нужны права администратора.",
                    "Для запуску та зупинки більшості служб потрібні права адміністратора.",
                ))
                .size(12.0)
                .color(t.text_weak),
            );
        }
        ui.add_space(8.0);

        let heads = [
            tr("Name", "Имя", "Ім'я"),
            tr("PID", "ИД процесса", "ІД процесу"),
            tr("Description", "Описание", "Опис"),
            tr("Status", "Состояние", "Стан"),
        ];
        let mut sort = self.svc_sort;
        let mut click = None;
        ui.push_id("services", |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .sense(Sense::click())
                .auto_shrink(false)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::initial(220.0).at_least(100.0).clip(true))
                .column(Column::initial(100.0).at_least(60.0))
                .column(Column::initial(380.0).at_least(100.0).clip(true))
                .column(Column::remainder().at_least(100.0))
                .header(30.0, |mut h| {
                    for (i, l) in heads.iter().enumerate() {
                        h.col(|ui| {
                            if simple_header(ui, &t, l, sort.0 == i as u8, sort.1) {
                                if sort.0 == i as u8 {
                                    sort.1 = !sort.1;
                                } else {
                                    sort = (i as u8, false);
                                }
                            }
                        });
                    }
                })
                .body(|body| {
                    body.rows(26.0, list.len(), |mut row| {
                        let s = &list[row.index()];
                        row.set_selected(self.svc_sel.as_deref() == Some(s.name.as_str()));
                        row.col(|ui| {
                            ui.label(RichText::new(&s.name).color(t.text));
                        });
                        row.col(|ui| {
                            if s.pid != 0 {
                                right_label(ui, s.pid.to_string(), t.text_weak);
                            }
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(&s.display).color(t.text_weak));
                        });
                        row.col(|ui| {
                            let c = match s.state {
                                4 => t.accent,
                                1 => t.text_weak,
                                _ => t.net,
                            };
                            ui.label(RichText::new(win::service_state_name(s.state)).color(c));
                        });
                        let resp = row.response();
                        if resp.clicked() {
                            click = Some(s.name.clone());
                        }
                        resp.context_menu(|ui| {
                            ui.set_min_width(220.0);
                            if ui.add_enabled(s.state == 1, Button::new(format!("▶  {start_s}"))).clicked() {
                                act = Some((s.name.clone(), 0));
                                ui.close_menu();
                            }
                            if ui.add_enabled(s.state == 4, Button::new(format!("⏹  {stop_s}"))).clicked() {
                                act = Some((s.name.clone(), 1));
                                ui.close_menu();
                            }
                            if ui.add_enabled(s.state == 4, Button::new(format!("⟳  {restart_s}"))).clicked() {
                                act = Some((s.name.clone(), 2));
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui
                                .add_enabled(s.pid != 0, Button::new(format!("📋  {}", tr("Go to process", "Перейти к процессу", "Перейти до процесу"))))
                                .clicked()
                            {
                                actions.push(Action::Details(s.pid));
                                ui.close_menu();
                            }
                            if ui.button(format!("🔎  {}", tr("Search online", "Поиск в Интернете", "Пошук в Інтернеті"))).clicked() {
                                actions.push(Action::Search(format!("windows service {}", s.name)));
                                ui.close_menu();
                            }
                            if ui.button(format!("📄  {}", tr("Copy name", "Копировать имя", "Копіювати ім'я"))).clicked() {
                                actions.push(Action::Copy(s.name.clone()));
                                ui.close_menu();
                            }
                        });
                    });
                });
        });
        self.svc_sort = sort;
        if let Some(c) = click {
            self.svc_sel = Some(c);
        }
        if let Some((n, k)) = act {
            self.svc_action(n, k);
        }
    }

    fn theme_card(ui: &mut Ui, base: &Settings, id: ThemeId, selected: bool) -> egui::Response {
        let mut s = base.clone();
        s.theme = id;
        let pt = Theme::build(&s);
        let (rect, resp) = ui.allocate_exact_size(vec2(176.0, 128.0), Sense::click());
        let p = ui.painter();
        let prev = Rect::from_min_size(rect.min, vec2(rect.width(), 96.0));
        let r = Rounding::same(12.0);
        if pt.glass {
            // «обои» за стеклом
            p.rect_filled(prev, r, Color32::from_rgb(120, 140, 190));
            p.circle_filled(prev.left_top() + vec2(40.0, 30.0), 34.0, Color32::from_rgb(255, 110, 170));
            p.circle_filled(prev.right_bottom() - vec2(40.0, 26.0), 40.0, Color32::from_rgb(80, 200, 255));
            p.circle_filled(prev.center() + vec2(10.0, 4.0), 22.0, Color32::from_rgb(255, 200, 90));
        }
        p.rect_filled(prev, r, pt.window_bg);
        let side = Rect::from_min_size(prev.min + vec2(8.0, 8.0), vec2(36.0, prev.height() - 16.0));
        let main = Rect::from_min_max(pos2(side.right() + 6.0, side.top()), prev.max - vec2(8.0, 8.0));
        for rr in [side, main] {
            p.rect_filled(rr, Rounding::same(7.0), pt.card);
            p.rect_stroke(rr, Rounding::same(7.0), Stroke::new(1.0_f32, pt.card_stroke));
        }
        p.rect_filled(Rect::from_min_size(side.min + vec2(5.0, 8.0), vec2(26.0, 6.0)), Rounding::same(3.0), pt.accent);
        for (i, c) in [pt.cpu, pt.mem, pt.disk].iter().enumerate() {
            let y = main.top() + 12.0 + i as f32 * 16.0;
            let w = main.width() - 20.0;
            p.rect_filled(
                Rect::from_min_size(pos2(main.left() + 10.0, y), vec2(w * (0.85 - i as f32 * 0.2), 7.0)),
                Rounding::same(3.0),
                *c,
            );
        }
        p.text(pos2(main.left() + 10.0, main.bottom() - 8.0), Align2::LEFT_BOTTOM, "Aa", FontId::proportional(12.0), pt.text);
        let stroke = if selected {
            Stroke::new(2.0_f32, ui.visuals().selection.stroke.color)
        } else if resp.hovered() {
            Stroke::new(1.0_f32, ui.visuals().widgets.hovered.bg_stroke.color)
        } else {
            Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color)
        };
        p.rect_stroke(prev, r, stroke);
        let txt = if selected { format!("✔ {}", id.name()) } else { id.name().to_string() };
        p.text(pos2(rect.left() + 2.0, prev.bottom() + 8.0), Align2::LEFT_TOP, txt, FontId::proportional(13.0), ui.visuals().text_color());
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    /// Плавно выезжающий блок настроек.
    fn reveal(ui: &mut Ui, id: &str, open: bool, add: impl FnOnce(&mut Ui)) {
        let mut st = CollapsingState::load_with_default_open(ui.ctx(), ui.make_persistent_id(id), open);
        st.set_open(open);
        st.show_body_unindented(ui, |ui| add(ui));
        st.store(ui.ctx());
    }

    fn settings_tab(&mut self, ui: &mut Ui) {
        let t = self.theme.clone();
        if self.hook_checked.map(|c| c.elapsed() > Duration::from_millis(1500)).unwrap_or(true) {
            self.hook_ours = win::hook_is_ours();
            self.hook_other = win::hook_value().filter(|_| !self.hook_ours);
            self.hook_checked = Some(Instant::now());
        }
        ui.ctx().request_repaint_after(Duration::from_millis(1500));

        ui.label(bold(Tab::Settings.label(), 22.0));
        ui.add_space(6.0);
        ui.push_id("settings_scroll", |ui| {
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let section = |ui: &mut Ui, s: &str| {
                    ui.add_space(12.0);
                    ui.label(bold(s, 16.0).color(t.text));
                    ui.add_space(4.0);
                };
                let note = |ui: &mut Ui, s: &str| {
                    ui.label(RichText::new(s).size(12.0).color(t.text_weak));
                };

                // ── язык
                section(ui, "Language / Мова / Язык");
                ui.horizontal(|ui| {
                    for l in Lang::ALL {
                        ui.selectable_value(&mut self.settings.lang, l, l.native_name());
                    }
                });

                // ── тема
                section(ui, tr("Theme", "Тема оформления", "Тема оформлення"));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
                    for id in ThemeId::ALL {
                        if Self::theme_card(ui, &self.settings, id, self.settings.theme == id).clicked() {
                            self.settings.theme = id;
                        }
                    }
                });

                // ── настройки стекла (выезжают, когда активна Liquid Glass)
                let is_glass = self.settings.theme == ThemeId::Glass;
                Self::reveal(ui, "glass_settings", is_glass, |ui| {
                    card(ui, &t, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(bold("Liquid Glass", 15.0).color(t.text));
                        ui.horizontal(|ui| {
                            ui.label(tr("Tint density", "Плотность тонировки", "Щільність тонування"));
                            ui.add(egui::Slider::new(&mut self.settings.glass_opacity, 0.0..=1.0).show_value(false));
                            ui.label(RichText::new(format!("{:.0}%", self.settings.glass_opacity * 100.0)).color(t.text_weak));
                        });
                        for m in BlurMode::ALL {
                            ui.radio_value(&mut self.settings.blur, m, m.name());
                        }
                        note(
                            ui,
                            tr(
                                "Tip: on Windows 10 acrylic may lag while dragging the window — choose “Blur” then.",
                                "Совет: на Windows 10 акрил может подтормаживать при перетаскивании окна — тогда выберите «Размытие».",
                                "Порада: у Windows 10 акрил може гальмувати під час перетягування вікна — тоді оберіть «Розмиття».",
                            ),
                        );
                    });
                });

                // ── палитра для Default (выезжает)
                let is_default = self.settings.theme == ThemeId::Default;
                Self::reveal(ui, "palette_settings", is_default, |ui| {
                    card(ui, &t, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(bold(tr("RGB palette", "RGB-палитра", "RGB-палітра"), 15.0).color(t.text));
                        note(
                            ui,
                            tr(
                                "Pick any colors — dark or light is up to you.",
                                "Выберите любые цвета — тёмная или светлая тема решаете вы сами.",
                                "Оберіть будь-які кольори — темна чи світла тема вирішуєте ви.",
                            ),
                        );
                        ui.horizontal(|ui| {
                            ui.label(tr("Background", "Фон", "Фон"));
                            egui::color_picker::color_edit_button_srgb(ui, &mut self.settings.custom_bg);
                            ui.add_space(16.0);
                            ui.label(tr("Accent", "Акцент", "Акцент"));
                            egui::color_picker::color_edit_button_srgb(ui, &mut self.settings.custom_accent);
                            ui.add_space(16.0);
                            if ui.button(tr("Reset", "Сбросить", "Скинути")).clicked() {
                                self.settings.custom_bg = DEFAULT_BG;
                                self.settings.custom_accent = DEFAULT_ACCENT;
                            }
                        });
                    });
                });

                // ── текст
                section(ui, tr("Text color", "Цвет текста", "Колір тексту"));
                ui.horizontal(|ui| {
                    for c in TextColor::ALL {
                        ui.selectable_value(&mut self.settings.text_color, c, c.label());
                    }
                });
                section(ui, tr("Text size", "Размер текста", "Розмір тексту"));
                ui.horizontal(|ui| {
                    for z in [0.9f32, 1.0, 1.1, 1.25, 1.5] {
                        let sel = (self.settings.ui_scale - z).abs() < 0.001;
                        if ui.selectable_label(sel, format!("{:.0}%", z * 100.0)).clicked() {
                            self.settings.ui_scale = z;
                        }
                    }
                });

                // ── обновление
                section(ui, tr("Update speed", "Скорость обновления", "Швидкість оновлення"));
                ui.horizontal(|ui| {
                    for (ms, l) in [
                        (500u64, tr("High · 0.5 s", "Высокая · 0,5 с", "Висока · 0,5 с")),
                        (1000, tr("Normal · 1 s", "Обычная · 1 с", "Звичайна · 1 с")),
                        (2000, tr("Low · 2 s", "Низкая · 2 с", "Низька · 2 с")),
                        (5000, tr("Very low · 5 s", "Очень низкая · 5 с", "Дуже низька · 5 с")),
                    ] {
                        ui.selectable_value(&mut self.settings.refresh_ms, ms, l);
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.settings.paused, tr("Pause updates", "Приостановить обновление", "Призупинити оновлення"));
                    if ui.button(format!("⟳  {}", tr("Refresh now (F5)", "Обновить сейчас (F5)", "Оновити зараз (F5)"))).clicked() {
                        self.monitor.refresh_now();
                    }
                });

                // ── окно
                section(ui, tr("Window", "Окно", "Вікно"));
                ui.checkbox(&mut self.settings.always_on_top, tr("Always on top", "Поверх остальных окон", "Поверх інших вікон"));
                ui.checkbox(
                    &mut self.settings.group_by_name,
                    tr("Group processes by name", "Группировать процессы по имени", "Групувати процеси за ім'ям"),
                );

                // ── горячая клавиша
                section(ui, tr("Ctrl+Shift+Esc hotkey", "Горячая клавиша Ctrl+Shift+Esc", "Гаряча клавіша Ctrl+Shift+Esc"));
                card(ui, &t, |ui| {
                    ui.set_width(ui.available_width());
                    if self.hook_ours {
                        ui.label(
                            RichText::new(format!(
                                "✔ {}",
                                tr("Enabled — Ctrl+Shift+Esc opens SwTsM", "Включено — Ctrl+Shift+Esc открывает SwTsM", "Увімкнено — Ctrl+Shift+Esc відкриває SwTsM")
                            ))
                            .color(t.accent),
                        );
                    } else {
                        ui.label(tr(
                            "Ctrl+Shift+Esc currently opens the standard Windows Task Manager.",
                            "Сейчас Ctrl+Shift+Esc открывает стандартный диспетчер задач Windows.",
                            "Зараз Ctrl+Shift+Esc відкриває стандартний диспетчер завдань Windows.",
                        ));
                    }
                    if let Some(o) = &self.hook_other {
                        ui.label(
                            RichText::new(format!(
                                "{}: {o}",
                                tr("Replaced by another program", "Диспетчер заменён другой программой", "Диспетчер замінено іншою програмою")
                            ))
                            .size(12.0)
                            .color(t.net),
                        );
                    }
                    note(
                        ui,
                        tr(
                            "Uses the same mechanism as Process Explorer (registry entry for taskmgr.exe). Requires administrator rights (UAC prompt). Don't move the exe after enabling.",
                            "Используется тот же механизм, что у Process Explorer (запись в реестре для taskmgr.exe). Нужны права администратора (запрос UAC). Не перемещайте exe после включения.",
                            "Використовується той самий механізм, що й у Process Explorer (запис у реєстрі для taskmgr.exe). Потрібні права адміністратора (запит UAC). Не переміщуйте exe після ввімкнення.",
                        ),
                    );
                    if let Ok(exe) = std::env::current_exe() {
                        note(ui, &format!("{}: {}", tr("Path", "Путь", "Шлях"), exe.display()));
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if !self.hook_ours
                            && ui
                                .button(format!(
                                    "⌨  {}",
                                    tr("Open with Ctrl+Shift+Esc", "Открывать по Ctrl+Shift+Esc", "Відкривати по Ctrl+Shift+Esc")
                                ))
                                .clicked()
                        {
                            match win::request_hook(true) {
                                Ok(_) => self.msg(tr("Requested", "Запрошено", "Запитано")),
                                Err(e) => self.msg(format!("{}: {e}", tr("Failed", "Не удалось", "Не вдалося"))),
                            }
                            self.hook_checked = None;
                        }
                        if (self.hook_ours || self.hook_other.is_some())
                            && ui
                                .button(format!(
                                    "↩  {}",
                                    tr("Restore standard Task Manager", "Вернуть стандартный диспетчер", "Повернути стандартний диспетчер")
                                ))
                                .clicked()
                        {
                            match win::request_hook(false) {
                                Ok(_) => self.msg(tr("Requested", "Запрошено", "Запитано")),
                                Err(e) => self.msg(format!("{}: {e}", tr("Failed", "Не удалось", "Не вдалося"))),
                            }
                            self.hook_checked = None;
                        }
                    });
                });

                // ── права
                section(ui, tr("Permissions", "Права", "Права"));
                if self.is_admin {
                    ui.label(
                        RichText::new(format!(
                            "🛡 {}",
                            tr("Running as administrator", "Запущено от имени администратора", "Запущено від імені адміністратора")
                        ))
                        .color(t.accent),
                    );
                } else {
                    ui.label(tr(
                        "Running with standard rights — some processes and services are unavailable.",
                        "Запущено с обычными правами — некоторые процессы и службы недоступны.",
                        "Запущено зі звичайними правами — деякі процеси та служби недоступні.",
                    ));
                    if ui
                        .button(format!(
                            "🛡  {}",
                            tr("Restart as administrator", "Перезапустить от имени администратора", "Перезапустити від імені адміністратора")
                        ))
                        .clicked()
                    {
                        match win::relaunch_as_admin() {
                            Ok(_) => {
                                self.settings.save();
                                ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                            }
                            Err(e) => self.msg(format!("{}: {e}", tr("Failed", "Не удалось", "Не вдалося"))),
                        }
                    }
                }

                // ── сочетания
                section(ui, tr("Keyboard shortcuts", "Сочетания клавиш", "Сполучення клавіш"));
                for (k, d) in [
                    ("Ctrl+F", tr("search", "поиск", "пошук")),
                    ("Delete", tr("end selected task", "снять выбранную задачу", "зняти вибране завдання")),
                    ("F5", tr("refresh now", "обновить сейчас", "оновити зараз")),
                    (
                        tr("Right click", "ПКМ по строке", "ПКМ по рядку"),
                        tr("menu: priority, suspend, process tree…", "меню: приоритет, приостановка, дерево процессов…", "меню: пріоритет, призупинення, дерево процесів…"),
                    ),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(bold(k, 13.0).color(t.text));
                        ui.label(RichText::new(d).size(13.0).color(t.text_weak));
                    });
                }

                // ── о программе
                section(ui, tr("About", "О программе", "Про програму"));
                ui.horizontal(|ui| {
                    logo(ui, 40.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(bold(format!("SwTsM {}", env!("CARGO_PKG_VERSION")), 16.0).color(t.text));
                        ui.label(
                            RichText::new(tr(
                                "Task manager written in Rust (egui + sysinfo).",
                                "Диспетчер задач, написан на Rust (egui + sysinfo).",
                                "Диспетчер завдань, написаний на Rust (egui + sysinfo).",
                            ))
                            .color(t.text_weak),
                        );
                        ui.label(bold("Made by MrSnowman", 14.0).color(t.accent));
                    });
                });

                // ── удаление
                section(ui, tr("Uninstall", "Удаление", "Видалення"));
                note(
                    ui,
                    tr(
                        "Removes SwTsM, its shortcuts, settings and restores the standard Ctrl+Shift+Esc.",
                        "Удаляет SwTsM, ярлыки, настройки и возвращает стандартный Ctrl+Shift+Esc.",
                        "Видаляє SwTsM, ярлики, налаштування та повертає стандартний Ctrl+Shift+Esc.",
                    ),
                );
                if ui
                    .add(
                        Button::new(RichText::new(format!("🗑  {}", tr("Uninstall SwTsM", "Удалить SwTsM", "Видалити SwTsM"))).color(Color32::WHITE))
                            .fill(t.danger),
                    )
                    .clicked()
                {
                    self.uninstall_confirm = Some(Instant::now());
                }
                ui.add_space(20.0);
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(started) = self.uninstall_confirm {
            let left = 3i64 - started.elapsed().as_secs() as i64;
            let t = self.theme.clone();
            let mut close = false;
            let mut go = false;
            egui::Window::new(tr("Uninstall SwTsM", "Удаление SwTsM", "Видалення SwTsM"))
                .id(egui::Id::new("uninstall_dialog"))
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(bold(
                        tr(
                            "Are you sure you want to uninstall SwTsM?",
                            "Вы точно хотите удалить SwTsM?",
                            "Ви точно хочете видалити SwTsM?",
                        ),
                        16.0,
                    ));
                    ui.label(RichText::new(tr(
                        "The app, its shortcuts and settings will be removed, and Ctrl+Shift+Esc will open the standard Task Manager again.",
                        "Будут удалены приложение, ярлыки и настройки, а Ctrl+Shift+Esc снова будет открывать стандартный диспетчер.",
                        "Буде видалено програму, ярлики та налаштування, а Ctrl+Shift+Esc знову відкриватиме стандартний диспетчер.",
                    )).color(t.text_weak));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let label = if left > 0 {
                            format!("{} ({left})", tr("Uninstall", "Удалить", "Видалити"))
                        } else {
                            tr("Uninstall", "Удалить", "Видалити").to_string()
                        };
                        let btn = Button::new(RichText::new(format!("  {label}  ")).color(Color32::WHITE)).fill(if left > 0 {
                            t.danger.gamma_multiply(0.45)
                        } else {
                            t.danger
                        });
                        if ui.add_enabled(left <= 0, btn).clicked() {
                            go = true;
                        }
                        if ui.button(tr("Cancel", "Отмена", "Скасувати")).clicked() {
                            close = true;
                        }
                    });
                });
            if left > 0 {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                close = true;
            }
            if go {
                self.uninstall_confirm = None;
                let spawned = std::env::current_exe()
                    .and_then(|exe| std::process::Command::new(exe).arg("--uninstall").spawn())
                    .is_ok();
                if spawned {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                } else {
                    self.msg(tr("Failed to start uninstall", "Не удалось запустить удаление", "Не вдалося запустити видалення"));
                }
            } else if close {
                self.uninstall_confirm = None;
            }
        }

        if let Some(mut d) = self.run_dialog.take() {
            let mut open = true;
            let mut submit = false;
            let mut cancel = false;
            egui::Window::new(tr("Create new task", "Создать новую задачу", "Створити нове завдання"))
                .id(egui::Id::new("run_dialog"))
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(tr(
                        "Type the name of a program, folder, document or Internet resource:",
                        "Введите имя программы, папки, документа или ресурса Интернета:",
                        "Введіть ім'я програми, папки, документа або ресурсу Інтернету:",
                    ));
                    let r = ui.add(
                        TextEdit::singleline(&mut d.text)
                            .desired_width(400.0)
                            .hint_text(tr("e.g.: notepad, cmd, regedit, C:\\Windows", "например: notepad, cmd, regedit, C:\\Windows", "наприклад: notepad, cmd, regedit, C:\\Windows")),
                    );
                    if d.first {
                        r.request_focus();
                        d.first = false;
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        submit = true;
                    }
                    ui.checkbox(
                        &mut d.admin,
                        format!(
                            "🛡 {}",
                            tr(
                                "Create this task with administrative privileges",
                                "Создать задачу с правами администратора",
                                "Створити завдання з правами адміністратора"
                            )
                        ),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("   OK   ").clicked() {
                            submit = true;
                        }
                        if ui.button(tr("Cancel", "Отмена", "Скасувати")).clicked() {
                            cancel = true;
                        }
                    });
                });
            if submit {
                match win::run_task(&d.text, d.admin) {
                    Ok(_) => self.msg(format!("{}: {}", tr("Started", "Запущено", "Запущено"), d.text)),
                    Err(e) => self.msg(format!("{} «{}»: {e}", tr("Failed to start", "Не удалось запустить", "Не вдалося запустити"), d.text)),
                }
                self.monitor.refresh_now();
            } else if !cancel && open && !ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.run_dialog = Some(d);
            }
        }

        if let Some(c) = self.confirm.take() {
            let mut yes = false;
            let mut no = false;
            let t = self.theme.clone();
            egui::Window::new(c.title.clone())
                .id(egui::Id::new("confirm_dialog"))
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(&c.text);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(Button::new(RichText::new(format!("  {}  ", tr("End", "Завершить", "Завершити"))).color(Color32::WHITE)).fill(t.danger))
                            .clicked()
                        {
                            yes = true;
                        }
                        if ui.button(tr("Cancel", "Отмена", "Скасувати")).clicked() {
                            no = true;
                        }
                    });
                });
            if yes {
                self.kill_pids(&c.pids, &c.label);
            } else if !no && !ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.confirm = Some(c);
            }
        }
    }
}

impl eframe::App for TaskApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.theme.glass {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            let c = self.theme.window_bg;
            [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0]
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.snap = self.monitor.latest();
        while let Ok(m) = self.rx.try_recv() {
            self.msg(m);
            self.services_at = None;
        }
        self.sync_window(ctx);
        self.pump_icons(ctx);

        let mut actions = Vec::new();
        self.shortcuts(ctx, &mut actions);

        let t = self.theme.clone();
        let bg = Frame::none().fill(t.window_bg);

        egui::TopBottomPanel::bottom("status")
            .show_separator_line(false)
            .frame(bg.inner_margin(Margin { left: 18.0, right: 18.0, top: 2.0, bottom: 8.0 }))
            .show(ctx, |ui| self.status_bar(ui));

        egui::SidePanel::left("nav")
            .exact_width(236.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(bg.inner_margin(Margin { left: 10.0, right: 5.0, top: 10.0, bottom: 6.0 }))
            .show(ctx, |ui| self.nav(ui));

        egui::CentralPanel::default()
            .frame(bg.inner_margin(Margin { left: 5.0, right: 10.0, top: 10.0, bottom: 6.0 }))
            .show(ctx, |ui| {
                card(ui, &t, |ui| {
                    ui.set_min_size(ui.available_size());
                    match self.tab {
                        Tab::Processes => self.processes_tab(ui, &mut actions),
                        Tab::Performance => self.performance_tab(ui),
                        Tab::Users => self.users_tab(ui, &mut actions),
                        Tab::Details => self.details_tab(ui, &mut actions),
                        Tab::Startup => self.startup_tab(ui),
                        Tab::Services => self.services_tab(ui, &mut actions),
                        Tab::Settings => self.settings_tab(ui),
                    }
                });
            });

        self.run_actions(ctx, actions);
        self.dialogs(ctx);

        if self.settings != self.saved {
            self.settings.save();
            self.saved = self.settings.clone();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.settings.save();
    }
}
