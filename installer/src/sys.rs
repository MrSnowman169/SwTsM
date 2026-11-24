//! Windows-часть инсталлера.

use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::UI::Shell::{
    IsUserAnAdmin, SHBrowseForFolderW, SHGetPathFromIDListW, ShellExecuteW, BROWSEINFOW,
};
use winreg::enums::*;
use winreg::RegKey;

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub const IFEO: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\taskmgr.exe";
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\SwTsM";

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn shell_execute(verb: &str, file: &str, params: &str, show: i32) -> Result<(), i32> {
    let (v, f, p) = (wide(verb), wide(file), wide(params));
    let r = unsafe {
        ShellExecuteW(
            0 as HWND,
            v.as_ptr(),
            f.as_ptr(),
            if params.is_empty() { null() } else { p.as_ptr() },
            null(),
            show,
        )
    } as isize;
    if r > 32 {
        Ok(())
    } else {
        Err(r as i32)
    }
}

/// Перезапуск инсталлера с правами администратора (запрос UAC).
pub fn relaunch_elevated() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    shell_execute("runas", &exe.display().to_string(), "--elevated", 1).is_ok()
}

/// Стандартный диалог выбора папки (вызывать из отдельного потока).
pub fn browse_folder(title: &str) -> Option<String> {
    unsafe {
        CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
        let t = wide(title);
        let mut name = [0u16; 260];
        let bi = BROWSEINFOW {
            hwndOwner: 0 as HWND,
            pidlRoot: null_mut(),
            pszDisplayName: name.as_mut_ptr(),
            lpszTitle: t.as_ptr(),
            ulFlags: 0x0001 | 0x0040, // BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE
            lpfn: None,
            lParam: 0,
            iImage: 0,
        };
        let pidl = SHBrowseForFolderW(&bi);
        if pidl.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let ok = SHGetPathFromIDListW(pidl, buf.as_mut_ptr());
        CoTaskMemFree(pidl as *const c_void);
        if ok == 0 {
            return None;
        }
        let len = buf.iter().position(|c| *c == 0).unwrap_or(0);
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

fn run_hidden(exe: &str, args: &[&str]) -> std::io::Result<std::process::ExitStatus> {
    std::process::Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).status()
}

pub fn kill_running_app() {
    let _ = run_hidden("taskkill.exe", &["/F", "/IM", "SwTsM.exe"]);
}

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Создаёт ярлыки через WScript.Shell.
pub fn create_shortcuts(exe: &Path, desktop: bool, start_menu: bool, all_users: bool) -> Result<(), String> {
    let exe_s = exe.display().to_string();
    let dir_s = exe.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let mut script = String::from("$ws = New-Object -ComObject WScript.Shell;\n");
    let mut add = |folder_expr: &str| {
        script.push_str(&format!(
            "$l = $ws.CreateShortcut((Join-Path ({folder_expr}) 'SwTsM.lnk')); $l.TargetPath = {exe}; $l.WorkingDirectory = {dir}; $l.IconLocation = {exe} + ',0'; $l.Description = 'SwTsM Task Manager'; $l.Save();\n",
            exe = ps_quote(&exe_s),
            dir = ps_quote(&dir_s),
        ));
    };
    if desktop {
        add("[Environment]::GetFolderPath('Desktop')");
    }
    if start_menu {
        add(if all_users {
            "[Environment]::GetFolderPath('CommonPrograms')"
        } else {
            "[Environment]::GetFolderPath('Programs')"
        });
    }
    if !desktop && !start_menu {
        return Ok(());
    }
    let st = run_hidden(
        "powershell.exe",
        &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script],
    )
    .map_err(|e| e.to_string())?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("powershell exit {:?}", st.code()))
    }
}

pub fn register_uninstall(exe: &Path, version: &str, size_kb: u32, all_users: bool) -> std::io::Result<()> {
    let root = RegKey::predef(if all_users { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER });
    let (k, _) = root.create_subkey(UNINSTALL_KEY)?;
    let exe_s = exe.display().to_string();
    k.set_value("DisplayName", &"SwTsM")?;
    k.set_value("DisplayVersion", &version)?;
    k.set_value("Publisher", &"MrSnowman")?;
    k.set_value("DisplayIcon", &format!("\"{exe_s}\",0"))?;
    k.set_value("InstallLocation", &exe.parent().map(|p| p.display().to_string()).unwrap_or_default())?;
    k.set_value("UninstallString", &format!("\"{exe_s}\" --uninstall"))?;
    k.set_value("QuietUninstallString", &format!("\"{exe_s}\" --uninstall --quiet"))?;
    k.set_value("NoModify", &1u32)?;
    k.set_value("NoRepair", &1u32)?;
    k.set_value("EstimatedSize", &size_kb)?;
    Ok(())
}

pub fn install_hook(exe: &Path) -> std::io::Result<()> {
    let (key, _) = RegKey::predef(HKEY_LOCAL_MACHINE).create_subkey(IFEO)?;
    key.set_value("Debugger", &format!("\"{}\"", exe.display()))
}

/// Записывает выбранный язык в настройки приложения (не трогая остальное).
pub fn save_language(code: &str) -> Result<(), String> {
    let base = std::env::var_os("APPDATA").ok_or("APPDATA not set")?;
    let dir = Path::new(&base).join("SwTsM");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("settings.json");
    let mut v: serde_json::Value = std::fs::read_to_string(&file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !v.is_object() {
        v = serde_json::json!({});
    }
    v["lang"] = serde_json::Value::String(code.to_string());
    std::fs::write(&file, serde_json::to_string_pretty(&v).unwrap()).map_err(|e| e.to_string())
}

/// Запуск без прав администратора (через проводник).
pub fn launch_unelevated(exe: &Path) {
    let _ = std::process::Command::new("explorer.exe").arg(exe).spawn();
}
