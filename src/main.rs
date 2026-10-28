// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod store;
mod totp;
mod tray;
mod winutil;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use slint::winit_030::winit::event::WindowEvent;
use slint::winit_030::winit::platform::windows::{CornerPreference, WindowExtWindows};
use slint::winit_030::{EventResult, WinitWindowAccessor};
use slint::{
    CloseRequestResponse, ComponentHandle, LogicalSize, Model, ModelRc, PhysicalPosition, PhysicalSize, SharedString,
    Timer, TimerMode, VecModel,
};
use tray_icon::menu::MenuEvent;
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};

use store::{Config, Entry, OpenMode};
use totp::{Algorithm, SecretEncoding};

slint::include_modules!();

#[derive(Clone, Copy)]
enum Clock {
    /// Real time plus an offset.
    Live { offset_ms: i64 },
    /// Time stands still at a fixed instant.
    Fixed { at_ms: i64 },
}

impl Clock {
    fn now_ms(self) -> i64 {
        match self {
            Clock::Live { offset_ms } => system_ms() + offset_ms,
            Clock::Fixed { at_ms } => at_ms,
        }
    }

    fn set_ms(&mut self, target: i64) {
        match self {
            Clock::Live { offset_ms } => *offset_ms = target - system_ms(),
            Clock::Fixed { at_ms } => *at_ms = target,
        }
    }
}

fn system_ms() -> i64 {
    Utc::now().timestamp_millis()
}

enum Confirm {
    ClearAll,
    Delete(String),
}

struct State {
    cfg: Config,
    clock: Clock,
    revealed: HashSet<String>,
    filter: String,
    /// Ids of the rows currently in the list model, in order.
    shown: Vec<String>,
    /// `None` while adding a new entry.
    editing: Option<String>,
    confirm: Option<Confirm>,
}

struct App {
    ui: AppWindow,
    state: RefCell<State>,
    model: Rc<VecModel<EntryView>>,
    tray: RefCell<Option<tray_icon::TrayIcon>>,
    snackbar_timer: Timer,
    /// Whether the window is currently presented as the tray popup.
    popup: Cell<bool>,
    /// When the popup last hid itself after losing focus.
    auto_hidden_at: Cell<Option<Instant>>,
    /// A native file dialog is open; the popup must not auto-hide.
    modal: Cell<bool>,
    /// Last position and size of the separate window.
    window_geometry: Cell<Option<(PhysicalPosition, PhysicalSize)>>,
}

/// Popup size in logical pixels.
const POPUP_SIZE: (f32, f32) = (480.0, 720.0);
/// Default size of the separate window in logical pixels.
const WINDOW_SIZE: (f32, f32) = (500.0, 820.0);

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

fn with_app(f: impl FnOnce(&Rc<App>)) {
    if let Some(app) = APP.with(|a| a.borrow().clone()) {
        f(&app);
    }
}

fn main() {
    let Some(_guard) = winutil::single_instance() else {
        winutil::info_box("TOTP Tray", "TOTP Tray is already running.\nLook for its icon in the system tray.");
        return;
    };
    // The app starts in the tray. `--show` opens the separate window right away.
    let start_shown = std::env::args().any(|a| a == "--show");
    let first_run = !Config::path().exists();

    let (cfg, load_error) = match Config::load() {
        Ok(cfg) => (cfg, None),
        Err(e) => (Config::default(), Some(e)),
    };

    let ui = AppWindow::new().expect("failed to create window");
    let model = Rc::new(VecModel::<EntryView>::default());
    ui.set_entries(ModelRc::from(model.clone()));

    let app = Rc::new(App {
        ui,
        state: RefCell::new(State {
            cfg,
            clock: Clock::Live { offset_ms: 0 },
            revealed: HashSet::new(),
            filter: String::new(),
            shown: Vec::new(),
            editing: None,
            confirm: None,
        }),
        model,
        tray: RefCell::new(None),
        snackbar_timer: Timer::default(),
        popup: Cell::new(false),
        auto_hidden_at: Cell::new(None),
        modal: Cell::new(false),
        window_geometry: Cell::new(None),
    });
    APP.with(|a| *a.borrow_mut() = Some(app.clone()));

    match tray::create(&app.state.borrow().cfg.entries) {
        Ok(t) => *app.tray.borrow_mut() = Some(t),
        Err(e) => winutil::info_box("TOTP Tray", &format!("Could not create the tray icon: {e}")),
    }
    install_tray_handlers();
    app.bind_callbacks();
    app.rebuild();
    app.tick();

    let ticker = Timer::default();
    ticker.start(TimerMode::Repeated, Duration::from_millis(100), || with_app(|app| app.tick()));

    // Window handles exist once the event loop runs, so present from inside it.
    let _ = slint::invoke_from_event_loop(move || {
        with_app(|app| {
            app.install_window_hooks();
            if let Some(e) = load_error {
                // Do not overwrite a config we could not read.
                app.state.borrow_mut().cfg.version = 0;
                app.open(OpenMode::Window, None);
                app.toast(&format!("Config not loaded, saving disabled until fixed: {e}"));
            } else if start_shown {
                app.open(OpenMode::Window, None);
            } else if first_run {
                // Show once where the app lives; afterwards it starts silently in the tray.
                app.persist();
                app.open(OpenMode::Popup, None);
                app.toast("TOTP Tray lives in the tray. Click its icon to open it again.");
            }
        })
    });

    slint::run_event_loop_until_quit().expect("event loop failed");
    // Drop the tray icon before exit so it disappears immediately.
    with_app(|app| drop(app.tray.borrow_mut().take()));
}

