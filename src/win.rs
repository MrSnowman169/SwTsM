//! Всё, что специфично для Windows: управление процессами, службы,
//! автозагрузка, эффект стекла и перехват Ctrl+Shift+Esc.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::null;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, BOOL, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_MORE_DATA,
    HANDLE, HWND, LPARAM,
};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::Security::{
    GetTokenInformation, LookupAccountSidW, TokenUser, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, ControlService, EnumServicesStatusExW, OpenSCManagerW, OpenServiceW,
    QueryServiceStatus, StartServiceW, ENUM_SERVICE_STATUS_PROCESSW, SC_ENUM_PROCESS_INFO,
    SERVICE_STATUS,
};
use windows_sys::Win32::System::Threading::{
    CreateMutexW, GetPriorityClass, OpenProcess, OpenProcessToken, SetPriorityClass,
    TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    PROCESS_SUSPEND_RESUME, PROCESS_TERMINATE,
};
use windows_sys::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    SetForegroundWindow, ShowWindow, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNORMAL,
};

use windows_sys::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
};
use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

use crate::i18n::tr;
use winreg::enums::*;

// ───────────────────────────── Иконки ─────────────────────────────

/// Вызывается один раз в потоке, который извлекает иконки (нужен COM для оболочки).
pub fn init_icon_thread() {
    unsafe {
        CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
    }
}

/// Иконка файла (32×32) в RGBA без премультипликации.
pub fn file_icon_rgba(path: &str) -> Option<(usize, usize, Vec<u8>)> {
    unsafe {
        let mut sfi: SHFILEINFOW = std::mem::zeroed();
        let p = wide(path);
        let r = SHGetFileInfoW(
            p.as_ptr(),
            0,
            &mut sfi,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if r == 0 || (sfi.hIcon as isize) == 0 {
            return None;
        }
        let res = hicon_to_rgba(sfi.hIcon as isize);
        DestroyIcon(sfi.hIcon);
        res
    }
}

unsafe fn dib_bits(hbm: isize, w: i32, h: i32) -> Option<Vec<u8>> {
    let mut bi: BITMAPINFO = std::mem::zeroed();
    bi.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: -h, // сверху вниз
        biPlanes: 1,
        biBitCount: 32,
        biCompression: 0, // BI_RGB
        ..std::mem::zeroed()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let dc = GetDC(0 as HWND);
    let n = GetDIBits(dc, hbm as _, 0, h as u32, buf.as_mut_ptr() as *mut c_void, &mut bi, DIB_RGB_COLORS);
    ReleaseDC(0 as HWND, dc);
    if n == 0 {
        None
    } else {
        Some(buf)
    }
}

unsafe fn hicon_to_rgba(hicon: isize) -> Option<(usize, usize, Vec<u8>)> {
    let mut ii: ICONINFO = std::mem::zeroed();
    if GetIconInfo(hicon as _, &mut ii) == 0 {
        return None;
    }
    let color = ii.hbmColor as isize;
    let mask = ii.hbmMask as isize;
    let cleanup = || {
        if color != 0 {
            DeleteObject(color as _);
        }
        if mask != 0 {
            DeleteObject(mask as _);
        }
    };
    if color == 0 {
        cleanup();
        return None;
    }
    let mut bm: BITMAP = std::mem::zeroed();
    if GetObjectW(color as _, std::mem::size_of::<BITMAP>() as i32, &mut bm as *mut BITMAP as *mut c_void) == 0 {
        cleanup();
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    if w <= 0 || h <= 0 || w > 256 || h > 256 {
        cleanup();
        return None;
    }
    let Some(mut px) = dib_bits(color, w, h) else {
        cleanup();
        return None;
    };
    // Старые иконки без альфа-канала: прозрачность берём из маски.
    if !px.chunks_exact(4).any(|p| p[3] != 0) {
        let m = if mask != 0 { dib_bits(mask, w, h) } else { None };
        for (i, p) in px.chunks_exact_mut(4).enumerate() {
            let transparent = m.as_ref().map(|m| m[i * 4] != 0).unwrap_or(false);
            p[3] = if transparent { 0 } else { 255 };
        }
    }
    cleanup();
    for p in px.chunks_exact_mut(4) {
        p.swap(0, 2); // BGRA → RGBA
    }
    Some((w as usize, h as usize, px))
}
use winreg::{RegKey, RegValue};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error() -> String {
    std::io::Error::last_os_error().to_string()
}

fn is_null_handle(h: HANDLE) -> bool {
    (h as isize) == 0
}

// ───────────────────────────── Процессы ─────────────────────────────

pub fn kill(pid: u32) -> Result<(), String> {
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if is_null_handle(h) {
            return Err(last_error());
        }
        let ok = TerminateProcess(h, 1);
        let err = last_error();
        CloseHandle(h);
        if ok == 0 {
            Err(err)
        } else {
            Ok(())
        }
    }
}

pub const PRIORITIES: [u32; 6] = [0x0000_0100, 0x0000_0080, 0x0000_8000, 0x0000_0020, 0x0000_4000, 0x0000_0040];

pub fn priority_name(class: u32) -> &'static str {
    match class {
        0x0000_0100 => tr("Realtime", "Реального времени", "Реального часу"),
        0x0000_0080 => tr("High", "Высокий", "Високий"),
        0x0000_8000 => tr("Above normal", "Выше среднего", "Вище середнього"),
        0x0000_0020 => tr("Normal", "Обычный", "Звичайний"),
        0x0000_4000 => tr("Below normal", "Ниже среднего", "Нижче середнього"),
        0x0000_0040 => tr("Low", "Низкий", "Низький"),
        _ => "?",
    }
}

