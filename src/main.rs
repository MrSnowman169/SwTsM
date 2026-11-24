#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod i18n;
mod monitor;
mod settings;
mod theme;
mod util;
mod win;

use eframe::egui;

pub const APP_TITLE: &str = "SwTsM";

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a.eq_ignore_ascii_case(flag));

    // Служебные режимы (запускаются с правами администратора через UAC).
    if has("--install-hook") {
        std::process::exit(if win::install_hook().is_ok() { 0 } else { 1 });
    }
    if has("--uninstall-hook") {
        std::process::exit(if win::uninstall_hook().is_ok() { 0 } else { 1 });
    }
    // Полное удаление (кнопка в приложении или «Программы» Windows).
    if has("--uninstall") {
        let quiet = has("--quiet");
        if !win::is_admin() && !has("--elevated") {
            let args = if quiet { "--uninstall --elevated --quiet" } else { "--uninstall --elevated" };
            if win::run_self_elevated(args) {
                return Ok(());
            }
        }
        i18n::set(settings::Settings::load().lang);
        let r = win::uninstall_self();
        if !quiet {
            let text = match &r {
                Ok(_) => i18n::tr("SwTsM has been uninstalled.", "SwTsM удалён.", "SwTsM видалено.").to_string(),
                Err(e) => format!("{}: {e}", i18n::tr("Uninstall error", "Ошибка удаления", "Помилка видалення")),
            };
            win::message_box("SwTsM", &text);
        }
        std::process::exit(if r.is_ok() { 0 } else { 1 });
    }

    // Один экземпляр: повторный запуск (например, по Ctrl+Shift+Esc)
    // просто выводит уже открытое окно на передний план.
    if has("--relaunch") {
        std::thread::sleep(std::time::Duration::from_millis(400));
        win::acquire_single_instance();
    } else if !win::acquire_single_instance() {
        win::activate_existing(APP_TITLE);
        return Ok(());
    }

    let settings = settings::Settings::load();
    i18n::set(settings.lang);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_inner_size([1120.0, 740.0])
            .with_min_inner_size([780.0, 480.0])
            .with_transparent(true)
            .with_icon(util::app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(move |cc| Ok(Box::new(app::TaskApp::new(cc, settings)))),
    )
}