fn install_tray_handlers() {
    TrayIconEvent::set_event_handler(Some(|event: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = event {
            let _ = slint::invoke_from_event_loop(move || with_app(|app| app.on_tray_click(rect)));
        }
    }));
    MenuEvent::set_event_handler(Some(|event: MenuEvent| {
        let id = event.id().as_ref().to_string();
        let _ = slint::invoke_from_event_loop(move || with_app(|app| app.on_menu(&id)));
    }));
}

/// Borrows the app inside a UI callback without creating an Rc cycle.
macro_rules! cb {
    ($ui:expr, $on:ident, |$app:ident $(, $arg:ident)*| $body:expr) => {
        $ui.$on(move |$($arg),*| with_app(|$app| $body))
    };
}

impl App {
    fn bind_callbacks(&self) {
        let ui = &self.ui;
        cb!(ui, on_shift_time, |app, secs| app.shift_time(i64::from(secs)));
        cb!(ui, on_freeze_toggle, |app| app.freeze_toggle());
        cb!(ui, on_set_time, |app, text| app.set_time(&text));
        cb!(ui, on_live_time, |app| app.set_clock(Clock::Live { offset_ms: 0 }));
        cb!(ui, on_copy_timestamp, |app| {
            let now = app.state.borrow().clock.now_ms();
            app.copy(&now.div_euclid(1000).to_string(), "Timestamp copied");
        });

        cb!(ui, on_copy_code, |app, id| app.copy_code(&id));
        cb!(ui, on_copy_secret, |app, id| {
            if let Some(e) = app.entry(&id) {
                app.copy(&e.secret, "Secret copied");
            }
        });
        cb!(ui, on_copy_uri, |app, id| {
            if let Some(e) = app.entry(&id) {
                app.copy(&totp::build_uri(&e.params()), "otpauth URI copied");
            }
        });
        cb!(ui, on_toggle_secret, |app, id| {
            {
                let mut st = app.state.borrow_mut();
                if !st.revealed.remove(id.as_str()) {
                    st.revealed.insert(id.to_string());
                }
            }
            app.tick();
        });
        cb!(ui, on_edit_entry, |app, id| app.open_editor(Some(id.to_string())));
        cb!(ui, on_delete_entry, |app, id| {
            if let Some(e) = app.entry(&id) {
                app.ask(Confirm::Delete(e.id.clone()), "Delete entry?", &format!("\"{}\" will be removed.", e.label()), "Delete");
            }
        });

        cb!(ui, on_add_entry, |app| app.open_editor(None));
        cb!(ui, on_parse_uri, |app| app.parse_editor_uri());
        cb!(ui, on_save_entry, |app| app.save_editor());
        cb!(ui, on_confirm_accept, |app| app.confirm_accept());

        cb!(ui, on_open_import, |app| {
            app.ui.set_import_input(SharedString::new());
            app.ui.set_import_error(SharedString::new());
            app.ui.set_dialog("import".into());
        });
        cb!(ui, on_import_submit, |app| app.import_text());
        cb!(ui, on_import_clipboard, |app| app.import_clipboard());
        cb!(ui, on_import_qr_file, |app| app.import_qr_file());
        cb!(ui, on_import_config_file, |app| app.import_config_file());
        cb!(ui, on_export_config, |app| app.export(false));
        cb!(ui, on_export_uris, |app| app.export(true));
        cb!(ui, on_clear_all, |app| {
            let n = app.state.borrow().cfg.entries.len();
            app.ask(
                Confirm::ClearAll,
                "Clear all entries?",
                &format!("All {n} entries will be deleted. Export the config first if you need them later."),
                "Clear all",
            );
        });
        cb!(ui, on_filter_changed, |app, text| {
            app.state.borrow_mut().filter = text.to_string();
            app.rebuild();
        });
        cb!(ui, on_hide_to_tray, |app| app.hide_window());
        cb!(ui, on_toggle_mode, |app| {
            let mode = if app.popup.get() { OpenMode::Window } else { OpenMode::Popup };
            app.state.borrow_mut().cfg.settings.open_mode = mode;
            app.persist();
            app.open(mode, None);
        });
        cb!(ui, on_escape, |app| {
            if app.popup.get() {
                app.hide_window();
            }
        });
        cb!(ui, on_quit, |_app| {
            let _ = slint::quit_event_loop();
        });
    }

