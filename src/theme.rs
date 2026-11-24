use eframe::egui::{self, epaint::Shadow, Color32, Rounding, Stroke, Visuals};
use serde::{Deserialize, Serialize};

use crate::i18n::tr;
use crate::settings::Settings;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Hash)]
pub enum ThemeId {
    #[serde(alias = "GlassDark", alias = "GlassLight")]
    Glass,
    #[serde(alias = "Dark", alias = "Light")]
    Default,
    Neon,
    Nord,
    Telegram,
    Discord,
}

impl ThemeId {
    pub const ALL: [ThemeId; 6] = [
        ThemeId::Glass,
        ThemeId::Default,
        ThemeId::Neon,
        ThemeId::Nord,
        ThemeId::Telegram,
        ThemeId::Discord,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ThemeId::Glass => "Liquid Glass",
            ThemeId::Default => "Default",
            ThemeId::Neon => "Neon",
            ThemeId::Nord => "Nord",
            ThemeId::Telegram => "Like-Telegram",
            ThemeId::Discord => "Like-Discord",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Hash)]
pub enum TextColor {
    Auto,
    Black,
    Grey,
    White,
}

impl TextColor {
    pub const ALL: [TextColor; 4] = [TextColor::Auto, TextColor::Black, TextColor::Grey, TextColor::White];
    pub fn label(self) -> &'static str {
        match self {
            TextColor::Auto => tr("Auto", "Авто", "Авто"),
            TextColor::Black => tr("Black", "Чёрный", "Чорний"),
            TextColor::Grey => tr("Grey", "Серый", "Сірий"),
            TextColor::White => tr("White", "Белый", "Білий"),
        }
    }
    /// (основной, второстепенный)
    fn colors(self) -> Option<(Color32, Color32)> {
        match self {
            TextColor::Auto => None,
            TextColor::Black => Some((rgb(12, 12, 16), rgb(40, 45, 58))),
            TextColor::Grey => Some((rgb(150, 154, 162), rgb(120, 124, 134))),
            TextColor::White => Some((rgb(246, 247, 250), rgb(200, 205, 215))),
        }
    }
}

#[derive(Clone)]
pub struct Theme {
    pub dark: bool,
    pub glass: bool,
    pub window_bg: Color32,
    pub card: Color32,
    pub card_stroke: Color32,
    pub popup: Color32,
    pub input_bg: Color32,
    pub stripe: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub accent: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub sheen: Color32,
    pub graph_bg: Color32,
    pub grid: Color32,
    pub cpu: Color32,
    pub mem: Color32,
    pub disk: Color32,
    pub net: Color32,
    pub danger: Color32,
    pub rounding: f32,
}

pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn rgb3(c: [u8; 3]) -> Color32 {
    rgb(c[0], c[1], c[2])
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn lum(c: Color32) -> f32 {
    (0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32) / 255.0
}

/// Полупрозрачный слой, контрастный к фону.
fn ov(dark_bg: bool, a: u8) -> Color32 {
    if dark_bg {
        rgba(255, 255, 255, a)
    } else {
        rgba(0, 0, 0, a)
    }
}

fn alpha(c: Color32, a: u8) -> Color32 {
    rgba(c.r(), c.g(), c.b(), a)
}

struct Solid {
    bg: Color32,
    card: Color32,
    stroke: Color32,
    popup: Color32,
    input: Color32,
    text: Color32,
    weak: Color32,
    accent: Color32,
    cpu: Color32,
    mem: Color32,
    disk: Color32,
    net: Color32,
    danger: Color32,
    rounding: f32,
}

fn from_solid(s: Solid) -> Theme {
    let dark = lum(s.bg) < 0.5;
    Theme {
        dark,
        glass: false,
        window_bg: s.bg,
        card: s.card,
        card_stroke: s.stroke,
        popup: s.popup,
        input_bg: s.input,
        stripe: ov(dark, 6),
        text: s.text,
        text_weak: s.weak,
        accent: s.accent,
        hover: ov(dark, if dark { 16 } else { 12 }),
        selected: alpha(s.accent, if dark { 60 } else { 45 }),
        sheen: Color32::TRANSPARENT,
        graph_bg: if dark { mix(s.card, Color32::BLACK, 0.25) } else { mix(s.card, Color32::WHITE, 0.5) },
        grid: ov(dark, 20),
        cpu: s.cpu,
        mem: s.mem,
        disk: s.disk,
        net: s.net,
        danger: s.danger,
        rounding: s.rounding,
    }
}

fn glass(s: &Settings) -> Theme {
    let op = s.glass_opacity.clamp(0.0, 1.0);
    // светлый текст, когда чёрная тонировка достаточно плотная (или выбран явно)
    let light = match s.text_color {
        TextColor::White => true,
        TextColor::Black => false,
        TextColor::Auto | TextColor::Grey => op >= 0.35,
    };
    let (text, weak) = s.text_color.colors().unwrap_or(if light {
        (rgb(246, 247, 250), rgb(200, 205, 215))
    } else {
        (rgb(10, 12, 20), rgb(35, 40, 52))
    });
    let (accent, cpu, mem, disk, net, danger) = if light {
        (rgb(122, 205, 255), rgb(96, 198, 255), rgb(196, 146, 255), rgb(110, 231, 160), rgb(255, 184, 92), rgb(255, 110, 110))
    } else {
        (rgb(0, 106, 220), rgb(0, 120, 215), rgb(140, 70, 220), rgb(20, 150, 80), rgb(220, 120, 0), rgb(210, 40, 40))
    };
    Theme {
        dark: light,
        glass: true,
        // Тонировка чёрная: на максимуме окно становится тёмным, а не белым.
        window_bg: rgba(0, 0, 0, (op * 250.0) as u8),
        card: rgba(255, 255, 255, 6),
        card_stroke: rgba(255, 255, 255, 60),
        popup: if light { rgba(24, 26, 34, 245) } else { rgba(248, 250, 255, 245) },
        input_bg: rgba(0, 0, 0, 30),
        stripe: ov(light, 6),
        text,
        text_weak: weak,
        accent,
        hover: ov(light, 10),
        selected: alpha(accent, 30),
        sheen: rgba(255, 255, 255, 7),
        graph_bg: rgba(0, 0, 0, 30),
        grid: ov(light, 22),
        cpu,
        mem,
        disk,
        net,
        danger,
        rounding: 14.0,
    }
}

impl Theme {
    pub fn build(s: &Settings) -> Theme {
        if s.theme == ThemeId::Glass {
            return glass(s);
        }
        let mut t = match s.theme {
            ThemeId::Glass | ThemeId::Default => {
                let bg = rgb3(s.custom_bg);
                let accent = rgb3(s.custom_accent);
                let dark = lum(bg) < 0.5;
                let c = if dark { Color32::WHITE } else { Color32::BLACK };
                let text = if dark { rgb(236, 238, 242) } else { rgb(22, 24, 30) };
                from_solid(Solid {
                    bg,
                    card: if dark { mix(bg, Color32::WHITE, 0.05) } else { mix(bg, Color32::WHITE, 0.65) },
                    stroke: mix(bg, c, 0.13),
                    popup: if dark { mix(bg, Color32::WHITE, 0.08) } else { mix(bg, Color32::WHITE, 0.8) },
                    input: if dark { mix(bg, Color32::BLACK, 0.2) } else { mix(bg, Color32::WHITE, 0.4) },
                    text,
                    weak: mix(text, bg, 0.38),
                    accent,
                    cpu: accent,
                    mem: if dark { rgb(167, 139, 250) } else { rgb(139, 18, 174) },
                    disk: if dark { rgb(74, 222, 128) } else { rgb(77, 166, 12) },
                    net: if dark { rgb(251, 191, 36) } else { rgb(167, 79, 1) },
                    danger: if dark { rgb(248, 113, 113) } else { rgb(196, 43, 28) },
                    rounding: 8.0,
                })
            }
            ThemeId::Neon => from_solid(Solid {
                bg: rgb(9, 5, 20),
                card: rgb(20, 12, 38),
                stroke: rgb(88, 40, 160),
                popup: rgb(30, 18, 56),
                input: rgb(12, 6, 26),
                text: rgb(238, 228, 255),
                weak: rgb(170, 150, 210),
                accent: rgb(255, 60, 200),
                cpu: rgb(0, 255, 220),
                mem: rgb(255, 60, 200),
                disk: rgb(255, 230, 0),
                net: rgb(120, 130, 255),
                danger: rgb(255, 80, 80),
                rounding: 6.0,
            }),
            ThemeId::Nord => from_solid(Solid {
                bg: rgb(46, 52, 64),
                card: rgb(59, 66, 82),
                stroke: rgb(76, 86, 106),
                popup: rgb(67, 76, 94),
                input: rgb(46, 52, 64),
                text: rgb(236, 239, 244),
                weak: rgb(196, 204, 218),
                accent: rgb(136, 192, 208),
                cpu: rgb(136, 192, 208),
                mem: rgb(180, 142, 173),
                disk: rgb(163, 190, 140),
                net: rgb(235, 203, 139),
                danger: rgb(191, 97, 106),
                rounding: 10.0,
            }),
            ThemeId::Telegram => from_solid(Solid {
                bg: rgb(14, 22, 33),
                card: rgb(23, 33, 43),
                stroke: rgb(36, 47, 61),
                popup: rgb(36, 47, 61),
                input: rgb(24, 37, 51),
                text: rgb(245, 245, 245),
                weak: rgb(112, 132, 153),
                accent: rgb(100, 181, 239),
                cpu: rgb(100, 181, 239),
                mem: rgb(170, 130, 230),
                disk: rgb(80, 200, 120),
                net: rgb(240, 180, 80),
                danger: rgb(230, 90, 90),
                rounding: 12.0,
            }),
            ThemeId::Discord => from_solid(Solid {
                bg: rgb(30, 31, 34),
                card: rgb(49, 51, 56),
                stroke: rgb(63, 65, 71),
                popup: rgb(17, 18, 20),
                input: rgb(30, 31, 34),
                text: rgb(219, 222, 225),
                weak: rgb(148, 155, 164),
                accent: rgb(88, 101, 242),
                cpu: rgb(88, 101, 242),
                mem: rgb(235, 69, 158),
                disk: rgb(35, 165, 90),
                net: rgb(240, 178, 50),
                danger: rgb(242, 63, 67),
                rounding: 8.0,
            }),
        };
        if let Some((text, weak)) = s.text_color.colors() {
            t.text = text;
            t.weak_from(weak);
        }
        t
    }

    fn weak_from(&mut self, weak: Color32) {
        self.text_weak = weak;
    }

    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = if self.dark { Visuals::dark() } else { Visuals::light() };
        v.override_text_color = Some(self.text);
        v.panel_fill = self.window_bg;
        v.window_fill = self.popup;
        v.extreme_bg_color = self.input_bg;
        v.faint_bg_color = self.stripe;
        v.code_bg_color = self.input_bg;
        v.hyperlink_color = self.accent;
        v.selection.bg_fill = self.selected;
        v.selection.stroke = Stroke::new(1.0_f32, self.accent);

        let r = Rounding::same((self.rounding * 0.6).max(4.0));
        let shadow_alpha = if self.dark { 110 } else { 40 };
        v.window_rounding = Rounding::same(self.rounding);
        v.menu_rounding = r;
        v.window_stroke = Stroke::new(1.0_f32, self.card_stroke);
        v.window_shadow = Shadow {
            offset: egui::vec2(0.0, 10.0),
            blur: 28.0,
            spread: 0.0,
            color: Color32::from_black_alpha(shadow_alpha),
        };
        v.popup_shadow = Shadow {
            offset: egui::vec2(0.0, 6.0),
            blur: 16.0,
            spread: 0.0,
            color: Color32::from_black_alpha(shadow_alpha),
        };

        let btn = if self.glass { self.hover } else { self.card };
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.card;
        w.noninteractive.weak_bg_fill = self.card;
        w.noninteractive.bg_stroke = Stroke::new(1.0_f32, self.card_stroke);
        w.noninteractive.fg_stroke = Stroke::new(1.0_f32, self.text);
        w.noninteractive.rounding = r;

        w.inactive.bg_fill = self.input_bg;
        w.inactive.weak_bg_fill = btn;
        w.inactive.bg_stroke = if self.glass && !self.dark {
            Stroke::new(1.0_f32, rgba(0, 0, 0, 60))
        } else {
            Stroke::new(1.0_f32, self.card_stroke)
        };
        w.inactive.fg_stroke = Stroke::new(1.0_f32, self.text);
        w.inactive.rounding = r;

        w.hovered.bg_fill = self.hover;
        w.hovered.weak_bg_fill = self.hover;
        w.hovered.bg_stroke = Stroke::new(1.0_f32, self.accent.gamma_multiply(0.7));
        w.hovered.fg_stroke = Stroke::new(1.5_f32, self.text);
        w.hovered.rounding = r;

        w.active.bg_fill = self.selected;
        w.active.weak_bg_fill = self.selected;
        w.active.bg_stroke = Stroke::new(1.0_f32, self.accent);
        w.active.fg_stroke = Stroke::new(2.0_f32, self.text);
        w.active.rounding = r;

        w.open.bg_fill = self.selected;
        w.open.weak_bg_fill = self.selected;
        w.open.bg_stroke = Stroke::new(1.0_f32, self.accent);
        w.open.fg_stroke = Stroke::new(1.0_f32, self.text);
        w.open.rounding = r;

        ctx.set_visuals(v);

        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.menu_margin = egui::Margin::same(8.0);
        style.spacing.window_margin = egui::Margin::same(16.0);
        ctx.set_style(style);
    }

    pub fn card_frame(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.card)
            .stroke(Stroke::new(1.0_f32, self.card_stroke))
            .rounding(Rounding::same(self.rounding))
            .inner_margin(egui::Margin::same(14.0))
    }

    /// ABGR-тон для акрила Windows: чёрный, плотность от настройки.
    pub fn acrylic_tint(&self, opacity: f32) -> u32 {
        let a = (16.0 + opacity.clamp(0.0, 1.0) * 144.0) as u32;
        a << 24
    }
}

