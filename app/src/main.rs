#![windows_subsystem = "windows"]
mod gfonts;
mod keys;
mod media;
mod render;
mod settings;
mod ui;
mod update;
mod win;

use engine::{Line, Lyric, Track};
use render::{BgMode, Edge, Fill, Renderer, Row, Style};
use settings::Settings;
use std::cell::RefCell;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::DirectWrite::*;

slint::include_modules!();

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Results coming back from worker threads.
pub enum Msg {
    Lyric { key: String, lyric: Option<Lyric>, status: String },
    Status(String),
    Update(update::Event),
    FontsChanged(String),
    GfCatalog(Result<Vec<gfonts::Family>, String>),
    GfPreview { gen: u64, name: String, path: std::path::PathBuf, big: bool },
    GfInstalled(Result<String, String>),
    SearchHits { gen: u64, found: Vec<Lyric> },
}

pub struct App {
    pub s: Settings,
    pub r: Renderer,
    pub tb_r: Renderer,
    pub hwnd: HWND,
    pub tb_hwnd: HWND,
    pub now: Arc<Mutex<media::NowPlaying>>,
    pub track: Option<Track>,
    pub lyric: Option<Lyric>,
    pub song_key: String,
    pub status: String,
    pub song_offset: f64,
    pub tx: Sender<Msg>,
    rx: Receiver<Msg>,
    pub manual_hidden: bool,
    peek: bool,
    applied_ct: Option<bool>,
    anim: Option<(Instant, f32)>,
    anim_index: i64,
    anim_key: String,
    pub dirty: Option<Instant>,
    pub settings_win: Option<SettingsWindow>,
    pub menu: Option<TrayMenu>,
    pub menu_page: u8,
    pub prompt: Option<SearchPrompt>,
    pub hits: Vec<Lyric>,
    pub search_gen: u64,
    pub search_done: usize,
    pub fonts: Vec<String>,
    pub gf: Vec<gfonts::Family>,
    pub gf_gen: u64,
    pub gf_model: Option<std::rc::Rc<slint::VecModel<GfRow>>>,
    pub gf_win: Option<GoogleFontsWindow>,
    pub hotkey_errors: String,
    pub pending_update: Option<update::Pending>,
    pub update_downloaded: bool,
    last_update_check: Instant,
    pub update_status: String,
    started: Instant,
    last_place: Instant,
    last_frame: Instant,
    applied_topmost: Option<bool>,
    last_topmost: Instant,
    hover_mix: f32,
    hover_target: f32,
    pub offset_win: Option<OffsetWindow>,
}

thread_local! { pub static APP: RefCell<Option<App>> = const { RefCell::new(None) }; }

/// Run `f` on the app state; skipped (returns None) if it is already borrowed (re-entrant window message).
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn main() {
    velopack::VelopackApp::build().run(); // handles install/update hooks, must run first
    if std::env::args().any(|x| x == "--probe-media") {
        win::attach_console();
        win::com_init_mta();
        if let Err(e) = media::probe(std::env::args().any(|x| x == "seek")) { println!("probe failed: {e}"); }
        return;
    }
    if std::env::args().any(|x| x == "--check-settings") {
        win::attach_console();
        let path = std::env::args().skip_while(|x| x != "--check-settings").nth(1);
        let s = path.map(|p| Settings::load_from(std::path::Path::new(&p))).unwrap_or_else(Settings::load);
        ui::check_settings_roundtrip(s);
        return;
    }
    if !win::single_instance() { return; }
    win::init_process();

    let (tx, rx) = channel();
    let now = Arc::new(Mutex::new(media::NowPlaying::default()));
    media_thread(now.clone());

    settings::backup_on_launch();
    let s = Settings::load();
    let mut r = Renderer::new().expect("Direct2D unavailable");
    let custom = r.load_fonts(&Settings::fonts_dir());
    let mut fonts = r.system_fonts();
    fonts.extend(custom);
    fonts.sort(); fonts.dedup();
    let mut tb_r = Renderer::new().expect("Direct2D unavailable");
    tb_r.load_fonts(&Settings::fonts_dir());

    let hwnd = win::create_overlay(false);
    let tb_hwnd = win::create_overlay(true);
    let app = App {
        s, r, tb_r, hwnd, tb_hwnd, now, track: None, lyric: None, song_key: String::new(), status: String::new(),
        song_offset: 0.0, tx, rx, manual_hidden: false, peek: false, applied_ct: None, anim: None, anim_index: -2,
        anim_key: String::new(), dirty: None, settings_win: None, menu: None, menu_page: 0, prompt: None, hits: vec![], search_gen: 0, search_done: 0, fonts, gf: vec![], gf_gen: 0, gf_model: None, gf_win: None,
        hotkey_errors: String::new(), pending_update: None, update_downloaded: false, last_update_check: Instant::now(), update_status: String::new(), started: Instant::now(),
        last_place: Instant::now() - Duration::from_secs(5),
        last_frame: Instant::now(),
        applied_topmost: None,
        last_topmost: Instant::now(),
        hover_mix: 0.0,
        hover_target: 0.0,
        offset_win: None,
    };
    APP.with(|a| *a.borrow_mut() = Some(app));
    with_app(|a| {
        a.hotkey_errors = keys::register_all(a);
        win::restore_pos(a);
        win::apply_autostart(a.s.autostart);
        win::tray_add(a.hwnd);
        update::check_in_background(a, false);
    });
    if std::env::args().any(|x| x == "--settings") { ui::open_settings(); }
    if std::env::args().any(|x| x == "--google-fonts") { ui::open_gf_window(); }
    if std::env::args().any(|x| x == "--timing") { ui::open_offset(); }

    // startup touches lots of one-off pages (font lists, D2D init); give them back once settled
    slint::Timer::single_shot(Duration::from_secs(5), win::trim_memory);
    win::start_frame_clock(hwnd);
    slint::run_event_loop_until_quit().ok();
    with_app(|a| { a.s.save(); win::tray_remove(a.hwnd); });
}

