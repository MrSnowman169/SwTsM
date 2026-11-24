#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod sys;

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{
    self, vec2, Align, Align2, Button, Color32, FontFamily, FontId, Frame, Layout, Margin, RichText, Rounding, Sense,
    Stroke, TextEdit, Ui,
};

/// Само приложение, вшитое в инсталлер.
static APP_EXE: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/release/SwTsM.exe"));
const VERSION: &str = env!("CARGO_PKG_VERSION");
const TITLE: &str = "SwTsM Setup";

// ─────────────────────────── язык ───────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Lang {
    En,
    Ru,
    Uk,
}

impl Lang {
    fn code(self) -> &'static str {
        match self {
            Lang::En => "En",
            Lang::Ru => "Ru",
            Lang::Uk => "Uk",
        }
    }
    fn native(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ru => "Русский",
            Lang::Uk => "Українська",
        }
    }
    fn tr(self, en: &'static str, ru: &'static str, uk: &'static str) -> &'static str {
        match self {
            Lang::En => en,
            Lang::Ru => ru,
            Lang::Uk => uk,
        }
    }
}

// ─────────────────────────── палитра ───────────────────────────

const BG: Color32 = Color32::from_rgb(24, 25, 30);
const CARD: Color32 = Color32::from_rgb(33, 34, 41);
const STROKE: Color32 = Color32::from_rgb(52, 54, 64);
const TEXT: Color32 = Color32::from_rgb(236, 238, 242);
const WEAK: Color32 = Color32::from_rgb(150, 155, 168);
const ACCENT: Color32 = Color32::from_rgb(88, 166, 255);
const GOOD: Color32 = Color32::from_rgb(74, 222, 128);
const BAD: Color32 = Color32::from_rgb(248, 113, 113);

fn bold(t: impl Into<String>, size: f32) -> RichText {
    RichText::new(t.into()).size(size).family(FontFamily::Name("bold".into()))
}

// ─────────────────────────── состояние ───────────────────────────

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Language,
    Location,
    Installing,
    Done,
    Failed,
}

#[derive(Default)]
struct Progress {
    step: usize,
    total: usize,
    text: String,
    finished: bool,
    error: Option<String>,
    warnings: Vec<String>,
}

struct Opts {
    dir: PathBuf,
    desktop: bool,
    start_menu: bool,
    hotkey: bool,
    lang: Lang,
    admin: bool,
}

struct Setup {
    lang: Lang,
    page: Page,
    admin: bool,
    path: String,
    desktop: bool,
    start_menu: bool,
    hotkey: bool,
    launch: bool,
    browse_rx: Option<Receiver<Option<String>>>,
    progress: Arc<Mutex<Progress>>,
}

fn default_dir(admin: bool) -> String {
    let base = if admin {
        std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into())
    } else {
        std::env::var("LOCALAPPDATA").map(|p| format!("{p}\\Programs")).unwrap_or_else(|_| "C:\\SwTsM".into())
    };
    format!("{base}\\SwTsM")
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let load = |f: &str| std::fs::read(format!("{windir}\\Fonts\\{f}")).ok();
    if let Some(b) = load("segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), egui::FontData::from_owned(b));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "segoe".into());
    }
    let mut bold_fam = Vec::new();
    if let Some(b) = load("seguisb.ttf") {
        fonts.font_data.insert("segoe_sb".into(), egui::FontData::from_owned(b));
        bold_fam.push("segoe_sb".to_string());
    }
    bold_fam.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Name("bold".into()), bold_fam);
    ctx.set_fonts(fonts);

    let mut v = egui::Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.extreme_bg_color = Color32::from_rgb(18, 19, 23);
    v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    let r = Rounding::same(7.0);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.rounding = r;
    }
    v.widgets.inactive.weak_bg_fill = CARD;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, STROKE);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(44, 46, 56);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.7));
    ctx.set_visuals(v);
    let mut style = (*ctx.style()).clone();
    style.spacing.button_padding = vec2(14.0, 6.0);
    style.spacing.item_spacing = vec2(8.0, 8.0);
    ctx.set_style(style);
}

