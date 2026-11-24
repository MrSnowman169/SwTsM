// 1) Инсталлер вшивает в себя готовый SwTsM.exe — он должен быть собран заранее
//    (`cargo build --release -p swtsm`, см. build.ps1).
// 2) Обход отсутствия libshlwapi.a в GNU-тулчейне (как и в основном приложении).
fn main() {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let app = manifest.join("..").join("target").join("release").join("SwTsM.exe");
    if !app.exists() {
        panic!(
            "SwTsM.exe not found at {}. Build the app first: cargo build --release -p swtsm",
            app.display()
        );
    }
    println!("cargo:rerun-if-changed={}", app.display());
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default() == "gnu" {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        let src = std::path::Path::new(&windir).join("System32").join("shlwapi.dll");
        if std::fs::copy(&src, out.join("shlwapi.dll")).is_ok() {
            println!("cargo:rustc-link-search=native={}", out.display());
        }
    }
}