pub fn get_priority(pid: u32) -> Option<u32> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if is_null_handle(h) {
            return None;
        }
        let p = GetPriorityClass(h);
        CloseHandle(h);
        if p == 0 {
            None
        } else {
            Some(p)
        }
    }
}

pub fn set_priority(pid: u32, class: u32) -> Result<(), String> {
    unsafe {
        let h = OpenProcess(PROCESS_SET_INFORMATION, 0, pid);
        if is_null_handle(h) {
            return Err(last_error());
        }
        let ok = SetPriorityClass(h, class);
        let err = last_error();
        CloseHandle(h);
        if ok == 0 {
            Err(err)
        } else {
            Ok(())
        }
    }
}

type NtProcFn = unsafe extern "system" fn(HANDLE) -> i32;

fn ntdll_fn(name: &[u8]) -> Option<NtProcFn> {
    unsafe {
        let m = GetModuleHandleW(wide("ntdll.dll").as_ptr());
        if (m as isize) == 0 {
            return None;
        }
        GetProcAddress(m, name.as_ptr())
            .map(|f| std::mem::transmute::<unsafe extern "system" fn() -> isize, NtProcFn>(f))
    }
}

fn suspend_resume(pid: u32, suspend: bool) -> Result<(), String> {
    let name: &[u8] = if suspend { b"NtSuspendProcess\0" } else { b"NtResumeProcess\0" };
    let f = ntdll_fn(name).ok_or(tr("ntdll function unavailable", "функция ntdll недоступна", "функція ntdll недоступна"))?;
    unsafe {
        let h = OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid);
        if is_null_handle(h) {
            return Err(last_error());
        }
        let status = f(h);
        CloseHandle(h);
        if status < 0 {
            Err(format!("NTSTATUS 0x{:08X}", status as u32))
        } else {
            Ok(())
        }
    }
}

pub fn suspend(pid: u32) -> Result<(), String> {
    suspend_resume(pid, true)
}

pub fn resume(pid: u32) -> Result<(), String> {
    suspend_resume(pid, false)
}