fn logo(ui: &mut Ui, size: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, Rounding::same(size * 0.29), Color32::from_rgb(10, 10, 12));
    p.rect_stroke(r, Rounding::same(size * 0.29), Stroke::new(1.0_f32, Color32::from_white_alpha(40)));
    let k = size / 34.0;
    let pts = [(-11.0, 2.0), (-5.0, 2.0), (-2.0, -7.0), (2.0, 8.0), (5.0, -2.0), (11.0, -2.0)];
    let pts: Vec<_> = pts.iter().map(|(x, y)| r.center() + vec2(*x * k, *y * k)).collect();
    p.add(egui::Shape::line(pts, Stroke::new(2.0_f32 * k.max(1.0), Color32::WHITE)));
}

fn icon_data() -> egui::IconData {
    let n = 64usize;
    let mut rgba = vec![0u8; n * n * 4];
    let radius = 14.0f32;
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let cx = fx.clamp(radius, n as f32 - radius);
            let cy = fy.clamp(radius, n as f32 - radius);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let a = (radius - d + 0.5).clamp(0.0, 1.0);
            let i = (y * n + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[18, 18, 20, (a * 255.0) as u8]);
        }
    }
    let pts = [(8, 36), (22, 36), (28, 20), (36, 48), (42, 30), (56, 30)];
    for w in pts.windows(2) {
        let (x0, y0, x1, y1) = (w[0].0 as f32, w[0].1 as f32, w[1].0 as f32, w[1].1 as f32);
        for s in 0..=80 {
            let t = s as f32 / 80.0;
            let (px, py) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    if dx * dx + dy * dy > 5 {
                        continue;
                    }
                    let xx = (px as i32 + dx).clamp(0, n as i32 - 1) as usize;
                    let yy = (py as i32 + dy).clamp(0, n as i32 - 1) as usize;
                    let i = (yy * n + xx) * 4;
                    rgba[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
                }
            }
        }
    }
    egui::IconData { rgba, width: n as u32, height: n as u32 }
}

// ─────────────────────────── установка ───────────────────────────