    // --- window & tray ------------------------------------------------------

    fn install_window_hooks(&self) {
        let window = self.ui.window();
        window.on_close_requested(|| {
            with_app(|app| app.remember_geometry());
            CloseRequestResponse::HideWindow
        });
        window.on_winit_window_event(|_, event| {
            if let WindowEvent::Focused(false) = event {
                with_app(|app| app.on_focus_lost());
            }
            EventResult::Propagate
        });
    }

    fn open(&self, mode: OpenMode, anchor: Option<tray_icon::Rect>) {
        match mode {
            OpenMode::Popup => self.show_popup(anchor),
            OpenMode::Window => self.show_separate_window(),
        }
    }

    /// Shows the borderless popup next to the tray icon.
    fn show_popup(&self, anchor: Option<tray_icon::Rect>) {
        self.remember_geometry();
        self.popup.set(true);
        self.ui.set_popup_mode(true);

        let anchor = anchor.or_else(|| self.tray.borrow().as_ref().and_then(|t| t.rect())).filter(|r| r.size.width > 0);
        let (ax, ay) = match anchor {
            Some(r) => (r.position.x as i32 + r.size.width as i32 / 2, r.position.y as i32 + r.size.height as i32 / 2),
            // Icon hidden in the overflow area: use the corner of the screen with the cursor.
            None => {
                let (cx, cy) = winutil::cursor_pos();
                let wa = winutil::work_area_at(cx, cy);
                (wa.right, wa.bottom)
            }
        };
        let wa = winutil::work_area_at(ax, ay);
        let margin = (12.0 * wa.scale) as i32;
        let w = (POPUP_SIZE.0 * wa.scale) as i32;
        let h = ((POPUP_SIZE.1 * wa.scale) as i32).min(wa.bottom - wa.top - 2 * margin);
        self.ui.window().set_size(PhysicalSize::new(w as u32, h as u32));
        self.place_popup(ax, ay);
        self.present();
        // Slint may grow the window to the layout's minimum size when it is shown, and
        // removing the title bar of a visible window shifts it, so place it again.
        self.place_popup(ax, ay);
        Timer::single_shot(Duration::from_millis(30), move || {
            with_app(|app| {
                if app.popup.get() {
                    app.place_popup(ax, ay);
                }
            })
        });
    }

    /// Positions the popup next to the anchor point, inside the work area.
    fn place_popup(&self, ax: i32, ay: i32) {
        let wa = winutil::work_area_at(ax, ay);
        let margin = (12.0 * wa.scale) as i32;
        let size = self.ui.window().size();
        let (w, h) = (size.width as i32, size.height as i32);
        let x = (ax - w / 2).clamp(wa.left + margin, (wa.right - w - margin).max(wa.left + margin));
        let y = if ay > (wa.top + wa.bottom) / 2 {
            (wa.bottom - h - margin).max(wa.top)
        } else {
            wa.top + margin
        };
        self.ui.window().set_position(PhysicalPosition::new(x, y));
    }

    /// Shows the regular window, restoring its last position and size.
    fn show_separate_window(&self) {
        let was_popup = self.popup.get() || !self.ui.window().is_visible();
        self.popup.set(false);
        self.ui.set_popup_mode(false);
        if was_popup {
            let window = self.ui.window();
            match self.window_geometry.get() {
                Some((pos, size)) => {
                    window.set_size(size);
                    window.set_position(pos);
                }
                None => {
                    let (cx, cy) = winutil::cursor_pos();
                    let wa = winutil::work_area_at(cx, cy);
                    let h = (WINDOW_SIZE.1 * wa.scale).min((wa.bottom - wa.top) as f32);
                    let w = WINDOW_SIZE.0 * wa.scale;
                    window.set_size(LogicalSize::new(WINDOW_SIZE.0, h / wa.scale));
                    window.set_position(PhysicalPosition::new(
                        wa.left + (wa.right - wa.left - w as i32) / 2,
                        wa.top + (wa.bottom - wa.top - h as i32) / 2,
                    ));
                }
            }
        }
        self.present();
    }