fn media_thread(now: Arc<Mutex<media::NowPlaying>>) {
    std::thread::spawn(move || {
        win::com_init_mta();
        let mut m = media::Media::new();
        let mut fails = 0;
        loop {
            match m.poll() {
                Ok(()) => { fails = 0; *now.lock().unwrap() = m.state.clone(); }
                // the media manager/session can go stale (audio stack reset, player restart): reconnect
                // instead of showing the last reading forever
                Err(_) => { fails += 1; if fails >= 3 { m = media::Media::new(); fails = 0; } }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

pub static FRAME_PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called every display refresh. Redraws at full rate only while something moves; otherwise ~10 fps.
pub fn frame() {
    with_app(|a| {
        // hover fade: only while click-through (otherwise the mouse is there to drag it)
        let fade_on_hover = a.applied_ct == Some(true) && a.s.hover_opacity < 0.999;
        let over = fade_on_hover && match win::cursor_in(a.hwnd) {
            None => false,
            Some((x, y)) if a.s.hover_trigger == "Lyrics text" => {
                let m = 4.0 * win::dpi_scale(a.hwnd); // a little slack around the glyphs
                a.r.text_rects.iter().any(|r| x >= r.0 - m && x < r.0 + r.2 + m && y >= r.1 - m && y < r.1 + r.3 + m)
            }
            Some(_) => true,
        };
        a.hover_target = if over { 1.0 } else { 0.0 };
        let dt = a.last_frame.elapsed().as_secs_f32();
        let fading = (a.hover_mix - a.hover_target).abs() > 0.001;
        let busy = fading || a.anim.is_some() || a.s.fill_style == "Rainbow" || (a.lyric.is_some() && a.s.karaoke && a.now.lock().unwrap().playing);
        if busy || a.last_frame.elapsed() >= Duration::from_millis(100) {
            let step = (dt / 0.15).min(1.0); // ~150 ms fade
            a.hover_mix += (a.hover_target - a.hover_mix).clamp(-step, step);
            a.last_frame = Instant::now();
            tick(a);
        }
    });
}

pub fn song_key(t: &Option<Track>) -> String { t.as_ref().map(|t| format!("{} - {}", t.artist, t.title)).unwrap_or_default() }

fn cache_name(t: &Track) -> String {
    let n: String = format!("{} - {}", t.artist, t.title).chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    n.chars().take(150).collect()
}

/// Load lyrics on a worker thread: your own .lrc, cache, then online sources in priority order.
pub fn start_load(a: &mut App, skip_cache: bool) {
    let Some(t) = a.track.clone() else { return };
    let key = a.song_key.clone();
    let (dir, order, prefer_roma, lang, roma) = (a.s.lyrics_dir(), a.s.sources(), a.s.romaji_main || a.s.show_romaji, a.s.translation_lang.clone(), a.s.romaji_main || a.s.show_romaji);
    let tx = a.tx.clone();
    a.status = "searching…".into();
    std::thread::spawn(move || {
        let name = cache_name(&t);
        let lrc = dir.join(format!("{name}.lrc"));
        let json = dir.join(format!("{name}.json"));
        if skip_cache { let _ = std::fs::remove_file(&json); }
        let mut lyric = None;
        let mut source = String::new();
        if let Ok(text) = std::fs::read_to_string(&lrc) { lyric = Some(engine::parse_lrc(&text, t.duration)); source = "local .lrc file".into(); }
        if lyric.is_none() && !skip_cache {
            if let Some(l) = std::fs::read_to_string(&json).ok().and_then(|x| serde_json::from_str::<Lyric>(&x).ok()) { source = format!("cache · {}", l.source); lyric = Some(l); }
        }
        if lyric.is_none() {
            if let Some(l) = engine::find(&t, &order, prefer_roma) {
                if l.synced { let _ = std::fs::create_dir_all(&dir); let _ = std::fs::write(&json, serde_json::to_string(&l).unwrap_or_default()); }
                source = l.source.clone();
                lyric = Some(l);
            }
        }
        let mut status = match &lyric {
            None => "no lyrics found".to_string(),
            Some(l) => format!("{} · {source}", if l.lines.iter().any(|x| x.syls.is_some()) { "word-synced" } else if l.synced { "synced" } else { "unsynced, timing estimated" }),
        };
        if let Some(l) = lyric.as_mut() {
            if !lang.trim().is_empty() || roma {
                if let Err(e) = engine::translate::apply(l, &lang, roma) { status += &format!(" · translation failed: {e}"); }
            }
        }
        let _ = tx.send(Msg::Lyric { key, lyric, status });
    });
}

pub fn retranslate(a: &mut App) {
    let Some(mut l) = a.lyric.clone() else { return };
    for x in &mut l.lines { x.trans = None; }
    let (lang, roma, key, status, tx) = (a.s.translation_lang.clone(), a.s.romaji_main || a.s.show_romaji, a.song_key.clone(), a.status.clone(), a.tx.clone());
    std::thread::spawn(move || {
        let _ = engine::translate::apply(&mut l, &lang, roma);
        let _ = tx.send(Msg::Lyric { key, lyric: Some(l), status });
    });
}

pub fn mark_dirty(a: &mut App) { a.dirty = Some(Instant::now()); }

pub fn set_song_offset(a: &mut App, v: f64) {
    a.song_offset = (v * 100.0).round() / 100.0;
    if a.song_key.is_empty() { return; }
    ui::refresh_offset(a);
    if a.song_offset == 0.0 { a.s.song_offsets.remove(&a.song_key); } else { a.s.song_offsets.insert(a.song_key.clone(), a.song_offset); }
    mark_dirty(a);
}

pub fn position(a: &App) -> f64 { a.now.lock().unwrap().position() + a.s.global_offset + a.song_offset }

/// "The next line starts now."
/// "-5", "+1.3", "-1.3 sec", "2s" -> seconds. None if it isn't a number.
pub fn parse_shift(s: &str) -> Option<f64> {
    let t: String = s.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    let t = ["seconds", "second", "secs", "sec", "s"].iter().find_map(|u| t.strip_suffix(u)).unwrap_or(&t);
    let t = t.strip_prefix('+').unwrap_or(t);
    t.parse::<f64>().ok().filter(|v| v.is_finite())
}

pub fn tap_sync(a: &mut App) {
    let Some(l) = &a.lyric else { return };
    if l.lines.is_empty() { return; }
    let raw = a.now.lock().unwrap().position() + a.s.global_offset;
    let i = index(l, raw + a.song_offset);
    let target = l.lines[((i + 1).max(0) as usize).min(l.lines.len() - 1)].time;
    set_song_offset(a, target - raw);
}

pub fn index(l: &Lyric, pos: f64) -> i64 {
    let mut i = -1;
    for (k, x) in l.lines.iter().enumerate() { if x.time <= pos { i = k as i64 } else { break } }
    i
}

/// Characters sung at `pos` (fractional) for the karaoke wipe.
#[cfg(test)]
mod tests {
    #[test]
    fn shifts() {
        assert_eq!(super::parse_shift("- 5 sec"), Some(-5.0));
        assert_eq!(super::parse_shift("+1.3"), Some(1.3));
        assert_eq!(super::parse_shift("-1.3s"), Some(-1.3));
        assert_eq!(super::parse_shift("2 seconds"), Some(2.0));
        assert_eq!(super::parse_shift("abc"), None);
        assert_eq!(super::parse_shift(""), None);
    }
}

pub fn sung(l: &Line, pos: f64, end: f64) -> f64 {
    let n = l.text.chars().count() as f64;
    if let Some(syls) = &l.syls {
        let mut c = 0.0;
        for y in syls {
            if pos >= y.end { c = (y.pos + y.len) as f64; continue; }
            if pos > y.start { c = y.pos as f64 + y.len as f64 * (pos - y.start) / (y.end - y.start).max(0.01); }
            break;
        }
        return c.min(n);
    }
    n * ((pos - l.time) / (end - l.time).max(0.1)).clamp(0.0, 1.0)
}

fn tick(a: &mut App) {
    // worker results
    while let Ok(m) = a.rx.try_recv() {
        match m {
            Msg::Lyric { key, lyric, status } => if key == a.song_key { a.lyric = lyric; a.status = status; win::tray_tip(a); },
            Msg::Status(s) => a.status = s,
            Msg::Update(e) => update::on_event(a, e),
            Msg::FontsChanged(st) => { ui::reload_fonts(a); ui::set_font_status(a, &st); }
            Msg::GfCatalog(r) => ui::gf_on_catalog(a, r),
            Msg::GfPreview { gen, name, path, big } => ui::gf_on_preview(a, gen, name, path, big),
            Msg::GfInstalled(r) => ui::gf_on_installed(a, r),
            Msg::SearchHits { gen, found } => ui::on_search_hits(a, gen, found),
        }
    }
    // song change
    let now = a.now.lock().unwrap().clone();
    let key = song_key(&now.track);
    if key != a.song_key {
        a.song_key = key.clone();
        a.track = now.track.clone();
        a.lyric = None;
        a.song_offset = a.s.song_offsets.get(&key).copied().unwrap_or(0.0);
        a.status.clear();
        if a.track.is_some() { start_load(a, false); }
        ui::refresh_offset(a);
        win::tray_tip(a);
    }
    // periodic update check (startup check already ran); skip once an update is waiting to be installed
    let every = (a.s.update_check_hours.round() as u64) * 3600;
    if every > 0 && a.s.update_mode != "Off" && a.pending_update.is_none() && !a.update_downloaded
        && a.last_update_check.elapsed() >= Duration::from_secs(every) {
        a.last_update_check = Instant::now();
        update::check_in_background(a, false);
    }
    // debounced save
    if a.dirty.is_some_and(|d| d.elapsed() > Duration::from_millis(600)) { a.dirty = None; a.s.save(); }
    // click-through + hold-to-grab
    let p = a.s.click_through && keys::peek_held(&a.s.peek_keys);
    a.peek = p;
    let ct = a.s.click_through && !p;
    if a.applied_ct != Some(ct) { win::set_click_through(a.hwnd, ct); a.applied_ct = Some(ct); }
    // other always-on-top windows activated later sit above us; take the top spot back every second
    if a.applied_topmost != Some(a.s.always_on_top) || (a.s.always_on_top && a.last_topmost.elapsed() >= Duration::from_secs(1)) {
        win::set_topmost(a.hwnd, a.s.always_on_top);
        a.applied_topmost = Some(a.s.always_on_top);
        a.last_topmost = Instant::now();
    }
    ui::menu_autohide(a);
    draw(a, !ct);
}

fn display_text(a: &App, l: &Line) -> String {
    if a.s.romaji_main && l.roma.as_deref().is_some_and(|r| !r.is_empty()) { return l.roma.clone().unwrap(); }
    if l.text.is_empty() { "♪".into() } else { l.text.clone() }
}

fn sub_text(a: &App, l: Option<&Line>, show: bool) -> String {
    let Some(l) = l else { return String::new() };
    if !show { return String::new(); }
    let mut parts = vec![];
    let roma_main = a.s.romaji_main && l.roma.as_deref().is_some_and(|r| !r.is_empty());
    if roma_main { if a.s.show_romaji { parts.push(l.text.clone()); } }
    else if a.s.show_romaji { if let Some(r) = l.roma.as_ref().filter(|r| !r.is_empty()) { parts.push(r.clone()); } }
    if !a.s.translation_lang.trim().is_empty() { if let Some(t) = l.trans.as_ref().filter(|t| !t.is_empty()) { parts.push(t.clone()); } }
    parts.join("\n")
}

fn align_of(a: &App, l: Option<&Line>) -> DWRITE_TEXT_ALIGNMENT {
    if a.s.duet_view { if let Some(l) = l { if l.align == 1 { return DWRITE_TEXT_ALIGNMENT_LEADING; } if l.align == 2 { return DWRITE_TEXT_ALIGNMENT_TRAILING; } } }
    match a.s.align.as_str() { "Left" => DWRITE_TEXT_ALIGNMENT_LEADING, "Right" => DWRITE_TEXT_ALIGNMENT_TRAILING, _ => DWRITE_TEXT_ALIGNMENT_CENTER }
}

fn draw(a: &mut App, interactive: bool) {
    let s = &a.s;
    let scale = win::dpi_scale(a.hwnd);
    let no_lyrics = a.track.is_some() && a.lyric.is_none() && a.status.starts_with("no lyrics");
    let hidden = a.manual_hidden || (!interactive && ((a.track.is_none() && s.hide_when_idle) || (no_lyrics && s.hide_when_no_lyrics)));

    // what to show
    let mut lines: Vec<(String, String, Option<Line>)> = vec![]; // (text, sub, line) for before.., current, after..
    let cur_text: String;
    let (mut sungc, mut idx) = (0.0, 0i64);
    let total = (s.lines_before + 1 + s.lines_after) as i64;
    match (&a.track, &a.lyric) {
        (None, _) => {
            cur_text = "FloatLyrics".into();
            for k in 0..total { lines.push((if k == s.lines_before as i64 + 1 { "Waiting for music · drag to move · double-click to centre".into() } else { String::new() }, String::new(), None)); }
        }
        (Some(t), None) => {
            cur_text = format!("♪ {}", t.title);
            for k in 0..total { lines.push((if k == s.lines_before as i64 + 1 { format!("{} · {}", t.artist, a.status) } else { String::new() }, String::new(), None)); }
        }
        (Some(t), Some(l)) => {
            let pos = position(a);
            idx = index(l, pos);
            for k in (idx - s.lines_before as i64)..=(idx + s.lines_after as i64) {
                let line = if k >= 0 && (k as usize) < l.lines.len() { Some(l.lines[k as usize].clone()) } else { None };
                let text = line.as_ref().map(|x| display_text(a, x)).unwrap_or_default();
                let sub = sub_text(a, line.as_ref(), k == idx || s.sub_all_lines);
                lines.push((text, sub, line));
            }
            if idx < 0 { cur_text = format!("♪ {}{}", t.title, if t.artist.is_empty() { String::new() } else { format!(" — {}", t.artist) }); }
            else {
                let cl = &l.lines[idx as usize];
                cur_text = display_text(a, cl);
                if s.karaoke && l.synced && !cl.text.is_empty() {
                    let end = l.lines.get(idx as usize + 1).map(|n| n.time).unwrap_or(cl.time + 5.0);
                    sungc = sung(cl, pos, end);
                    // word timing indexes the original script; map the same fraction onto the romaji
                    if cur_text != cl.text { sungc = sungc / cl.text.chars().count().max(1) as f64 * cur_text.chars().count() as f64; }
                }
            }
        }
    }
    let ci = s.lines_before as usize;
    if let Some(c) = lines.get_mut(ci) { c.0 = cur_text.clone(); }

    // animation on line change
    if a.lyric.is_some() && idx != a.anim_index && a.song_key == a.anim_key && s.anim_style != "Off" {
        a.anim = Some((Instant::now(), if idx > a.anim_index { 1.0 } else { -1.0 }));
    }
    a.anim_index = idx; a.anim_key = a.song_key.clone();
    let (mut slide, mut fade) = (0.0f32, 1.0f32);
    if let Some((t0, dir)) = a.anim {
        let p = (t0.elapsed().as_secs_f64() * 1000.0 / s.anim_ms.max(1.0)).min(1.0) as f32;
        let e = 1.0 - (1.0 - p).powi(3); // ease-out cubic
        let shift = (s.font_size as f32 * 1.3 * scale) * dir;
        if s.anim_style == "Slide" { slide = shift * (1.0 - e); fade = 0.4 + 0.6 * e; } else { fade = 0.1 + 0.9 * e; }
        if p >= 1.0 { a.anim = None; }
    }

    let fill = match s.fill_style.as_str() { "Gradient" => Fill::Gradient, "Rainbow" => Fill::Rainbow, _ => Fill::Solid };
    let edge = match s.edge_style.as_str() { "None" => Edge::None, "Shadow" => Edge::Shadow, "Raised" => Edge::Raised, "Depressed" => Edge::Depressed, _ => Edge::Outline };
    let bg_mode = match s.bg_mode.as_str() { "Block" => BgMode::Block, "Line" => BgMode::Line, _ => BgMode::None };
    let frame = interactive && s.show_frame;
    let mut bg = render::color(&s.bg_color);
    bg.a *= s.bg_opacity as f32;
    if frame && bg_mode != BgMode::Block { bg.a = bg.a.max(0.35); }
    let style = Style {
        family: s.font_family.clone(), bold: s.bold, italic: s.italic, edge, bg_mode: if frame { BgMode::Block } else { bg_mode },
        outline: s.outline_width as f32 * scale, outline_color: render::color(&s.outline_color),
        hl: render::color(&s.highlight_color), hl2: render::color(&s.highlight_color2), fill,
        // alpha-0 pixels don't receive the mouse, so keep a hint of background while movable
        bg: if interactive && bg.a < 0.004 { render::D2D1_COLOR_F_TRANSPARENT_HIT } else { bg },
        frame, opacity: s.opacity as f32 * (1.0 - a.hover_mix * (1.0 - s.hover_opacity as f32)), padding: 12.0 * scale, slide, fade, clock: a.started.elapsed().as_secs_f64(), text_aa: s.text_antialias.clone(),
    };
    let (main_c, other_c) = (render::color(&s.text_color), render::color(&s.next_color));
    // fill style also colours the whole current line when the wipe is off
    let cur_sung = if !s.karaoke && fill != Fill::Solid { cur_text.chars().count() as f64 } else { sungc };
    let rows: Vec<Row> = if hidden { vec![] } else {
        lines.iter().enumerate().map(|(k, (text, sub, line))| {
            let cur = k == ci;
            Row {
                text, sub, size: (s.font_size * if cur { 1.0 } else { s.next_scale }) as f32 * scale,
                color: if cur { main_c } else { other_c }, sub_color: other_c, sung: if cur { cur_sung } else { 0.0 },
                align: align_of(a, line.as_ref()), sub_scale: (s.sub_scale / if cur { 1.0 } else { s.next_scale }) as f32,
            }
        }).collect()
    };
    let width = (s.width as f32 * scale) as i32;
    let style = if hidden { Style { bg: render::D2D1_COLOR_F_CLEAR, frame: false, ..style } } else { style };
    let _ = a.r.draw(a.hwnd, width, &rows, &style, if hidden { Some(1) } else { None });

    // taskbar strip
    if s.taskbar_lyrics {
        let next = lines.get(ci + 1).map(|x| x.0.clone()).unwrap_or_default();
        let empty = a.track.is_none() || a.manual_hidden;
        if a.last_place.elapsed() > Duration::from_secs(1) { a.last_place = Instant::now(); win::place_taskbar(a.tb_hwnd, s.taskbar_width, &s.taskbar_side); }
        let (w, h) = win::taskbar_size(a);
        if w > 0 {
            let tb_style = Style { bg: render::D2D1_COLOR_F_CLEAR, bg_mode: BgMode::None, frame: false, slide: 0.0, fade: 1.0, padding: 2.0, outline: style.outline.min(1.5), ..style };
            let al = if s.taskbar_side == "Left" { DWRITE_TEXT_ALIGNMENT_LEADING } else { DWRITE_TEXT_ALIGNMENT_TRAILING };
            let rows = if empty { vec![] } else { vec![
                Row { text: &cur_text, sub: "", size: h as f32 * 0.36, color: main_c, sub_color: other_c, sung: cur_sung, align: al, sub_scale: 0.5 },
                Row { text: &next, sub: "", size: h as f32 * 0.26, color: other_c, sub_color: other_c, sung: 0.0, align: al, sub_scale: 0.5 },
            ] };
            let _ = a.tb_r.draw(a.tb_hwnd, w, &rows, &tb_style, Some(h));
        }
    }
    win::show_taskbar(a.tb_hwnd, s.taskbar_lyrics);
}