/// Имя пользователя-владельца процесса (как в стандартном диспетчере).
pub fn process_user(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if is_null_handle(h) {
            return None;
        }
        let mut tok: HANDLE = std::mem::zeroed();
        let ok = OpenProcessToken(h, TOKEN_QUERY, &mut tok);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let mut buf = [0u64; 128];
        let mut len = 0u32;
        let ok = GetTokenInformation(
            tok,
            TokenUser,
            buf.as_mut_ptr() as *mut c_void,
            std::mem::size_of_val(&buf) as u32,
            &mut len,
        );
        CloseHandle(tok);
        if ok == 0 {
            return None;
        }
        let tu = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut name = [0u16; 256];
        let mut nlen = name.len() as u32;
        let mut dom = [0u16; 256];
        let mut dlen = dom.len() as u32;
        let mut use_ = 0;
        if LookupAccountSidW(
            null(),
            tu.User.Sid,
            name.as_mut_ptr(),
            &mut nlen,
            dom.as_mut_ptr(),
            &mut dlen,
            &mut use_,
        ) == 0
        {
            return None;
        }
        Some(String::from_utf16_lossy(&name[..nlen as usize]))
    }
}

// ───────────────────────────── Оболочка ─────────────────────────────

pub fn is_admin() -> bool {
    unsafe { IsUserAnAdmin() != 0 }
}

pub fn shell_execute(verb: &str, file: &str, params: &str, show: i32) -> Result<(), String> {
    let v = wide(verb);
    let f = wide(file);
    let p = wide(params);
    let r = unsafe {
        ShellExecuteW(
            0 as HWND,
            v.as_ptr(),
            f.as_ptr(),
            if params.is_empty() { null() } else { p.as_ptr() },
            null(),
            show,
        )
    };
    let code = r as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(match code {
            2 => tr("file not found", "файл не найден", "файл не знайдено").into(),
            3 => tr("path not found", "путь не найден", "шлях не знайдено").into(),
            5 => tr(
                "access denied (or UAC prompt declined)",
                "отказано в доступе (или запрос UAC отклонён)",
                "доступ заборонено (або запит UAC відхилено)",
            )
            .into(),
            _ => format!("ShellExecute error {code}"),
        })
    }
}

/// «Выполнить» как в диспетчере задач.
pub fn run_task(cmdline: &str, admin: bool) -> Result<(), String> {
    let cmdline = cmdline.trim();
    if cmdline.is_empty() {
        return Err(tr("empty command", "пустая команда", "порожня команда").into());
    }
    let (file, params) = if std::path::Path::new(cmdline).exists() {
        (cmdline.to_string(), String::new())
    } else if let Some(rest) = cmdline.strip_prefix('"') {
        match rest.find('"') {
            Some(end) => (rest[..end].to_string(), rest[end + 1..].trim().to_string()),
            None => (rest.to_string(), String::new()),
        }
    } else {
        match cmdline.find(' ') {
            Some(i) => (cmdline[..i].to_string(), cmdline[i + 1..].trim().to_string()),
            None => (cmdline.to_string(), String::new()),
        }
    };
    shell_execute(if admin { "runas" } else { "open" }, &file, &params, SW_SHOWNORMAL)
}

pub fn open_location(path: &str) {
    use std::os::windows::process::CommandExt;
    if path.is_empty() {
        return;
    }
    let _ = std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path))
        .spawn();
}

pub fn open_url(url: &str) {
    let _ = shell_execute("open", url, "", SW_SHOWNORMAL);
}

pub fn relaunch_as_admin() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    shell_execute("runas", &exe.display().to_string(), "--relaunch", SW_SHOWNORMAL)
}

// ─────────────────────── Окно / один экземпляр ───────────────────────