    fn present(&self) {
        let _ = self.ui.show();
        let popup = self.popup.get();
        self.ui.window().with_winit_window(|w| {
            w.set_skip_taskbar(popup);
            w.set_corner_preference(CornerPreference::Round);
        });
        winutil::bring_to_front(self.ui.window());
        self.tick();
    }

    fn remember_geometry(&self) {
        let window = self.ui.window();
        if !self.popup.get() && window.is_visible() && !window.is_minimized() {
            self.window_geometry.set(Some((window.position(), window.size())));
        }
    }

    fn hide_window(&self) {
        self.remember_geometry();
        let _ = self.ui.hide();
    }

    fn on_focus_lost(&self) {
        if !self.popup.get() || self.modal.get() {
            return;
        }
        // Give owned windows (file dialogs, dropdowns) a moment to take focus first.
        Timer::single_shot(Duration::from_millis(150), || {
            with_app(|app| {
                let window = app.ui.window();
                if app.popup.get() && !app.modal.get() && window.is_visible() && !winutil::is_foreground(window) {
                    app.hide_window();
                    app.auto_hidden_at.set(Some(Instant::now()));
                }
            })
        });
    }

    fn on_tray_click(&self, rect: tray_icon::Rect) {
        // Clicking the icon steals focus from the popup first; that click should only close it.
        if self.auto_hidden_at.get().is_some_and(|t| t.elapsed() < Duration::from_millis(600)) {
            return;
        }
        let window = self.ui.window();
        if window.is_visible() && !window.is_minimized() {
            self.hide_window();
            return;
        }
        let mode = self.state.borrow().cfg.settings.open_mode;
        self.open(mode, Some(rect));
    }

    fn on_menu(&self, id: &str) {
        match id {
            tray::MENU_OPEN_POPUP => self.open(OpenMode::Popup, None),
            tray::MENU_OPEN_WINDOW => self.open(OpenMode::Window, None),
            tray::MENU_QUIT => {
                let _ = slint::quit_event_loop();
            }
            _ => {
                if let Some(entry_id) = id.strip_prefix(tray::MENU_COPY_PREFIX) {
                    self.copy_code(entry_id);
                }
            }
        }
    }

    // --- model --------------------------------------------------------------

    fn entry(&self, id: &str) -> Option<Entry> {
        self.state.borrow().cfg.entries.iter().find(|e| e.id == id).cloned()
    }

    /// Rebuilds the list after entries or the filter changed.
    fn rebuild(&self) {
        {
            let mut st = self.state.borrow_mut();
            let needle = st.filter.trim().to_lowercase();
            st.shown = st
                .cfg
                .entries
                .iter()
                .filter(|e| {
                    needle.is_empty()
                        || e.name.to_lowercase().contains(&needle)
                        || e.issuer.to_lowercase().contains(&needle)
                })
                .map(|e| e.id.clone())
                .collect();
            let ids: HashSet<&String> = st.cfg.entries.iter().map(|e| &e.id).collect();
            let revealed: HashSet<String> = st.revealed.iter().filter(|id| ids.contains(id)).cloned().collect();
            st.revealed = revealed;
            self.ui.set_total_count(st.cfg.entries.len() as i32);
        }
        let now = self.state.borrow().clock.now_ms();
        let rows = self.rows(now);
        self.model.set_vec(rows);
        if let Some(tray) = self.tray.borrow().as_ref() {
            tray::refresh(tray, &self.state.borrow().cfg.entries);
        }
    }

    fn rows(&self, now_ms: i64) -> Vec<EntryView> {
        let st = self.state.borrow();
        st.shown
            .iter()
            .filter_map(|id| st.cfg.entries.iter().find(|e| &e.id == id))
            .map(|e| entry_view(e, now_ms, st.revealed.contains(&e.id)))
            .collect()
    }

