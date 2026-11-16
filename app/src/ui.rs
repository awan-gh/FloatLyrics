//! Slint windows: settings, tray menu, manual search prompt.
use crate::settings::Settings;
use crate::{with_app, App, MenuItem, Msg, SearchHit, SearchPrompt, SettingsWindow, SourceItem, TrayMenu};
use slint::{ComponentHandle, Model, ModelRc, SharedString, StandardListViewItem, VecModel};
use std::time::Instant;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, POINT};
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::WindowsAndMessaging::*;

// every settings field shown in the window: (slint property, settings field, kind)
macro_rules! each_field {
    ($m:ident) => {
        $m!(font_family, s); $m!(edge_style, s); $m!(text_color, s); $m!(highlight_color, s); $m!(highlight_color2, s);
        $m!(next_color, s); $m!(outline_color, s); $m!(bg_color, s); $m!(fill_style, s); $m!(bg_mode, s); $m!(align, s);
        $m!(anim_style, s); $m!(translation_lang, s); $m!(taskbar_side, s); $m!(key_click_through, s); $m!(peek_keys, s);
        $m!(key_hide, s); $m!(key_sooner, s); $m!(key_later, s); $m!(key_tap_sync, s); $m!(key_reset_offset, s);
        $m!(key_centre, s); $m!(key_taskbar, s); $m!(update_mode, s);
        $m!(font_size, f); $m!(outline_width, f); $m!(bg_opacity, f); $m!(next_scale, f); $m!(anim_ms, f);
        $m!(global_offset, f); $m!(sub_scale, f); $m!(taskbar_width, f);
        $m!(bold, b); $m!(italic, b); $m!(karaoke, b); $m!(duet_view, b); $m!(click_through, b); $m!(show_frame, b);
        $m!(hide_when_idle, b); $m!(hide_when_no_lyrics, b); $m!(show_romaji, b); $m!(romaji_main, b); $m!(sub_all_lines, b); $m!(taskbar_lyrics, b); $m!(autostart, b);
    };
}

fn slint_color(s: &str) -> slint::Color {
    let c = crate::render::color(s);
    slint::Color::from_argb_f32(c.a, c.r, c.g, c.b)
}

pub fn push_to_settings(a: &App) {
    let Some(w) = &a.settings_win else { return };
    let s = &a.s;
    macro_rules! push {
        ($f:ident, s) => { paste_set!(w, $f, SharedString::from(s.$f.as_str())) };
        ($f:ident, f) => { paste_set!(w, $f, s.$f as f32) };
        ($f:ident, b) => { paste_set!(w, $f, s.$f) };
    }
    each_field!(push);
    w.set_width_px(s.width as f32);
    w.set_lines_before(s.lines_before as f32);
    w.set_lines_after(s.lines_after as f32);
    w.set_overlay_opacity(s.opacity as f32);
    w.set_screen_width(unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) } as f32);
    push_swatches(w, s);
    w.set_lyrics_dir(s.lyrics_dir().display().to_string().into());
    w.set_sources(sources_model(s));
    w.set_hotkey_errors(a.hotkey_errors.clone().into());
    w.set_update_status(a.update_status.clone().into());
    w.set_version(crate::VERSION.into());
}

fn push_swatches(w: &SettingsWindow, s: &Settings) {
    w.set_sw_text(slint_color(&s.text_color)); w.set_sw_hl(slint_color(&s.highlight_color)); w.set_sw_hl2(slint_color(&s.highlight_color2));
    w.set_sw_next(slint_color(&s.next_color)); w.set_sw_outline(slint_color(&s.outline_color)); w.set_sw_bg(slint_color(&s.bg_color));
}

fn sources_model(s: &Settings) -> ModelRc<SourceItem> {
    let items: Vec<SourceItem> = s.source_order.iter().map(|id| SourceItem {
        name: id.name().into(), enabled: !s.source_disabled.contains(id),
        note: if id.word_timed() { "Word-timed".into() } else { "Line-timed or plain".into() },
    }).collect();
    ModelRc::new(VecModel::from(items))
}

