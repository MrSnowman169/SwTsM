use eframe::egui::{self, Color32};

use crate::i18n::{self, Lang};

pub fn fmt_bytes(b: f64) -> String {
    let u: [&str; 5] = match i18n::get() {
        Lang::En => ["B", "KB", "MB", "GB", "TB"],
        _ => ["Б", "КБ", "МБ", "ГБ", "ТБ"],
    };
    #[allow(non_snake_case)]
    let U = u;
    let mut v = b.max(0.0);
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{:.0} {}", v, U[i])
    } else {
        format!("{:.1} {}", v, U[i])
    }
}

pub fn fmt_rate(bps: f64) -> String {
    let s = if i18n::get() == Lang::En { "/s" } else { "/с" };
    format!("{}{s}", fmt_bytes(bps))
}

pub fn fmt_bits(bytes_per_s: f64) -> String {
    #[allow(non_snake_case)]
    let U: [&str; 4] = match i18n::get() {
        Lang::En => ["bps", "Kbps", "Mbps", "Gbps"],
        Lang::Ru => ["бит/с", "Кбит/с", "Мбит/с", "Гбит/с"],
        Lang::Uk => ["біт/с", "Кбіт/с", "Мбіт/с", "Гбіт/с"],
    };
    let mut v = bytes_per_s.max(0.0) * 8.0;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    format!("{:.1} {}", v, U[i])
}

pub fn fmt_duration(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        let dl = if i18n::get() == Lang::En { "d" } else { "д" };
        format!("{d} {dl} {h:02}:{m:02}:{s:02}")
    } else {
        format!("{h}:{m:02}:{s:02}")
    }
}

pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

pub fn name_color(name: &str) -> Color32 {
    let mut h: u32 = 2166136261;
    for b in name.to_lowercase().bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    let hue = (h % 360) as f32 / 360.0;
    egui::ecolor::Hsva::new(hue, 0.55, 0.78, 1.0).into()
}

pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Простейшее раскрытие %VAR% в путях.
pub fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('%') {
            let var = &after[..end];
            match std::env::var(var) {
                Ok(v) if !var.is_empty() => out.push_str(&v),
                _ => {
                    out.push('%');
                    out.push_str(var);
                    out.push('%');
                }
            }
            rest = &after[end + 1..];
        } else {
            out.push('%');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Извлекает путь к исполняемому файлу из командной строки.
pub fn exe_from_command(cmd: &str) -> String {
    let cmd = expand_env(cmd.trim());
    if std::path::Path::new(&cmd).is_file() {
        return cmd;
    }
    if let Some(stripped) = cmd.strip_prefix('"') {
        return stripped.split('"').next().unwrap_or("").to_string();
    }
    let lower = cmd.to_lowercase();
    if let Some(pos) = lower.find(".exe") {
        return cmd[..pos + 4].to_string();
    }
    cmd.split_whitespace().next().unwrap_or("").to_string()
}

/// Иконка окна: стеклянный скруглённый квадрат с «пульсом».
pub fn app_icon() -> egui::IconData {
    let n = 64usize;
    let mut rgba = vec![0u8; n * n * 4];
    let radius = 14.0f32;
    for y in 0..n {
        for x in 0..n {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            // расстояние до скруглённого прямоугольника
            let cx = fx.clamp(radius, n as f32 - radius);
            let cy = fy.clamp(radius, n as f32 - radius);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let a = (radius - d + 0.5).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            // чёрный логотип с лёгким вертикальным градиентом
            let t = fy / n as f32;
            let r = (34.0 - 24.0 * t) as u8;
            let g = r;
            let b = (38.0 - 24.0 * t) as u8;
            let i = (y * n + x) * 4;
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = (a * 255.0) as u8;
        }
    }
    // линия «пульса»
    let pts = [(8, 36), (22, 36), (28, 20), (36, 48), (42, 30), (56, 30)];
    for w in pts.windows(2) {
        let (x0, y0) = (w[0].0 as f32, w[0].1 as f32);
        let (x1, y1) = (w[1].0 as f32, w[1].1 as f32);
        let steps = 80;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let px = x0 + (x1 - x0) * t;
            let py = y0 + (y1 - y0) * t;
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    if dx * dx + dy * dy > 5 {
                        continue;
                    }
                    let xx = (px as i32 + dx).clamp(0, n as i32 - 1) as usize;
                    let yy = (py as i32 + dy).clamp(0, n as i32 - 1) as usize;
                    let i = (yy * n + xx) * 4;
                    rgba[i] = 255;
                    rgba[i + 1] = 255;
                    rgba[i + 2] = 255;
                    rgba[i + 3] = 255;
                }
            }
        }
    }
    egui::IconData { rgba, width: n as u32, height: n as u32 }
}
