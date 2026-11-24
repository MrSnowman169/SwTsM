use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::i18n::{tr, Lang};
use crate::theme::{TextColor, ThemeId};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum BlurMode {
    Acrylic,
    Blur,
    Clear,
}

impl BlurMode {
    pub const ALL: [BlurMode; 3] = [BlurMode::Blur, BlurMode::Acrylic, BlurMode::Clear];
    pub fn name(self) -> &'static str {
        match self {
            BlurMode::Acrylic => tr("Acrylic (dense frosted glass)", "Акрил (плотное матовое стекло)", "Акрил (щільне матове скло)"),
            BlurMode::Blur => tr("Blur (clear glass)", "Размытие (прозрачное стекло)", "Розмиття (прозоре скло)"),
            BlurMode::Clear => tr("No blur (transparency only)", "Без размытия (только прозрачность)", "Без розмиття (лише прозорість)"),
        }
    }
}

pub const DEFAULT_BG: [u8; 3] = [30, 31, 36];
pub const DEFAULT_ACCENT: [u8; 3] = [88, 166, 255];

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeId,
    pub glass_opacity: f32,
    pub blur: BlurMode,
    pub text_color: TextColor,
    pub custom_bg: [u8; 3],
    pub custom_accent: [u8; 3],
    pub ui_scale: f32,
    pub lang: Lang,
    pub refresh_ms: u64,
    pub paused: bool,
    pub always_on_top: bool,
    pub group_by_name: bool,
    pub per_core: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeId::Default,
            glass_opacity: 0.3,
            blur: BlurMode::Blur,
            text_color: TextColor::Auto,
            custom_bg: DEFAULT_BG,
            custom_accent: DEFAULT_ACCENT,
            ui_scale: 1.0,
            lang: Lang::En,
            refresh_ms: 1000,
            paused: false,
            always_on_top: false,
            group_by_name: true,
            per_core: false,
        }
    }
}

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("SwTsM").join("settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(s) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(p, s);
            }
        }
    }
}