fn pull_from_settings(a: &mut App, w: &SettingsWindow) {
    let old = a.s.clone();
    let s = &mut a.s;
    macro_rules! pull {
        ($f:ident, s) => { s.$f = paste_get!(w, $f).to_string() };
        ($f:ident, f) => { s.$f = paste_get!(w, $f) as f64 };
        ($f:ident, b) => { s.$f = paste_get!(w, $f) };
    }
    each_field!(pull);
    s.width = w.get_width_px() as f64;
    s.opacity = w.get_overlay_opacity() as f64;
    s.lines_before = w.get_lines_before().round() as u32;
    s.lines_after = w.get_lines_after().round() as u32;
    // invalid colours keep their previous value until the text becomes valid
    for (new, prev) in [(&mut s.text_color, &old.text_color), (&mut s.highlight_color, &old.highlight_color), (&mut s.highlight_color2, &old.highlight_color2),
        (&mut s.next_color, &old.next_color), (&mut s.outline_color, &old.outline_color), (&mut s.bg_color, &old.bg_color)] {
        if !crate::render::valid_color(new) { *new = prev.clone(); }
    }
    if s.autostart != old.autostart { crate::win::apply_autostart(s.autostart); }
    push_swatches(w, s);
    crate::mark_dirty(a);
}

// slint generates set_x/get_x; these keep the macro above readable
macro_rules! paste_set { ($w:expr, $f:ident, $v:expr) => { paste::paste! { $w.[<set_ $f>]($v) } }; }
macro_rules! paste_get { ($w:expr, $f:ident) => { paste::paste! { $w.[<get_ $f>]() } }; }
use {paste_get, paste_set};

pub fn open_settings() {
    if let Some(Some(w)) = with_app(|a| a.settings_win.as_ref().map(|w| w.as_weak())) {
        if let Some(w) = w.upgrade() { let _ = w.show(); }
        return;
    }
    let w = SettingsWindow::new().unwrap();
    let weak = w.as_weak();

    let wk = weak.clone();
    w.on_changed(move || { if let Some(w) = wk.upgrade() { with_app(|a| pull_from_settings(a, &w)); } });
    w.on_pick_color(move |which| pick_color(&which));
    let wk = weak.clone();
    w.on_filter_fonts(move |text| {
        let Some(w) = wk.upgrade() else { return };
        if let Some(m) = with_app(|a| font_items(a, &text)) { w.set_font_matches(m); }
    });
    let wk = weak.clone();
    w.on_pick_font(move |name| {
        with_app(|a| { a.s.font_family = name.to_string(); crate::mark_dirty(a); });
        if let Some(w) = wk.upgrade() { w.set_font_family(name); }
    });
    w.on_browse_google_fonts(open_gf_window);
    w.on_browse_lyrics_dir(|| { if let Some(dir) = rfd::FileDialog::new().set_title("Lyrics folder").pick_folder() { change_lyrics_dir(dir.display().to_string()); } });
    w.on_reset_lyrics_dir(|| change_lyrics_dir(String::new()));
    w.on_open_lyrics_dir(|| { with_app(|a| crate::win::open_folder(&a.s.lyrics_dir())); });
    let wk = weak.clone();
    w.on_move_source(move |i, j| {
        with_app(|a| {
            let n = a.s.source_order.len() as i32;
            if (0..n).contains(&i) && (0..n).contains(&j) { a.s.source_order.swap(i as usize, j as usize); crate::mark_dirty(a); }
            if let Some(w) = wk.upgrade() { w.set_sources(sources_model(&a.s)); }
        });
    });
    w.on_toggle_source(|i, on| {
        with_app(|a| {
            let Some(id) = a.s.source_order.get(i as usize).copied() else { return };
            a.s.source_disabled.retain(|x| *x != id);
            if !on { a.s.source_disabled.push(id); }
            crate::mark_dirty(a);
        });
    });
    w.on_add_font_file(|| {
        if let Some(files) = rfd::FileDialog::new().add_filter("Fonts", &["ttf", "otf", "ttc"]).pick_files() {
            let dir = Settings::fonts_dir();
            let _ = std::fs::create_dir_all(&dir);
            let n = files.iter().filter(|f| f.file_name().is_some_and(|name| std::fs::copy(f, dir.join(name)).is_ok())).count();
            with_app(|a| { reload_fonts(a); set_font_status(a, &format!("Added {n} font file(s)")); });
        }
    });
    w.on_open_fonts_dir(|| crate::win::open_folder(&Settings::fonts_dir()));
    w.on_check_updates(|| { with_app(|a| crate::update::check_in_background(a, true)); });
    w.on_retranslate(|| { with_app(crate::retranslate); });
    let wk = weak.clone();
    w.on_rekey(move || {
        with_app(|a| {
            a.hotkey_errors = crate::keys::register_all(a);
            if let Some(w) = wk.upgrade() { w.set_hotkey_errors(a.hotkey_errors.clone().into()); }
        });
    });
    w.window().on_close_requested(|| {
        with_app(|a| { a.s.save(); a.settings_win = None; crate::win::trim_memory(); });
        slint::CloseRequestResponse::HideWindow
    });
    with_app(|a| { w.set_font_matches(font_items(a, "")); a.settings_win = Some(w); push_to_settings(a); });
    with_app(|a| { if let Some(w) = &a.settings_win { let _ = w.show(); } });
}