    /// Refreshes the clock display, codes and countdowns.
    fn tick(&self) {
        if !self.ui.window().is_visible() {
            return;
        }
        let clock = self.state.borrow().clock;
        let now = clock.now_ms();
        let (unix, utc, local) = format_time(now);
        self.ui.set_time_unix(unix.into());
        self.ui.set_time_utc(utc.into());
        self.ui.set_time_local(local.into());
        let (mode, text) = match clock {
            Clock::Live { offset_ms: 0 } => ("live", "LIVE".to_string()),
            Clock::Live { offset_ms } => ("offset", format!("OFFSET {}", format_offset(offset_ms))),
            Clock::Fixed { .. } => ("fixed", "FROZEN".to_string()),
        };
        self.ui.set_time_mode(mode.into());
        self.ui.set_time_mode_text(text.into());

        for (i, row) in self.rows(now).into_iter().enumerate() {
            if i < self.model.row_count() {
                self.model.set_row_data(i, row);
            }
        }

        if self.ui.get_dialog() == "edit" {
            let preview = match self.editor_params() {
                Ok(p) => totp::totp(&p, totp::counter_at(now.div_euclid(1000), p.period))
                    .map(|c| totp::format_code(&c))
                    .unwrap_or_else(|_| "—".into()),
                Err(_) => "—".into(),
            };
            self.ui.set_edit_preview(preview.into());
        }
    }

    fn persist(&self) {
        let st = self.state.borrow();
        if st.cfg.version == 0 {
            return; // load failed earlier; never clobber the user's file
        }
        if let Err(e) = st.cfg.save() {
            drop(st);
            self.toast(&format!("Saving failed: {e}"));
        }
    }