pub fn acquire_single_instance() -> bool {
    unsafe {
        let name = wide("Local\\SwTsM_SingleInstance_v1");
        let h = CreateMutexW(null(), 0, name.as_ptr());
        let err = GetLastError();
        if is_null_handle(h) {
            return err != ERROR_ACCESS_DENIED;
        }
        // Дескриптор намеренно не закрываем — живёт до конца процесса.
        err != ERROR_ALREADY_EXISTS
    }
}

pub fn activate_existing(title: &str) {
    unsafe {
        let t = wide(title);
        let hwnd = FindWindowW(null(), t.as_ptr());
        if (hwnd as isize) == 0 {
            return;
        }
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
        SetForegroundWindow(hwnd);
    }
}

struct FindState {
    pid: u32,
    title: Vec<u16>,
    found: isize,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let st = &mut *(lparam as *mut FindState);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == st.pid {
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n > 0 && buf[..n as usize] == st.title[..st.title.len() - 1] {
            st.found = hwnd as isize;
            return 0;
        }
    }
    1
}

/// Ищет окно нашего процесса по заголовку. Возвращает HWND как isize.
pub fn find_own_window(title: &str) -> Option<isize> {
    let mut st = FindState { pid: std::process::id(), title: wide(title), found: 0 };
    unsafe {
        EnumWindows(Some(enum_cb), &mut st as *mut FindState as LPARAM);
    }
    if st.found != 0 {
        Some(st.found)
    } else {
        None
    }
}

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    color: u32,
    anim: u32,
}

#[repr(C)]
struct CompAttrData {
    attrib: u32,
    data: *mut c_void,
    size: usize,
}

type SwcaFn = unsafe extern "system" fn(HWND, *mut CompAttrData) -> BOOL;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backdrop {
    None,
    Transparent,
    Blur,
    Acrylic,
}

/// Включает размытие фона окна (Windows 10/11) через SetWindowCompositionAttribute.
pub fn set_backdrop(hwnd: isize, mode: Backdrop, tint_abgr: u32, dark: bool) {
    unsafe {
        let hwnd = hwnd as HWND;
        // Тёмный/светлый заголовок окна.
        let dark_val: BOOL = dark as BOOL;
        for attr in [20i32, 19i32] {
            if DwmSetWindowAttribute(
                hwnd,
                attr as _,
                &dark_val as *const BOOL as *const c_void,
                std::mem::size_of::<BOOL>() as u32,
            ) == 0
            {
                break;
            }
        }

        let user32 = GetModuleHandleW(wide("user32.dll").as_ptr());
        if (user32 as isize) == 0 {
            return;
        }
        let Some(f) = GetProcAddress(user32, b"SetWindowCompositionAttribute\0".as_ptr()) else {
            return;
        };
        let f: SwcaFn = std::mem::transmute::<unsafe extern "system" fn() -> isize, SwcaFn>(f);
        let (state, flags, color) = match mode {
            Backdrop::None => (0, 0, 0),
            Backdrop::Transparent => (2, 2, 0),
            Backdrop::Blur => (3, 2, 0),
            Backdrop::Acrylic => (4, 0, tint_abgr),
        };
        let mut policy = AccentPolicy { state, flags, color, anim: 0 };
        let mut data = CompAttrData {
            attrib: 19, // WCA_ACCENT_POLICY
            data: &mut policy as *mut AccentPolicy as *mut c_void,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        f(hwnd, &mut data);
    }
}

// ───────────────── Замена стандартного диспетчера (Ctrl+Shift+Esc) ─────────────────

const IFEO: &str =
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\taskmgr.exe";

/// Текущее значение Debugger для taskmgr.exe (если есть).
pub fn hook_value() -> Option<String> {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(IFEO)
        .ok()?
        .get_value::<String, _>("Debugger")
        .ok()
}

pub fn hook_is_ours() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let exe = exe.display().to_string().to_lowercase();
    hook_value()
        .map(|v| v.to_lowercase().replace('"', "").trim() == exe)
        .unwrap_or(false)
}

