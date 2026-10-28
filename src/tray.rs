//! System tray icon and its context menu.

use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::store::Entry;

pub const MENU_OPEN_POPUP: &str = "open-popup";
pub const MENU_OPEN_WINDOW: &str = "open-window";
pub const MENU_QUIT: &str = "quit";
pub const MENU_COPY_PREFIX: &str = "copy:";

pub fn create(entries: &[Entry]) -> Result<TrayIcon, String> {
    TrayIconBuilder::new()
        .with_icon(icon())
        .with_tooltip(tooltip(entries))
        .with_menu(Box::new(menu(entries)))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| e.to_string())
}

pub fn refresh(tray: &TrayIcon, entries: &[Entry]) {
    tray.set_menu(Some(Box::new(menu(entries))));
    let _ = tray.set_tooltip(Some(tooltip(entries)));
}

fn tooltip(entries: &[Entry]) -> String {
    match entries.len() {
        1 => "TOTP Tray - 1 entry".into(),
        n => format!("TOTP Tray - {n} entries"),
    }
}

fn menu(entries: &[Entry]) -> Menu {
    let menu = Menu::new();
    let _ = menu.append(&MenuItem::with_id(MENU_OPEN_POPUP, "Open popup", true, None));
    let _ = menu.append(&MenuItem::with_id(MENU_OPEN_WINDOW, "Open in window", true, None));
    let _ = menu.append(&PredefinedMenuItem::separator());
    if entries.is_empty() {
        let _ = menu.append(&MenuItem::new("No entries yet", false, None));
    }
    for e in entries {
        // '&' marks a mnemonic in Win32 menus.
        let text = format!("Copy code: {}", e.label()).replace('&', "&&");
        let _ = menu.append(&MenuItem::with_id(format!("{MENU_COPY_PREFIX}{}", e.id), text, true, None));
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id(MENU_QUIT, "Quit", true, None));
    menu
}

/// Draws the app icon (purple disc with a white clock) at 32x32, anti-aliased.
pub fn icon() -> Icon {
    const SIZE: u32 = 32;
    const SS: u32 = 4; // supersampling per axis
    let c = SIZE as f32 / 2.0;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (mut disc, mut white) = (0.0f32, 0.0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = x as f32 + (sx as f32 + 0.5) / SS as f32 - c;
                    let py = y as f32 + (sy as f32 + 0.5) / SS as f32 - c;
                    let r = (px * px + py * py).sqrt();
                    if r <= 15.5 {
                        disc += 1.0;
                        let ring = (r - 10.5).abs() <= 1.2;
                        let hand_up = seg_dist(px, py, 0.0, 0.0, 0.0, -6.5) <= 1.3;
                        let hand_right = seg_dist(px, py, 0.0, 0.0, 4.5, 3.0) <= 1.3;
                        if ring || hand_up || hand_right {
                            white += 1.0;
                        }
                    }
                }
            }
            let n = (SS * SS) as f32;
            let (a, w) = (disc / n, if disc > 0.0 { white / disc } else { 0.0 });
            let mix = |bg: f32| (bg * (1.0 - w) + 255.0 * w) as u8;
            rgba.extend_from_slice(&[mix(0x67 as f32), mix(0x50 as f32), mix(0xA4 as f32), (a * 255.0) as u8]);
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("valid icon buffer")
}

fn seg_dist(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    let (qx, qy) = (ax + t * dx - px, ay + t * dy - py);
    (qx * qx + qy * qy).sqrt()
}