/// Installed fonts (Windows + added), filtered by a search string.
fn font_items(a: &App, filter: &str) -> ModelRc<StandardListViewItem> {
    let t = filter.to_lowercase();
    let items: Vec<StandardListViewItem> = a.fonts.iter().filter(|f| t.is_empty() || f.to_lowercase().contains(&t)).map(|f| f.as_str().into()).collect();
    ModelRc::new(VecModel::from(items))
}

pub fn set_font_status(a: &App, s: &str) { if let Some(w) = &a.settings_win { w.set_font_status(s.into()); } }

pub fn reload_fonts(a: &mut App) {
    let custom = a.r.load_fonts(&Settings::fonts_dir());
    a.tb_r.load_fonts(&Settings::fonts_dir());
    let mut f = a.r.system_fonts();
    f.extend(custom);
    f.sort(); f.dedup();
    a.fonts = f;
}


/// Change the lyrics folder and move existing files there.
fn change_lyrics_dir(new: String) {
    with_app(|a| {
        let from = a.s.lyrics_dir();
        a.s.lyrics_dir = new;
        let to = a.s.lyrics_dir();
        if from != to {
            let _ = std::fs::create_dir_all(&to);
            for e in std::fs::read_dir(&from).into_iter().flatten().flatten() {
                let dest = to.join(e.file_name());
                if !dest.exists() && std::fs::rename(e.path(), &dest).is_err() {
                    // different drive: copy, then delete the original only if the copy worked
                    if std::fs::copy(e.path(), &dest).is_ok() { let _ = std::fs::remove_file(e.path()); }
                }
            }
        }
        crate::mark_dirty(a);
        if let Some(w) = &a.settings_win { w.set_lyrics_dir(to.display().to_string().into()); }
    });
}

fn pick_color(which: &str) {
    let Some(cur) = with_app(|a| match which { "text" => a.s.text_color.clone(), "hl" => a.s.highlight_color.clone(), "hl2" => a.s.highlight_color2.clone(),
        "next" => a.s.next_color.clone(), "outline" => a.s.outline_color.clone(), _ => a.s.bg_color.clone() }) else { return };
    let c = crate::render::color(&cur);
    let rgb = |c: &windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F| ((c.r * 255.0) as u32) | (((c.g * 255.0) as u32) << 8) | (((c.b * 255.0) as u32) << 16);
    static mut CUSTOM: [COLORREF; 16] = [COLORREF(0xFFFFFF); 16];
    let mut cc = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32, rgbResult: COLORREF(rgb(&c)),
        lpCustColors: &raw mut CUSTOM as *mut COLORREF, Flags: CC_FULLOPEN | CC_RGBINIT, ..Default::default()
    };
    if !unsafe { ChooseColorW(&mut cc) }.as_bool() { return; }
    let v = cc.rgbResult.0;
    let hex = format!("#{:02X}{:02X}{:02X}{:02X}", (c.a * 255.0).round() as u8, v & 0xFF, (v >> 8) & 0xFF, (v >> 16) & 0xFF); // keep alpha
    with_app(|a| {
        match which { "text" => a.s.text_color = hex, "hl" => a.s.highlight_color = hex, "hl2" => a.s.highlight_color2 = hex,
            "next" => a.s.next_color = hex, "outline" => a.s.outline_color = hex, _ => a.s.bg_color = hex }
        crate::mark_dirty(a);
        push_to_settings(a);
    });
}

