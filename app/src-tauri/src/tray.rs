//! Ícone e menu da bandeja: Abrir painel / Não perturbe / Mudo / Sair.

use crate::AppState;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub const TRAY_ID: &str = "nyx";
const ICON: &[u8] = include_bytes!("../icons/tray.png");
const ICON_BADGE: &[u8] = include_bytes!("../icons/tray-badge.png");
const TOOLTIP: &str = "Nyx";
const TOOLTIP_BADGE: &str = "Nyx: avisos segurados pelo não perturbe";

type Check = CheckMenuItem<tauri::Wry>;

/// Devolve os itens "Não perturbe" e "Mudo" (o check acompanha as settings).
pub fn build(app: &AppHandle, dnd: bool, muted: bool) -> tauri::Result<(Check, Check)> {
    let open = MenuItem::with_id(app, "open", "Abrir painel", true, None::<&str>)?;
    let dnd_item = CheckMenuItem::with_id(app, "dnd", "Não perturbe", true, dnd, None::<&str>)?;
    let mute_item = CheckMenuItem::with_id(app, "mute", "Mudo", true, muted, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &dnd_item, &mute_item, &sep, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(ICON)?)
        .tooltip(TOOLTIP)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::show_main(app),
            "dnd" => crate::toggle_dnd(app),
            "mute" => crate::voice_tauri::toggle_mute(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok((dnd_item, mute_item))
}

/// Liga/desliga o badge (avisos de prioridade média segurados pelo não perturbe).
pub fn set_badge(app: &AppHandle, on: bool) {
    let state = app.state::<AppState>();
    if state.badge.swap(on, std::sync::atomic::Ordering::SeqCst) == on {
        return;
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(img) = Image::from_bytes(if on { ICON_BADGE } else { ICON }) {
            let _ = tray.set_icon(Some(img));
        }
        let _ = tray.set_tooltip(Some(if on { TOOLTIP_BADGE } else { TOOLTIP }));
    }
}