fn install(o: Opts, prog: Arc<Mutex<Progress>>) -> Result<(), String> {
    let l = o.lang;
    let total = 6;
    let step = |n: usize, text: &str| {
        if let Ok(mut p) = prog.lock() {
            p.step = n;
            p.total = total;
            p.text = text.to_string();
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    let warn = |w: String| {
        if let Ok(mut p) = prog.lock() {
            p.warnings.push(w);
        }
    };

    step(1, l.tr("Closing running SwTsM…", "Закрытие запущенного SwTsM…", "Закриття запущеного SwTsM…"));
    sys::kill_running_app();
    std::thread::sleep(Duration::from_millis(300));

    step(2, l.tr("Copying files…", "Копирование файлов…", "Копіювання файлів…"));
    std::fs::create_dir_all(&o.dir).map_err(|e| format!("{}: {e}", o.dir.display()))?;
    let exe = o.dir.join("SwTsM.exe");
    let mut last_err = None;
    for _ in 0..10 {
        match std::fs::write(&exe, APP_EXE) {
            Ok(_) => {
                last_err = None;
                break;
            }
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(Duration::from_millis(300));
            }
        }
    }
    if let Some(e) = last_err {
        return Err(format!("{}: {e}", exe.display()));
    }
    // маркер: папку можно целиком удалить при деинсталляции
    let _ = std::fs::write(o.dir.join("swtsm.installed"), VERSION);

    step(3, l.tr("Saving settings…", "Сохранение настроек…", "Збереження налаштувань…"));
    if let Err(e) = sys::save_language(o.lang.code()) {
        warn(format!("settings: {e}"));
    }

    step(4, l.tr("Creating shortcuts…", "Создание ярлыков…", "Створення ярликів…"));
    if let Err(e) = sys::create_shortcuts(&exe, o.desktop, o.start_menu, o.admin) {
        warn(format!("shortcuts: {e}"));
    }

    step(5, l.tr("Registering in Windows…", "Регистрация в Windows…", "Реєстрація у Windows…"));
    let size_kb = (APP_EXE.len() / 1024) as u32;
    if let Err(e) = sys::register_uninstall(&exe, VERSION, size_kb, o.admin) {
        warn(format!("uninstall entry: {e}"));
    }

    step(6, l.tr("Setting up Ctrl+Shift+Esc…", "Настройка Ctrl+Shift+Esc…", "Налаштування Ctrl+Shift+Esc…"));
    if o.hotkey && o.admin {
        if let Err(e) = sys::install_hook(&exe) {
            warn(format!("Ctrl+Shift+Esc: {e}"));
        }
    }
    Ok(())
}

// ─────────────────────────── интерфейс ───────────────────────────

impl Setup {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        let admin = sys::is_admin();
        Self {
            lang: Lang::En,
            page: Page::Language,
            admin,
            path: default_dir(admin),
            desktop: true,
            start_menu: true,
            hotkey: admin,
            launch: true,
            browse_rx: None,
            progress: Arc::new(Mutex::new(Progress::default())),
        }
    }

    fn steps_bar(&self, ui: &mut Ui) {
        let l = self.lang;
        let items = [
            (Page::Language, l.tr("Language", "Язык", "Мова")),
            (Page::Location, l.tr("Location", "Папка", "Тека")),
            (Page::Installing, l.tr("Install", "Установка", "Встановлення")),
            (Page::Done, l.tr("Done", "Готово", "Готово")),
        ];
        let cur = match self.page {
            Page::Language => 0,
            Page::Location => 1,
            Page::Installing | Page::Failed => 2,
            Page::Done => 3,
        };
        ui.horizontal(|ui| {
            for (i, (_, name)) in items.iter().enumerate() {
                let (c, fill) = if i < cur {
                    (TEXT, GOOD)
                } else if i == cur {
                    (TEXT, ACCENT)
                } else {
                    (WEAK, STROKE)
                };
                let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 11.0, fill);
                let mark = if i < cur { "✔".to_string() } else { (i + 1).to_string() };
                ui.painter().text(r.center(), Align2::CENTER_CENTER, mark, FontId::proportional(12.0), Color32::BLACK);
                ui.label(RichText::new(*name).color(c));
                if i + 1 < items.len() {
                    let (r, _) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::hover());
                    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0_f32, STROKE));
                }
            }
        });
    }

    fn page_language(&mut self, ui: &mut Ui) {
        ui.label(bold("Choose language · Выберите язык · Оберіть мову", 18.0));
        ui.add_space(4.0);
        ui.label(RichText::new("The app will use this language too.").color(WEAK));
        ui.add_space(14.0);
        for l in [Lang::En, Lang::Ru, Lang::Uk] {
            let sel = self.lang == l;
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
            let p = ui.painter();
            p.rect_filled(rect, Rounding::same(10.0), if sel { ACCENT.gamma_multiply(0.25) } else if resp.hovered() { CARD.gamma_multiply(1.3) } else { CARD });
            p.rect_stroke(rect, Rounding::same(10.0), Stroke::new(1.0_f32, if sel { ACCENT } else { STROKE }));
            p.text(rect.left_center() + vec2(18.0, 0.0), Align2::LEFT_CENTER, l.native(), FontId::new(16.0, FontFamily::Name("bold".into())), TEXT);
            if sel {
                p.text(rect.right_center() - vec2(18.0, 0.0), Align2::RIGHT_CENTER, "✔", FontId::proportional(16.0), ACCENT);
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.lang = l;
            }
            ui.add_space(4.0);
        }
    }

    fn page_location(&mut self, ui: &mut Ui) {
        let l = self.lang;
        if let Some(rx) = &self.browse_rx {
            if let Ok(r) = rx.try_recv() {
                if let Some(mut p) = r {
                    if !p.to_lowercase().trim_end_matches('\\').ends_with("swtsm") {
                        p = format!("{}\\SwTsM", p.trim_end_matches('\\'));
                    }
                    self.path = p;
                }
                self.browse_rx = None;
            }
        }
        ui.label(bold(l.tr("Install location", "Место установки", "Місце встановлення"), 18.0));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(TextEdit::singleline(&mut self.path).desired_width(ui.available_width() - 110.0).margin(vec2(8.0, 6.0)));
            let browsing = self.browse_rx.is_some();
            if ui.add_enabled(!browsing, Button::new(l.tr("Browse…", "Обзор…", "Огляд…"))).clicked() {
                let (tx, rx) = channel();
                let title = l.tr("Choose install folder", "Выберите папку установки", "Оберіть теку встановлення").to_string();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let _ = tx.send(sys::browse_folder(&title));
                    ctx.request_repaint();
                });
                self.browse_rx = Some(rx);
            }
        });
        ui.label(
            RichText::new(format!(
                "{} {:.1} MB",
                l.tr("Required space:", "Требуется места:", "Потрібно місця:"),
                APP_EXE.len() as f64 / 1048576.0
            ))
            .size(12.5)
            .color(WEAK),
        );
        ui.add_space(10.0);
        ui.checkbox(&mut self.desktop, l.tr("Create desktop shortcut", "Ярлык на рабочем столе", "Ярлик на робочому столі"));
        ui.checkbox(&mut self.start_menu, l.tr("Add to Start menu", "Добавить в меню «Пуск»", "Додати до меню «Пуск»"));
        ui.add_enabled_ui(self.admin, |ui| {
            ui.checkbox(
                &mut self.hotkey,
                l.tr(
                    "Open SwTsM with Ctrl+Shift+Esc (instead of the standard Task Manager)",
                    "Открывать SwTsM по Ctrl+Shift+Esc (вместо стандартного диспетчера)",
                    "Відкривати SwTsM по Ctrl+Shift+Esc (замість стандартного диспетчера)",
                ),
            );
        });
        ui.checkbox(&mut self.launch, l.tr("Launch SwTsM after installation", "Запустить SwTsM после установки", "Запустити SwTsM після встановлення"));
        if !self.admin {
            ui.add_space(8.0);
            Frame::none().fill(Color32::from_rgb(50, 40, 20)).rounding(Rounding::same(8.0)).inner_margin(Margin::same(10.0)).show(ui, |ui| {
                ui.label(
                    RichText::new(l.tr(
                        "Setup is running without administrator rights: SwTsM will be installed for the current user only and Ctrl+Shift+Esc can't be configured.",
                        "Установщик запущен без прав администратора: SwTsM будет установлен только для текущего пользователя, Ctrl+Shift+Esc настроить нельзя.",
                        "Інсталятор запущено без прав адміністратора: SwTsM буде встановлено лише для поточного користувача, Ctrl+Shift+Esc налаштувати не можна.",
                    ))
                    .size(12.5)
                    .color(Color32::from_rgb(255, 210, 140)),
                );
                if ui.button(l.tr("🛡 Restart as administrator", "🛡 Перезапустить от имени администратора", "🛡 Перезапустити від імені адміністратора")).clicked()
                    && sys::relaunch_elevated()
                {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }
    }

    fn start_install(&mut self, ctx: &egui::Context) {
        self.page = Page::Installing;
        let opts = Opts {
            dir: PathBuf::from(self.path.trim()),
            desktop: self.desktop,
            start_menu: self.start_menu,
            hotkey: self.hotkey,
            lang: self.lang,
            admin: self.admin,
        };
        let prog = self.progress.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let r = install(opts, prog.clone());
            if let Ok(mut p) = prog.lock() {
                p.finished = true;
                p.error = r.err();
            }
            ctx.request_repaint();
        });
    }

    fn page_installing(&mut self, ui: &mut Ui) {
        let l = self.lang;
        let (step, total, text, finished, error) = {
            let p = self.progress.lock().unwrap();
            (p.step, p.total.max(1), p.text.clone(), p.finished, p.error.clone())
        };
        if finished {
            self.page = if error.is_some() { Page::Failed } else { Page::Done };
        }
        ui.label(bold(l.tr("Installing SwTsM…", "Установка SwTsM…", "Встановлення SwTsM…"), 18.0));
        ui.add_space(14.0);
        ui.add(egui::ProgressBar::new(step as f32 / total as f32).fill(ACCENT).animate(!finished));
        ui.add_space(6.0);
        ui.label(RichText::new(text).color(WEAK));
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }

    fn page_done(&mut self, ui: &mut Ui) {
        let l = self.lang;
        ui.horizontal(|ui| {
            ui.label(RichText::new("✔").size(28.0).color(GOOD));
            ui.label(bold(l.tr("SwTsM is installed!", "SwTsM установлен!", "SwTsM встановлено!"), 20.0));
        });
        ui.add_space(6.0);
        ui.label(RichText::new(format!("{}: {}", l.tr("Location", "Папка", "Тека"), self.path)).color(WEAK));
        if self.hotkey && self.admin {
            ui.label(RichText::new(l.tr("Ctrl+Shift+Esc now opens SwTsM.", "Теперь Ctrl+Shift+Esc открывает SwTsM.", "Тепер Ctrl+Shift+Esc відкриває SwTsM.")).color(WEAK));
        }
        ui.label(
            RichText::new(l.tr(
                "You can uninstall it from the app settings or from Windows “Apps”.",
                "Удалить можно в настройках приложения или через «Приложения» Windows.",
                "Видалити можна в налаштуваннях програми або через «Програми» Windows.",
            ))
            .color(WEAK),
        );
        let warnings = self.progress.lock().map(|p| p.warnings.clone()).unwrap_or_default();
        if !warnings.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(l.tr("Warnings:", "Предупреждения:", "Попередження:")).color(Color32::from_rgb(255, 210, 140)));
            for w in warnings {
                ui.label(RichText::new(w).size(12.0).color(WEAK));
            }
        }
        ui.add_space(10.0);
        ui.checkbox(&mut self.launch, l.tr("Launch SwTsM now", "Запустить SwTsM сейчас", "Запустити SwTsM зараз"));
    }

    fn page_failed(&mut self, ui: &mut Ui) {
        let l = self.lang;
        let err = self.progress.lock().ok().and_then(|p| p.error.clone()).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(RichText::new("✖").size(28.0).color(BAD));
            ui.label(bold(l.tr("Installation failed", "Ошибка установки", "Помилка встановлення"), 20.0));
        });
        ui.add_space(6.0);
        ui.label(RichText::new(err).color(WEAK));
        if !self.admin {
            ui.label(RichText::new(l.tr(
                "Try choosing another folder or run setup as administrator.",
                "Попробуйте выбрать другую папку или запустить установщик от имени администратора.",
                "Спробуйте обрати іншу теку або запустити інсталятор від імені адміністратора.",
            )).color(WEAK));
        }
    }
}