// ---------------- tray menu ----------------
const MAIN: &[&str] = &["Click-through", "Hide / show", "Position", "Taskbar lyrics", "Settings…", "", "Lyrics sooner (+0.5s)", "Lyrics later (−0.5s)",
    "Tap-sync: next line starts now", "Reset this song's offset", "", "Search lyrics manually…", "Reload lyrics (skip cache)", "Open lyrics folder", "", "Exit"];
const POS: &[&str] = &["Back", "", "Top left", "Top centre", "Top right", "Centre", "Bottom left", "Bottom centre", "Bottom right", "", "Move to next monitor"];

thread_local! { static MENU_SHOWN: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) }; }

pub fn show_menu() {
    let created = with_app(|a| a.menu.is_none()).unwrap_or(false);
    if created {
        let m = TrayMenu::new().unwrap();
        m.on_chosen(|i| menu_chosen(i));
        with_app(|a| a.menu = Some(m));
    }
    with_app(|a| { a.menu_page = 0; refresh_menu(a); });
    with_app(|a| {
        let Some(m) = &a.menu else { return };
        let _ = m.show();
        // place the menu's bottom-right corner at the cursor, kept inside the work area
        let mut p = POINT::default();
        unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p); }
        let size = m.window().size();
        let (w, h) = (size.width as i32, size.height as i32);
        let mut wa = windows::Win32::Foundation::RECT::default();
        unsafe { let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut wa as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)); }
        let x = (p.x - w).max(wa.left.min(p.x));
        let y = if p.y - h < wa.top { p.y } else { p.y - h };
        m.window().set_position(slint::PhysicalPosition::new(x, y));
        unsafe { if let Ok(h) = FindWindowW(None, w!("FloatLyrics Menu")) { let _ = SetForegroundWindow(h); } }
    });
    MENU_SHOWN.with(|c| c.set(Some(Instant::now())));
}

fn refresh_menu(a: &App) {
    let Some(m) = &a.menu else { return };
    let items: Vec<MenuItem> = if a.menu_page == 0 {
        MAIN.iter().map(|s| MenuItem {
            // MAIN keeps the stable key that menu_chosen matches; only the shown text follows the state
            text: match *s { "Hide / show" if a.manual_hidden => "Show lyrics", "Hide / show" => "Hide lyrics", s => s }.into(),
            checked: match *s { "Click-through" => a.s.click_through, "Taskbar lyrics" => a.s.taskbar_lyrics, _ => false },
            arrow: if *s == "Position" { 1 } else { 0 },
        }).collect()
    } else { POS.iter().map(|s| MenuItem { text: (*s).into(), checked: false, arrow: if *s == "Back" { 2 } else { 0 } }).collect() };
    m.set_items(ModelRc::new(VecModel::from(items)));
    m.set_header(match &a.track { Some(t) => format!("{} — {}", t.title, t.artist), None => "FloatLyrics".into() }.into());
}

fn hide_menu(a: &App) { if let Some(m) = &a.menu { let _ = m.hide(); } MENU_SHOWN.with(|c| c.set(None)); }

pub fn menu_autohide(a: &App) {
    let Some(t) = MENU_SHOWN.with(|c| c.get()) else { return };
    if t.elapsed().as_millis() < 300 { return; }
    let fg = unsafe { GetForegroundWindow() };
    let menu = unsafe { FindWindowW(None, w!("FloatLyrics Menu")) }.unwrap_or_default();
    if fg != menu { hide_menu(a); }
}