    fn toast(&self, msg: &str) {
        self.ui.set_snackbar(msg.into());
        let weak = self.ui.as_weak();
        self.snackbar_timer.start(TimerMode::SingleShot, Duration::from_millis(3200), move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_snackbar(SharedString::new());
            }
        });
    }

    fn copy(&self, text: &str, msg: &str) {
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
            Ok(()) => self.toast(msg),
            Err(e) => self.toast(&format!("Clipboard error: {e}")),
        }
    }

    fn copy_code(&self, id: &str) {
        let Some(e) = self.entry(id) else { return };
        let now = self.state.borrow().clock.now_ms();
        match totp::totp(&e.params(), totp::counter_at(now.div_euclid(1000), e.period)) {
            Ok(code) => self.copy(&code, &format!("Copied {} for {}", totp::format_code(&code), e.label())),
            Err(err) => self.toast(&err),
        }
    }

    // --- time ---------------------------------------------------------------

    fn set_clock(&self, clock: Clock) {
        self.state.borrow_mut().clock = clock;
        self.tick();
    }

    fn shift_time(&self, secs: i64) {
        let mut clock = self.state.borrow().clock;
        let target = clock.now_ms() + secs * 1000;
        clock.set_ms(target);
        self.set_clock(clock);
    }

    fn freeze_toggle(&self) {
        let clock = self.state.borrow().clock;
        self.set_clock(match clock {
            // Freeze on a whole second so the displayed value is exact.
            Clock::Live { .. } => Clock::Fixed { at_ms: clock.now_ms().div_euclid(1000) * 1000 },
            Clock::Fixed { at_ms } => Clock::Live { offset_ms: at_ms - system_ms() },
        });
    }

    fn set_time(&self, text: &str) {
        let mut clock = self.state.borrow().clock;
        match parse_time_input(text, clock.now_ms()) {
            Ok(target) => {
                clock.set_ms(target);
                self.set_clock(clock);
                self.ui.set_time_input(SharedString::new());
            }
            Err(e) => self.toast(&e),
        }
    }

    // --- editor -------------------------------------------------------------

    fn open_editor(&self, id: Option<String>) {
        let ui = &self.ui;
        let entry = id.as_deref().and_then(|id| self.entry(id));
        let e = entry.clone().unwrap_or_else(|| Entry {
            id: String::new(),
            name: String::new(),
            issuer: String::new(),
            secret: String::new(),
            algorithm: Algorithm::Sha1,
            digits: 6,
            period: 30,
        });
        ui.set_edit_title(if entry.is_some() { "Edit entry" } else { "Add entry" }.into());
        ui.set_edit_uri(SharedString::new());
        self.fill_editor(&e.params());
        ui.set_edit_error(SharedString::new());
        self.state.borrow_mut().editing = entry.map(|e| e.id);
        ui.set_dialog("edit".into());
        self.tick();
    }

    fn fill_editor(&self, p: &totp::OtpParams) {
        let ui = &self.ui;
        ui.set_edit_name(p.name.as_str().into());
        ui.set_edit_issuer(p.issuer.as_str().into());
        ui.set_edit_secret(p.secret.as_str().into());
        ui.set_edit_encoding(0);
        ui.set_edit_algorithm(p.algorithm.index());
        ui.set_edit_digits(p.digits as i32);
        ui.set_edit_period(p.period as i32);
    }

    fn editor_params(&self) -> Result<totp::OtpParams, String> {
        let ui = &self.ui;
        let encoding = SecretEncoding::from_index(ui.get_edit_encoding());
        let secret = totp::normalize_secret(&ui.get_edit_secret(), encoding)?;
        let digits = ui.get_edit_digits().max(0) as u32;
        let period = ui.get_edit_period().max(0) as u64;
        totp::validate_digits_period(digits, period)?;
        Ok(totp::OtpParams {
            name: ui.get_edit_name().trim().to_string(),
            issuer: ui.get_edit_issuer().trim().to_string(),
            secret,
            algorithm: Algorithm::from_index(ui.get_edit_algorithm()),
            digits,
            period,
        })
    }

    fn parse_editor_uri(&self) {
        match totp::parse_uri(&self.ui.get_edit_uri()) {
            Ok(list) => {
                self.fill_editor(&list[0]);
                let msg = if list.len() > 1 {
                    format!("URI contains {} entries; only the first was loaded. Use Import for all.", list.len())
                } else {
                    String::new()
                };
                self.ui.set_edit_error(msg.into());
            }
            Err(e) => self.ui.set_edit_error(e.into()),
        }
    }

    fn save_editor(&self) {
        let params = match self.editor_params() {
            Ok(p) => p,
            Err(e) => return self.ui.set_edit_error(e.into()),
        };
        let label;
        {
            let mut st = self.state.borrow_mut();
            let editing = st.editing.clone();
            let mut entry = Entry::from_params(params);
            label = entry.label();
            match editing.and_then(|id| st.cfg.entries.iter().position(|e| e.id == id)) {
                Some(i) => {
                    entry.id = st.cfg.entries[i].id.clone();
                    st.cfg.entries[i] = entry;
                }
                None => st.cfg.entries.push(entry),
            }
        }
        self.ui.set_dialog(SharedString::new());
        self.persist();
        self.rebuild();
        self.toast(&format!("Saved {label}"));
    }

    // --- confirmations ------------------------------------------------------

    fn ask(&self, what: Confirm, title: &str, message: &str, action: &str) {
        self.state.borrow_mut().confirm = Some(what);
        self.ui.set_confirm_title(title.into());
        self.ui.set_confirm_message(message.into());
        self.ui.set_confirm_action(action.into());
        self.ui.set_dialog("confirm".into());
    }

    fn confirm_accept(&self) {
        let what = self.state.borrow_mut().confirm.take();
        self.ui.set_dialog(SharedString::new());
        let msg = {
            let mut st = self.state.borrow_mut();
            match what {
                Some(Confirm::ClearAll) => {
                    let n = st.cfg.entries.len();
                    st.cfg.entries.clear();
                    st.revealed.clear();
                    format!("Cleared {n} entries")
                }
                Some(Confirm::Delete(id)) => {
                    st.cfg.entries.retain(|e| e.id != id);
                    "Entry deleted".to_string()
                }
                None => return,
            }
        };
        self.persist();
        self.rebuild();
        self.toast(&msg);
    }

    // --- import / export ----------------------------------------------------

    fn add_imported(&self, entries: Vec<Entry>, replace: bool) -> String {
        let msg = {
            let mut st = self.state.borrow_mut();
            if replace {
                let n = entries.len();
                st.cfg.entries = entries;
                format!("Replaced all entries with {n} imported")
            } else {
                let (added, skipped) = st.cfg.merge(entries);
                match skipped {
                    0 => format!("Imported {added} entries"),
                    s => format!("Imported {added} entries, skipped {s} duplicates"),
                }
            }
        };
        self.persist();
        self.rebuild();
        msg
    }

    fn import_text(&self) {
        let (params, errors) = store::parse_uri_lines(&self.ui.get_import_input());
        if params.is_empty() {
            let err = errors.into_iter().next().unwrap_or_else(|| "Nothing to import".into());
            return self.ui.set_import_error(err.into());
        }
        let mut msg = self.add_imported(params.into_iter().map(Entry::from_params).collect(), false);
        if !errors.is_empty() {
            msg = format!("{msg}; {} lines failed ({})", errors.len(), errors[0]);
        }
        self.ui.set_dialog(SharedString::new());
        self.toast(&msg);
    }

    fn append_import_input(&self, lines: &[String]) {
        let mut text = self.ui.get_import_input().to_string();
        for l in lines {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(l);
        }
        self.ui.set_import_input(text.into());
        self.ui.set_import_error(SharedString::new());
    }

    fn import_clipboard(&self) {
        let mut clipboard = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(e) => return self.ui.set_import_error(format!("Clipboard error: {e}").into()),
        };
        if let Ok(text) = clipboard.get_text() {
            let lines: Vec<String> =
                text.lines().map(str::trim).filter(|l| l.starts_with("otpauth")).map(String::from).collect();
            if !lines.is_empty() {
                return self.append_import_input(&lines);
            }
        }
        if let Ok(img) = clipboard.get_image() {
            let grey: Vec<u8> = img
                .bytes
                .chunks_exact(4)
                .map(|p| ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000) as u8)
                .collect();
            return self.handle_qr_result(decode_qr(img.width, img.height, &grey));
        }
        self.ui.set_import_error("The clipboard holds no otpauth URI or image".into());
    }

    /// A file dialog owned by the app window, so it stays on top of the popup.
    fn file_dialog(&self, title: &str) -> rfd::FileDialog {
        rfd::FileDialog::new().set_title(title).set_parent(&self.ui.window().window_handle())
    }

    fn run_modal<T>(&self, f: impl FnOnce() -> T) -> T {
        self.modal.set(true);
        let result = f();
        self.modal.set(false);
        result
    }

    fn import_qr_file(&self) {
        let dialog = self
            .file_dialog("Select QR code image")
            .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "webp"]);
        let Some(path) = self.run_modal(|| dialog.pick_file()) else {
            return;
        };
        let result = image::open(&path).map_err(|e| format!("Cannot open image: {e}")).map(|img| {
            let grey = img.to_luma8();
            decode_qr(grey.width() as usize, grey.height() as usize, grey.as_raw())
        });
        match result {
            Ok(r) => self.handle_qr_result(r),
            Err(e) => self.ui.set_import_error(e.into()),
        }
    }

    fn handle_qr_result(&self, found: Vec<String>) {
        let uris: Vec<String> = found.into_iter().filter(|s| s.starts_with("otpauth")).collect();
        if uris.is_empty() {
            self.ui.set_import_error("No otpauth QR code found in the image".into());
        } else {
            self.append_import_input(&uris);
            self.toast(&format!("Decoded {} QR code(s), review and press Import", uris.len()));
        }
    }

    fn import_config_file(&self) {
        let dialog = self
            .file_dialog("Import config")
            .add_filter("Config or URI list", &["json", "txt"])
            .add_filter("All files", &["*"]);
        let Some(path) = self.run_modal(|| dialog.pick_file()) else {
            return;
        };
        match Config::read_file(&path) {
            Ok(cfg) => {
                let msg = self.add_imported(cfg.entries, self.ui.get_import_replace());
                self.ui.set_dialog(SharedString::new());
                self.toast(&msg);
            }
            Err(e) => self.ui.set_import_error(e.into()),
        }
    }

    fn export(&self, as_uris: bool) {
        let (name, filter, ext) = if as_uris {
            ("totp-tray-uris.txt", "otpauth URI list", "txt")
        } else {
            ("totp-tray-config.json", "TOTP Tray config", "json")
        };
        let dialog = self.file_dialog("Export").set_file_name(name).add_filter(filter, &[ext]);
        let Some(path): Option<PathBuf> = self.run_modal(|| dialog.save_file()) else {
            return;
        };
        let cfg = self.state.borrow().cfg.clone();
        let result = if as_uris {
            let text: String = cfg.entries.iter().map(|e| totp::build_uri(&e.params()) + "\n").collect();
            std::fs::write(&path, text).map_err(|e| e.to_string())
        } else {
            Config { version: 1, ..cfg.clone() }.write_file(&path)
        };
        match result {
            Ok(()) => self.toast(&format!("Exported {} entries to {}", cfg.entries.len(), path.display())),
            Err(e) => self.toast(&format!("Export failed: {e}")),
        }
    }
}