/// Карточка в стиле «жидкого стекла»: полупрозрачная заливка, светлая кромка
/// и мягкий блик в верхней части.
pub fn card<R>(ui: &mut egui::Ui, t: &Theme, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let resp = t.card_frame().show(ui, |ui| {
        let idx = ui.painter().add(egui::Shape::Noop);
        (idx, add(ui))
    });
    let (idx, ret) = resp.inner;
    if t.glass {
        let rect = resp.response.rect.shrink(1.0);
        let r = t.rounding - 1.0;
        let max_h = (rect.height() * 0.4).min(150.0);
        let inset = r * 0.35;
        let g = egui::Rect::from_min_max(
            egui::pos2(rect.left() + inset, rect.top() + inset * 0.6),
            egui::pos2(rect.right() - inset, rect.top() + max_h),
        );
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(g.left_top(), t.sheen);
        mesh.colored_vertex(g.right_top(), t.sheen);
        mesh.colored_vertex(g.right_bottom(), Color32::TRANSPARENT);
        mesh.colored_vertex(g.left_bottom(), Color32::TRANSPARENT);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        let shapes = vec![
            egui::Shape::mesh(mesh),
            egui::Shape::line_segment(
                [
                    egui::pos2(rect.left() + r, rect.top() + 0.5),
                    egui::pos2(rect.right() - r, rect.top() + 0.5),
                ],
                Stroke::new(1.0_f32, t.card_stroke),
            ),
        ];
        ui.painter().set(idx, egui::Shape::Vec(shapes));
    }
    ret
}