fn menu_chosen(i: i32) {
    let page = with_app(|a| a.menu_page).unwrap_or(0);
    if page == 1 {
        let label = POS.get(i as usize).copied().unwrap_or("");
        if label == "Back" { with_app(|a| { a.menu_page = 0; refresh_menu(a); }); return; }
        with_app(|a| {
            hide_menu(a);
            let (fx, fy) = match label { "Top left" => (0.0, 0.0), "Top centre" => (0.5, 0.0), "Top right" => (1.0, 0.0), "Centre" => (0.5, 0.5),
                "Bottom left" => (0.0, 1.0), "Bottom centre" => (0.5, 1.0), "Bottom right" => (1.0, 1.0), _ => { crate::win::next_monitor(a); return; } };
            crate::win::move_to(a, fx, fy, None);
        });
        return;
    }
    let label = MAIN.get(i as usize).copied().unwrap_or("");
    if label == "Position" { with_app(|a| { a.menu_page = 1; refresh_menu(a); }); return; }
    with_app(|a| hide_menu(a));
    match label {
        "Settings…" => open_settings(),
        "Search lyrics manually…" => open_prompt(),
        "Exit" => { let _ = slint::quit_event_loop(); }
        _ => { with_app(|a| match label {
            "Click-through" => { a.s.click_through = !a.s.click_through; crate::mark_dirty(a); push_to_settings(a); }
            "Hide / show" => a.manual_hidden = !a.manual_hidden,
            "Taskbar lyrics" => { a.s.taskbar_lyrics = !a.s.taskbar_lyrics; crate::mark_dirty(a); push_to_settings(a); }
            "Lyrics sooner (+0.5s)" => crate::set_song_offset(a, a.song_offset + 0.5),
            "Lyrics later (−0.5s)" => crate::set_song_offset(a, a.song_offset - 0.5),
            "Tap-sync: next line starts now" => crate::tap_sync(a),
            "Reset this song's offset" => crate::set_song_offset(a, 0.0),
            "Reload lyrics (skip cache)" => crate::start_load(a, true),
            "Open lyrics folder" => crate::win::open_folder(&a.s.lyrics_dir()),
            _ => {}
        }); }
    }
}

// ---------------- manual search ----------------
fn rank(l: &engine::Lyric) -> u8 { if l.lines.iter().any(|x| x.syls.is_some()) { 3 } else if l.synced { 2 } else { 1 } }

fn hit_row(l: &engine::Lyric) -> SearchHit {
    let kind = match rank(l) { 3 => "Word-synced", 2 => "Line-synced", _ => "Plain text (timing estimated)" };
    let preview: Vec<&str> = l.lines.iter().map(|x| x.text.trim()).filter(|t| !t.is_empty()).take(2).collect();
    SearchHit { title: l.source.as_str().into(), detail: format!("{kind} · {} lines", l.lines.len()).into(), preview: preview.join("  /  ").into() }
}

fn open_prompt() {
    if let Some(Some(w)) = with_app(|a| a.prompt.as_ref().map(|w| w.as_weak())) { if let Some(w) = w.upgrade() { let _ = w.show(); return; } }
    let p = SearchPrompt::new().unwrap();
    let q = with_app(|a| a.track.as_ref().map(|t| format!("{} {}", engine::clean_artist(&t.artist), engine::clean_title(&t.title))).unwrap_or_default()).unwrap_or_default();
    p.set_query(q.clone().into());
    p.on_search(|q| { with_app(|a| start_search(a, q.to_string())); });
    p.on_pick(|i| { with_app(|a| pick_hit(a, i as usize)); });
    p.window().on_close_requested(|| { with_app(|a| { a.prompt = None; a.hits.clear(); }); slint::CloseRequestResponse::HideWindow });
    let _ = p.show();
    with_app(|a| { a.prompt = Some(p); if !q.trim().is_empty() { start_search(a, q); } });
}