fn decode_qr(width: usize, height: usize, grey: &[u8]) -> Vec<String> {
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(width, height, |x, y| grey[y * width + x]);
    img.detect_grids().into_iter().filter_map(|g| g.decode().ok().map(|(_, content)| content)).collect()
}

fn entry_view(e: &Entry, now_ms: i64, revealed: bool) -> EntryView {
    let params = e.params();
    let period_ms = (e.period.max(1) * 1000) as i64;
    let now = now_ms.max(0);
    let counter = (now / period_ms) as u64;
    let remaining_ms = period_ms - now % period_ms;

    let (title, subtitle) = match (e.name.is_empty(), e.issuer.is_empty()) {
        (false, false) => (e.name.clone(), e.issuer.clone()),
        (false, true) => (e.name.clone(), String::new()),
        (true, false) => (e.issuer.clone(), String::new()),
        (true, true) => ("Unnamed".to_string(), String::new()),
    };
    let initial = e.issuer.chars().chain(e.name.chars()).next().unwrap_or('?').to_uppercase().to_string();
    let code_at = |c: u64| totp::totp(&params, c).map(|code| totp::format_code(&code));
    let (code, prev, next, error) = match (code_at(counter), code_at(counter.saturating_sub(1)), code_at(counter + 1)) {
        (Ok(c), Ok(p), Ok(n)) => (c, p, n, String::new()),
        (Err(err), ..) | (_, Err(err), _) | (.., Err(err)) => (String::new(), String::new(), String::new(), err),
    };

    EntryView {
        id: e.id.as_str().into(),
        title: title.into(),
        subtitle: subtitle.into(),
        initial: initial.into(),
        code: code.into(),
        prev_code: prev.into(),
        next_code: next.into(),
        remaining: ((remaining_ms + 999) / 1000) as i32,
        progress: remaining_ms as f32 / period_ms as f32,
        meta: format!("T = {counter}  ·  {}  ·  {} digits  ·  {}s period", e.algorithm.name(), e.digits, e.period)
            .into(),
        revealed,
        secret: if revealed { e.secret.as_str().into() } else { SharedString::new() },
        uri: if revealed { totp::build_uri(&params).into() } else { SharedString::new() },
        error: error.into(),
    }
}

