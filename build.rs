// В самодостаточном GNU-тулчейне Rust нет импорт-библиотеки libshlwapi.a,
// которая нужна зависимости `webbrowser`. GNU ld умеет линковаться прямо с DLL,
// поэтому кладём копию системной shlwapi.dll в OUT_DIR и добавляем путь поиска.
fn main() {
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env != "gnu" {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let src = std::path::Path::new(&windir).join("System32").join("shlwapi.dll");
    let dst = out.join("shlwapi.dll");
    if std::fs::copy(&src, &dst).is_ok() {
        println!("cargo:rustc-link-search=native={}", out.display());
    }
    println!("cargo:rerun-if-changed=build.rs");
}