pub fn install_hook() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let (key, _) = RegKey::predef(HKEY_LOCAL_MACHINE).create_subkey(IFEO)?;
    key.set_value("Debugger", &format!("\"{}\"", exe.display()))
}

pub fn uninstall_hook() -> std::io::Result<()> {
    let key = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(IFEO, KEY_SET_VALUE)?;
    match key.delete_value("Debugger") {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

// ───────────────────────────── Удаление ─────────────────────────────

const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\SwTsM";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

/// Полное удаление SwTsM: Ctrl+Shift+Esc, ярлыки, запись в «Программах»,
/// настройки и сами файлы (папка удаляется целиком, только если её создал инсталлер).
pub fn uninstall_self() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no parent dir")?.to_path_buf();

    // другие запущенные копии
    let _ = std::process::Command::new("taskkill.exe")
        .args(["/F", "/FI", "IMAGENAME eq SwTsM.exe", "/FI", &format!("PID ne {}", std::process::id())])
        .creation_flags(CREATE_NO_WINDOW)
        .status();

    if hook_is_ours() {
        let _ = uninstall_hook();
    }
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let _ = RegKey::predef(root).delete_subkey_all(UNINSTALL_KEY);
    }
    let script = "foreach ($f in 'Desktop','CommonDesktopDirectory','Programs','CommonPrograms') { \
        $p = Join-Path ([Environment]::GetFolderPath($f)) 'SwTsM.lnk'; \
        if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Force } }";
    let _ = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    if let Some(a) = std::env::var_os("APPDATA") {
        let _ = std::fs::remove_dir_all(PathBuf::from(a).join("SwTsM"));
    }

    // Запущенный exe нельзя удалить — удаляем через пару секунд после выхода.
    let target = if dir.join("swtsm.installed").exists() {
        format!("rmdir /s /q \"{}\"", dir.display())
    } else {
        format!("del /f /q \"{}\"", exe.display())
    };
    std::process::Command::new("cmd.exe")
        .raw_arg(format!("/C ping 127.0.0.1 -n 3 >nul & {target}"))
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Перезапуск текущего exe с правами администратора и заданными аргументами.
pub fn run_self_elevated(args: &str) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    shell_execute("runas", &exe.display().to_string(), args, SW_HIDE).is_ok()
}