/// Search every enabled source in parallel; results stream in as each source answers.
fn start_search(a: &mut App, q: String) {
    a.hits.clear();
    a.search_done = 0;
    a.search_gen += 1;
    let (gen, order, dur) = (a.search_gen, a.s.sources(), a.track.as_ref().map(|t| t.duration).unwrap_or(0.0));
    if let Some(p) = &a.prompt { p.set_hits(ModelRc::new(VecModel::from(Vec::<SearchHit>::new()))); p.set_applied(-1); p.set_message(format!("Searching {} sources…", order.len()).into()); }
    for src in order {
        let (tx, q) = (a.tx.clone(), q.clone());
        std::thread::spawn(move || {
            let found = engine::search_query_many(src, &q, dur, 4).unwrap_or_default();
            let _ = tx.send(Msg::SearchHits { gen, found });
        });
    }
}

pub fn on_search_hits(a: &mut App, gen: u64, found: Vec<engine::Lyric>) {
    if gen != a.search_gen { return; }
    a.search_done += 1;
    a.hits.extend(found);
    a.hits.sort_by_key(|l| std::cmp::Reverse(rank(l))); // stable: keeps source priority within a rank
    let Some(p) = &a.prompt else { return };
    p.set_hits(ModelRc::new(VecModel::from(a.hits.iter().map(hit_row).collect::<Vec<_>>())));
    p.set_message(format!("{} result(s){}", a.hits.len(), if a.search_done < a.s.sources().len() { " — still searching…" } else { "" }).into());
}

