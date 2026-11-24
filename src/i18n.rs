//! Простая локализация: английский (по умолчанию), русский, украинский.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Lang {
    En,
    Ru,
    Uk,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Ru, Lang::Uk];
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ru => "Русский",
            Lang::Uk => "Українська",
        }
    }
}

static CUR: AtomicU8 = AtomicU8::new(0);

pub fn set(l: Lang) {
    CUR.store(l as u8, Ordering::Relaxed);
}

pub fn get() -> Lang {
    match CUR.load(Ordering::Relaxed) {
        1 => Lang::Ru,
        2 => Lang::Uk,
        _ => Lang::En,
    }
}

/// Выбор строки под текущий язык: (english, русский, українська).
pub fn tr(en: &'static str, ru: &'static str, uk: &'static str) -> &'static str {
    match get() {
        Lang::En => en,
        Lang::Ru => ru,
        Lang::Uk => uk,
    }
}