pub fn message_box(title: &str, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
    let (t, x) = (wide(title), wide(text));
    unsafe {
        MessageBoxW(0 as HWND, x.as_ptr(), t.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}

/// Устанавливает/снимает перехват; без прав администратора — через UAC.
pub fn request_hook(install: bool) -> Result<(), String> {
    if is_admin() {
        let r = if install { install_hook() } else { uninstall_hook() };
        return r.map_err(|e| e.to_string());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let flag = if install { "--install-hook" } else { "--uninstall-hook" };
    shell_execute("runas", &exe.display().to_string(), flag, SW_HIDE)
}

// ───────────────────────────── Службы ─────────────────────────────

#[derive(Clone)]
pub struct ServiceInfo {
    pub name: String,
    pub display: String,
    pub state: u32,
    pub pid: u32,
}

pub fn service_state_name(state: u32) -> &'static str {
    match state {
        1 => tr("Stopped", "Остановлена", "Зупинена"),
        2 => tr("Starting", "Запускается", "Запускається"),
        3 => tr("Stopping", "Останавливается", "Зупиняється"),
        4 => tr("Running", "Выполняется", "Виконується"),
        5 => tr("Resuming", "Возобновляется", "Відновлюється"),
        6 => tr("Pausing", "Приостанавливается", "Призупиняється"),
        7 => tr("Paused", "Приостановлена", "Призупинена"),
        _ => tr("Unknown", "Неизвестно", "Невідомо"),
    }
}

unsafe fn pwstr_to_string(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0;
    while *p.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

const SC_MANAGER_CONNECT: u32 = 0x0001;
const SC_MANAGER_ENUMERATE_SERVICE: u32 = 0x0004;
const SVC_TYPE_WIN32: u32 = 0x30;
const SVC_STATE_ALL: u32 = 0x3;
const ACC_QUERY_STATUS: u32 = 0x0004;
const ACC_START: u32 = 0x0010;
const ACC_STOP: u32 = 0x0020;
const CONTROL_STOP: u32 = 0x1;

pub fn list_services() -> Result<Vec<ServiceInfo>, String> {
    unsafe {
        let scm = OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE);
        if (scm as isize) == 0 {
            return Err(last_error());
        }
        let mut out = Vec::new();
        let mut buf: Vec<u64> = vec![0; 64 * 1024 / 8];
        let mut resume = 0u32;
        loop {
            let mut needed = 0u32;
            let mut count = 0u32;
            let ok = EnumServicesStatusExW(
                scm,
                SC_ENUM_PROCESS_INFO,
                SVC_TYPE_WIN32,
                SVC_STATE_ALL,
                buf.as_mut_ptr() as *mut u8,
                (buf.len() * 8) as u32,
                &mut needed,
                &mut count,
                &mut resume,
                null(),
            );
            let err = GetLastError();
            let items = std::slice::from_raw_parts(
                buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW,
                count as usize,
            );
            for it in items {
                out.push(ServiceInfo {
                    name: pwstr_to_string(it.lpServiceName),
                    display: pwstr_to_string(it.lpDisplayName),
                    state: it.ServiceStatusProcess.dwCurrentState,
                    pid: it.ServiceStatusProcess.dwProcessId,
                });
            }
            if ok != 0 {
                break;
            }
            if err == ERROR_MORE_DATA {
                let need = (needed as usize / 8 + 1).max(buf.len());
                buf.resize(need, 0);
                continue;
            }
            CloseServiceHandle(scm);
            return Err(std::io::Error::from_raw_os_error(err as i32).to_string());
        }
        CloseServiceHandle(scm);
        Ok(out)
    }
}

unsafe fn open_service(name: &str, access: u32) -> Result<(isize, isize), String> {
    let scm = OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT);
    if (scm as isize) == 0 {
        return Err(last_error());
    }
    let n = wide(name);
    let svc = OpenServiceW(scm, n.as_ptr(), access);
    if (svc as isize) == 0 {
        let e = last_error();
        CloseServiceHandle(scm);
        return Err(e);
    }
    Ok((scm as isize, svc as isize))
}

pub fn start_service(name: &str) -> Result<(), String> {
    unsafe {
        let (scm, svc) = open_service(name, ACC_START)?;
        let ok = StartServiceW(svc as _, 0, null());
        let e = last_error();
        CloseServiceHandle(svc as _);
        CloseServiceHandle(scm as _);
        if ok == 0 {
            Err(e)
        } else {
            Ok(())
        }
    }
}

pub fn stop_service(name: &str) -> Result<(), String> {
    unsafe {
        let (scm, svc) = open_service(name, ACC_STOP | ACC_QUERY_STATUS)?;
        let mut st: SERVICE_STATUS = std::mem::zeroed();
        let ok = ControlService(svc as _, CONTROL_STOP, &mut st);
        let e = last_error();
        let mut result = if ok == 0 { Err(e) } else { Ok(()) };
        if result.is_ok() {
            // ждём остановки до 15 секунд
            for _ in 0..60 {
                if QueryServiceStatus(svc as _, &mut st) == 0 || st.dwCurrentState == 1 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            if st.dwCurrentState != 1 {
                result = Err(tr(
                    "the service did not stop in time",
                    "служба не остановилась вовремя",
                    "служба не зупинилася вчасно",
                )
                .into());
            }
        }
        CloseServiceHandle(svc as _);
        CloseServiceHandle(scm as _);
        result
    }
}

// ─────────────────────────── Автозагрузка ───────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StartupSource {
    UserRun,
    MachineRun,
    MachineRun32,
    UserFolder,
    CommonFolder,
}

impl StartupSource {
    pub fn label(self) -> &'static str {
        match self {
            StartupSource::UserRun => tr("Registry (user)", "Реестр (пользователь)", "Реєстр (користувач)"),
            StartupSource::MachineRun => tr("Registry (machine)", "Реестр (компьютер)", "Реєстр (комп'ютер)"),
            StartupSource::MachineRun32 => {
                tr("Registry (machine, 32-bit)", "Реестр (компьютер, 32-бит)", "Реєстр (комп'ютер, 32-біт)")
            }
            StartupSource::UserFolder => tr("Startup folder", "Папка автозагрузки", "Папка автозавантаження"),
            StartupSource::CommonFolder => {
                tr("Common startup folder", "Общая папка автозагрузки", "Спільна папка автозавантаження")
            }
        }
    }

    fn approved(self) -> (RegKey, &'static str) {
        match self {
            StartupSource::UserRun => (
                RegKey::predef(HKEY_CURRENT_USER),
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
            ),
            StartupSource::MachineRun => (
                RegKey::predef(HKEY_LOCAL_MACHINE),
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
            ),
            StartupSource::MachineRun32 => (
                RegKey::predef(HKEY_LOCAL_MACHINE),
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run32",
            ),
            StartupSource::UserFolder => (
                RegKey::predef(HKEY_CURRENT_USER),
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder",
            ),
            StartupSource::CommonFolder => (
                RegKey::predef(HKEY_LOCAL_MACHINE),
                r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder",
            ),
        }
    }
}