fn pick_hit(a: &mut App, i: usize) {
    let Some(l) = a.hits.get(i).cloned() else { return };
    if let Some(t) = &a.track {
        let dir = a.s.lyrics_dir();
        let name: String = format!("{} - {}", t.artist, t.title).chars().map(|c| if r#"<>:"/\|?*"#.contains(c) { '_' } else { c }).take(150).collect();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("{name}.json")), serde_json::to_string(&l).unwrap_or_default());
    }
    a.status = format!("manual · {}", l.source);
    a.lyric = Some(l);
    crate::retranslate(a);
    crate::win::tray_tip(a);
    if let Some(p) = &a.prompt { p.set_applied(i as i32); }
}

// ---------------- Google Fonts browser ----------------
use crate::gfonts::{self, Family};
use crate::{GfRow, GoogleFontsWindow};

fn light_theme() -> bool {
    use windows::Win32::System::Registry::*;
    let mut v = 0u32; let mut n = 4u32;
    unsafe { RegGetValueW(HKEY_CURRENT_USER, w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"), w!("AppsUseLightTheme"), RRF_RT_REG_DWORD, None, Some(&mut v as *mut _ as *mut _), Some(&mut n)).is_ok() && v == 1 }
}

pub fn open_gf_window() {
    if let Some(Some(w)) = with_app(|a| a.gf_win.as_ref().map(|w| w.as_weak())) {
        if let Some(w) = w.upgrade() { let _ = w.show(); }
        return;
    }
    let w = GoogleFontsWindow::new().unwrap();
    wire_gfonts(&w);
    w.window().on_close_requested(|| { with_app(|a| { a.gf_win = None; a.gf_model = None; crate::win::trim_memory(); }); slint::CloseRequestResponse::HideWindow });
    let _ = w.show();
    with_app(|a| {
        a.gf_win = Some(w);
        if !a.gf.is_empty() { gf_filter(a); return; }
        if let Some(w) = &a.gf_win { w.set_gf_status("Loading Google Fonts catalogue…".into()); }
        let tx = a.tx.clone();
        std::thread::spawn(move || { let _ = tx.send(Msg::GfCatalog(gfonts::catalog())); });
    });
}

fn wire_gfonts(w: &GoogleFontsWindow) {
    w.on_gf_filter(|| { with_app(gf_filter); });
    w.on_gf_select(|name| { with_app(|a| gf_select(a, name.to_string())); });
    w.on_gf_sample_changed(|| { with_app(|a| { if let Some(n) = a.gf_win.as_ref().map(|w| w.get_gf_selected().to_string()) { if !n.is_empty() { gf_select(a, n); } } }); });
    w.on_gf_use(|name| { with_app(|a| {
        let name = name.to_string();
        if let Some(w) = &a.gf_win { w.set_gf_status(format!("Downloading {name}…").into()); }
        let tx = a.tx.clone();
        std::thread::spawn(move || { let r = gfonts::install(&name).map(|_| name); let _ = tx.send(Msg::GfInstalled(r)); });
    }); });
}

pub fn gf_on_catalog(a: &mut App, r: Result<Vec<Family>, String>) {
    match r {
        Ok(c) => { a.gf = c; if let Some(w) = &a.gf_win { w.set_gf_status("".into()); } gf_filter(a); }
        Err(e) => if let Some(w) = &a.gf_win { w.set_gf_status(format!("Couldn't load Google Fonts: {e}").into()); },
    }
}

/// Show the first 60 matches and fetch tiny previews of their names in the background.
fn gf_filter(a: &mut App) {
    let Some(w) = &a.gf_win else { return };
    let (q, cat) = (w.get_gf_search().to_lowercase(), w.get_gf_category().to_string());
    let hits: Vec<Family> = a.gf.iter().filter(|f| (cat == "All" || f.category.eq_ignore_ascii_case(&cat)) && f.name.to_lowercase().contains(&q)).take(60).cloned().collect();
    let rows: Vec<GfRow> = hits.iter().map(|f| GfRow { name: f.name.as_str().into(), category: f.category.as_str().into(), preview: slint::Image::default(), installed: a.fonts.contains(&f.name) }).collect();
    let model = std::rc::Rc::new(VecModel::from(rows));
    w.set_gf_rows(ModelRc::from(model.clone()));
    a.gf_model = Some(model);
    a.gf_gen += 1;
    let (gen, tx) = (a.gf_gen, a.tx.clone());
    std::thread::spawn(move || {
        for f in hits {
            let Ok(path) = gfonts::preview_subset(&f.name, &f.name) else { continue };
            if tx.send(Msg::GfPreview { gen, name: f.name, path, big: false }).is_err() { break; }
        }
    });
}

fn gf_select(a: &mut App, name: String) {
    let Some(w) = &a.gf_win else { return };
    w.set_gf_selected(name.as_str().into());
    let sample = w.get_gf_sample().to_string();
    let (gen, tx) = (a.gf_gen, a.tx.clone());
    std::thread::spawn(move || {
        if let Ok(path) = gfonts::preview_subset(&name, &sample) { let _ = tx.send(Msg::GfPreview { gen, name, path, big: true }); }
    });
}

pub fn gf_on_preview(a: &mut App, gen: u64, name: String, path: std::path::PathBuf, big: bool) {
    let Some(w) = &a.gf_win else { return };
    let to_image = |(wd, ht, px): (u32, u32, Vec<u8>)| {
        let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&px, wd, ht);
        slint::Image::from_rgba8_premultiplied(buf)
    };
    if big {
        if w.get_gf_selected() != name.as_str() { return; }
        let sample = w.get_gf_sample().to_string();
        if let Some(img) = a.r.preview(&path, &name, &sample, 44.0, crate::render::color(&a.s.text_color), crate::render::color(&a.s.outline_color), a.s.outline_width as f32).ok().map(to_image) { w.set_gf_preview(img); }
        return;
    }
    if gen != a.gf_gen { return; } // list changed since this was requested
    let fg = if light_theme() { "#FF202020" } else { "#FFF2F2F2" };
    let Some(img) = a.r.preview(&path, &name, &name, 24.0, crate::render::color(fg), crate::render::color("#00000000"), 0.0).ok().map(to_image) else { return };
    let Some(m) = &a.gf_model else { return };
    for i in 0..m.row_count() {
        if let Some(mut r) = m.row_data(i) { if r.name == name.as_str() { r.preview = img; m.set_row_data(i, r); break; } }
    }
}

pub fn gf_on_installed(a: &mut App, r: Result<String, String>) {
    match r {
        Ok(name) => {
            reload_fonts(a);
            a.s.font_family = name.clone();
            crate::mark_dirty(a);
            push_to_settings(a);
            if let Some(w) = &a.settings_win { w.set_font_matches(font_items(a, "")); }
            // applied: close the popup
            if let Some(w) = a.gf_win.take() { let _ = w.hide(); }
            a.gf_model = None;
        }
        Err(e) => if let Some(w) = &a.gf_win { w.set_gf_status(e.into()); },
    }
}