fn format_time(now_ms: i64) -> (String, String, String) {
    let secs = now_ms.div_euclid(1000);
    let Some(dt) = DateTime::<Utc>::from_timestamp(secs, 0) else {
        return (secs.to_string(), "out of range".into(), String::new());
    };
    (
        secs.to_string(),
        dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S local (UTC%:z)").to_string(),
    )
}

/// `+1h 2m 3s`, rounded to whole seconds.
fn format_offset(ms: i64) -> String {
    let secs = (ms as f64 / 1000.0).round() as i64;
    let sign = if secs < 0 { '−' } else { '+' };
    let mut rest = secs.unsigned_abs();
    let mut parts = Vec::new();
    for (unit, size) in [("d", 86_400), ("h", 3_600), ("m", 60)] {
        if rest >= size {
            parts.push(format!("{}{unit}", rest / size));
            rest %= size;
        }
    }
    if rest > 0 || parts.is_empty() {
        parts.push(format!("{rest}s"));
    }
    format!("{sign}{}", parts.join(" "))
}

/// Accepts unix seconds/milliseconds, RFC 3339, local date-times, or a
/// relative shift like `+90`, `-5m`, `+1h`.
fn parse_time_input(text: &str, now_ms: i64) -> Result<i64, String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("Enter a timestamp first".into());
    }
    if let Some(rel) = t.strip_prefix('+').map(|r| (1, r)).or_else(|| t.strip_prefix('-').map(|r| (-1, r))) {
        let (sign, body) = rel;
        let (num, unit) = body.split_at(body.find(|c: char| !c.is_ascii_digit()).unwrap_or(body.len()));
        if let Ok(n) = num.parse::<i64>() {
            let mult = match unit.trim() {
                "" | "s" => 1,
                "m" => 60,
                "h" => 3_600,
                "d" => 86_400,
                _ => return Err(format!("Unknown unit '{unit}' (use s, m, h, d)")),
            };
            return Ok(now_ms + sign * n * mult * 1000);
        }
    }
    if let Ok(n) = t.parse::<i64>() {
        // 13+ digits are treated as milliseconds.
        return Ok(if n.abs() >= 100_000_000_000 { n } else { n * 1000 });
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Ok(dt.timestamp_millis());
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(t, fmt) {
            return local_ms(naive);
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        return local_ms(date.and_hms_opt(0, 0, 0).expect("midnight is valid"));
    }
    Err(format!("Cannot parse '{t}' as a time"))
}

fn local_ms(naive: NaiveDateTime) -> Result<i64, String> {
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|dt| dt.timestamp_millis())
        .ok_or_else(|| "That local time does not exist (DST gap)".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_input() {
        assert_eq!(parse_time_input("1234567890", 0).unwrap(), 1_234_567_890_000);
        assert_eq!(parse_time_input("1234567890123", 0).unwrap(), 1_234_567_890_123);
        assert_eq!(parse_time_input("2009-02-13T23:31:30Z", 0).unwrap(), 1_234_567_890_000);
        assert_eq!(parse_time_input("+90", 1000).unwrap(), 91_000);
        assert_eq!(parse_time_input("-2m", 200_000).unwrap(), 80_000);
        assert!(parse_time_input("yesterday", 0).is_err());
    }

    #[test]
    fn offsets() {
        assert_eq!(format_offset(0), "+0s");
        assert_eq!(format_offset(-90_000), "−1m 30s");
        assert_eq!(format_offset(3_723_000), "+1h 2m 3s");
    }
}