#[derive(Clone)]
pub struct StartupItem {
    pub name: String,
    pub command: String,
    pub source: StartupSource,
    pub enabled: bool,
}

fn approved_enabled(src: StartupSource, name: &str) -> bool {
    let (root, path) = src.approved();
    root.open_subkey(path)
        .ok()
        .and_then(|k| k.get_raw_value(name).ok())
        .map(|v| v.bytes.first().map(|b| b % 2 == 0).unwrap_or(true))
        .unwrap_or(true)
}

pub fn list_startup() -> Vec<StartupItem> {
    let mut out = Vec::new();
    let run_keys = [
        (StartupSource::UserRun, HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Run"),
        (StartupSource::MachineRun, HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run"),
        (
            StartupSource::MachineRun32,
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run",
        ),
    ];
    for (src, root, path) in run_keys {
        if let Ok(key) = RegKey::predef(root).open_subkey(path) {
            for (name, _) in key.enum_values().flatten() {
                if name.is_empty() {
                    continue;
                }
                let command = key.get_value::<String, _>(&name).unwrap_or_default();
                out.push(StartupItem {
                    enabled: approved_enabled(src, &name),
                    name,
                    command,
                    source: src,
                });
            }
        }
    }
    let folders = [
        (
            StartupSource::UserFolder,
            std::env::var_os("APPDATA")
                .map(|p| PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs\Startup")),
        ),
        (
            StartupSource::CommonFolder,
            std::env::var_os("ProgramData")
                .map(|p| PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs\StartUp")),
        ),
    ];
    for (src, dir) in folders {
        let Some(dir) = dir else { continue };
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().to_string();
            if file.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            out.push(StartupItem {
                enabled: approved_enabled(src, &file),
                name: file,
                command: e.path().display().to_string(),
                source: src,
            });
        }
    }
    out.sort_by_key(|i| i.name.to_lowercase());
    out
}

pub fn set_startup_enabled(item: &StartupItem, enabled: bool) -> Result<(), String> {
    let (root, path) = item.source.approved();
    let (key, _) = root.create_subkey(path).map_err(|e| e.to_string())?;
    let mut bytes = vec![0u8; 12];
    bytes[0] = if enabled { 2 } else { 3 };
    key.set_raw_value(&item.name, &RegValue { bytes, vtype: REG_BINARY })
        .map_err(|e| e.to_string())
}
