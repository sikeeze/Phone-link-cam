//! System tray desktop (README C: minimize to tray, start with Windows).

#![allow(dead_code)] // kerangka: diisi oleh fase berikutnya

/// Ikon tray 1x1 PNG transparan (placeholder, diganti saat fase 1).
pub const TRAY_ICON_PNG: &[u8] = include_bytes!("../assets/tray.png");

/// Menu tray: Show, Start with Windows, Quit (README C2).
pub const MENU_SHOW: &str = "show";
pub const MENU_AUTOSTART: &str = "autostart";
pub const MENU_QUIT: &str = "quit";
