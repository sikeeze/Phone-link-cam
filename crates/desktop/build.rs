fn main() {
    // Bug tauri-build 2.7.4 + tauri 2.12: is_dev() membaca DEP_TAURI_DEV
    // (diset cargo dari build script crate `tauri`), namun build script
    // package ini kadang dijalankan terlebih dahulu. UI desktop selalu
    // produksi (disajikan server HTTPS axum, bukan dev server), jadi
    // dev=false akurat. Solusi resmi: pasangkan tauri-build ke versi
    // yang sama persis dengan tauri.
    std::env::set_var("DEP_TAURI_DEV", "false");
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new().window_icon_path("icons/icon.ico"),
    ))
    .expect("tauri-build gagal");
}