impl eframe::App for Setup {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let l = self.lang;
        egui::TopBottomPanel::top("head")
            .frame(Frame::none().fill(Color32::from_rgb(16, 17, 21)).inner_margin(Margin::symmetric(22.0, 16.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    logo(ui, 40.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(bold("SwTsM", 20.0));
                        ui.label(RichText::new(format!("{} {VERSION} · Made by MrSnowman", l.tr("Setup", "Установка", "Встановлення"))).size(12.5).color(WEAK));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.admin {
                            ui.label(RichText::new(l.tr("🛡 Administrator", "🛡 Администратор", "🛡 Адміністратор")).size(12.5).color(GOOD));
                        }
                    });
                });
            });

        egui::TopBottomPanel::bottom("foot")
            .frame(Frame::none().fill(Color32::from_rgb(16, 17, 21)).inner_margin(Margin::symmetric(22.0, 12.0)))
            .show(ctx, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    match self.page {
                        Page::Language => {
                            if ui.add(Button::new(bold(l.tr("Next  →", "Далее  →", "Далі  →"), 14.0)).fill(ACCENT.gamma_multiply(0.6))).clicked() {
                                self.page = Page::Location;
                            }
                            if ui.button(l.tr("Cancel", "Отмена", "Скасувати")).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        Page::Location => {
                            let ok = !self.path.trim().is_empty();
                            if ui
                                .add_enabled(ok, Button::new(bold(l.tr("Install", "Установить", "Встановити"), 14.0)).fill(ACCENT.gamma_multiply(0.6)))
                                .clicked()
                            {
                                self.start_install(ctx);
                            }
                            if ui.button(l.tr("←  Back", "←  Назад", "←  Назад")).clicked() {
                                self.page = Page::Language;
                            }
                        }
                        Page::Installing => {
                            ui.add_enabled(false, Button::new(l.tr("Please wait…", "Подождите…", "Зачекайте…")));
                        }
                        Page::Done => {
                            if ui.add(Button::new(bold(l.tr("Finish", "Готово", "Готово"), 14.0)).fill(ACCENT.gamma_multiply(0.6))).clicked() {
                                if self.launch {
                                    sys::launch_unelevated(&PathBuf::from(self.path.trim()).join("SwTsM.exe"));
                                }
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        Page::Failed => {
                            if ui.button(l.tr("Close", "Закрыть", "Закрити")).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            if ui.button(l.tr("←  Back", "←  Назад", "←  Назад")).clicked() {
                                *self.progress.lock().unwrap() = Progress::default();
                                self.page = Page::Location;
                            }
                        }
                    }
                });
            });

        egui::CentralPanel::default().frame(Frame::none().fill(BG).inner_margin(Margin::symmetric(26.0, 18.0))).show(ctx, |ui| {
            self.steps_bar(ui);
            ui.add_space(16.0);
            match self.page {
                Page::Language => self.page_language(ui),
                Page::Location => self.page_location(ui),
                Page::Installing => self.page_installing(ui),
                Page::Done => self.page_done(ui),
                Page::Failed => self.page_failed(ui),
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Тихая установка: SwTsM-Setup.exe --silent [--dir=PATH] [--lang=En|Ru|Uk] [--no-shortcuts] [--hotkey]
    if args.iter().any(|a| a == "--silent") {
        let get = |k: &str| args.iter().find_map(|a| a.strip_prefix(k).map(|s| s.to_string()));
        let admin = sys::is_admin();
        let lang = match get("--lang=").as_deref() {
            Some("Ru") | Some("ru") => Lang::Ru,
            Some("Uk") | Some("uk") => Lang::Uk,
            _ => Lang::En,
        };
        let shortcuts = !args.iter().any(|a| a == "--no-shortcuts");
        let opts = Opts {
            dir: PathBuf::from(get("--dir=").unwrap_or_else(|| default_dir(admin))),
            desktop: shortcuts,
            start_menu: shortcuts,
            hotkey: args.iter().any(|a| a == "--hotkey"),
            lang,
            admin,
        };
        let r = install(opts, Arc::new(Mutex::new(Progress::default())));
        std::process::exit(if r.is_ok() { 0 } else { 1 });
    }

    // Сначала — права администратора (если нужно): запрос UAC.
    // Если пользователь отказал, установка продолжится для текущего пользователя.
    if !sys::is_admin() && !args.iter().any(|a| a == "--elevated" || a == "--no-elevate") && sys::relaunch_elevated() {
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([640.0, 470.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(icon_data()),
        ..Default::default()
    };
    eframe::run_native(TITLE, options, Box::new(|cc| Ok(Box::new(Setup::new(cc)))))
}
