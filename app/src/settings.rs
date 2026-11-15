use engine::SourceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn app_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("FloatLyrics")
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    // text
    pub font_family: String,
    pub font_size: f64,
    pub bold: bool,
    pub italic: bool,
    /// None, Outline, Shadow, Raised, Depressed
    pub edge_style: String,
    pub outline_width: f64,
    pub karaoke: bool,
    pub duet_view: bool,
    // colours (#AARRGGBB or #RRGGBB)
    pub text_color: String,
    pub highlight_color: String,
    pub highlight_color2: String,
    pub next_color: String,
    pub outline_color: String,
    pub bg_color: String,
    pub bg_opacity: f64,
    /// None, Block (one box), Line (box behind each text line, caption style)
    pub bg_mode: String,
    pub opacity: f64,
    /// Solid, Gradient, Rainbow — how the sung part is filled
    pub fill_style: String,
    // layout
    pub lines_before: u32,
    pub lines_after: u32,
    pub next_scale: f64,
    pub align: String,
    pub width: f64,
    pub anim_style: String,
    pub anim_ms: f64,
    pub click_through: bool,
    pub show_frame: bool,
    pub hide_when_idle: bool,
    pub hide_when_no_lyrics: bool,
    pub global_offset: f64,
    pub left: i32,
    pub top: i32,
    // translation
    pub translation_lang: String,
    pub show_romaji: bool,
    pub romaji_main: bool,
    pub sub_all_lines: bool,
    pub sub_scale: f64,
    // sources (priority order) and storage
    pub source_order: Vec<SourceId>,
    pub source_disabled: Vec<SourceId>,
    /// empty = default (%APPDATA%\FloatLyrics\lyrics)
    pub lyrics_dir: String,
    // taskbar
    pub taskbar_lyrics: bool,
    pub taskbar_width: f64,
    pub taskbar_side: String,
    // shortcuts
    pub key_click_through: String,
    pub key_hide: String,
    pub key_sooner: String,
    pub key_later: String,
    pub key_tap_sync: String,
    pub key_reset_offset: String,
    pub key_centre: String,
    pub key_taskbar: String,
    pub peek_keys: String,
    // app
    pub update_mode: String, // Off, Notify, Download, Auto
    pub autostart: bool,
    pub song_offsets: HashMap<String, f64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_family: "Segoe UI".into(), font_size: 40.0, bold: true, italic: false, edge_style: "Outline".into(), outline_width: 2.5, karaoke: true, duet_view: true,
            text_color: "#FFFFFFFF".into(), highlight_color: "#FF4FC3F7".into(), highlight_color2: "#FFB388FF".into(),
            next_color: "#C0FFFFFF".into(), outline_color: "#FF000000".into(), bg_color: "#FF000000".into(),
            bg_opacity: 0.6, bg_mode: "None".into(), opacity: 1.0, fill_style: "Solid".into(),
            lines_before: 0, lines_after: 1, next_scale: 0.65, align: "Center".into(), width: 1100.0,
            anim_style: "Slide".into(), anim_ms: 350.0, click_through: false, show_frame: true, hide_when_idle: true, hide_when_no_lyrics: false,
            global_offset: 0.0, left: -1, top: -1,
            translation_lang: String::new(), show_romaji: false, romaji_main: false, sub_all_lines: false, sub_scale: 0.5,
            source_order: SourceId::all().to_vec(), source_disabled: vec![SourceId::SodaMusic], lyrics_dir: String::new(), // Soda search is unreliable lyrics_dir: String::new(),
            taskbar_lyrics: false, taskbar_width: 420.0, taskbar_side: "Right".into(),
            key_click_through: "Ctrl+Alt+L".into(), key_hide: "Ctrl+Alt+H".into(), key_sooner: "Ctrl+Alt+Right".into(),
            key_later: "Ctrl+Alt+Left".into(), key_tap_sync: "Ctrl+Alt+Space".into(), key_reset_offset: "Ctrl+Alt+0".into(),
            key_centre: "Ctrl+Alt+C".into(), key_taskbar: "Ctrl+Alt+T".into(), peek_keys: "Ctrl+Alt".into(),
            update_mode: "Notify".into(), autostart: true, song_offsets: HashMap::new(),
        }
    }
}

impl Settings {
    fn path() -> PathBuf { app_dir().join("settings.json") }

    pub fn load() -> Self { Self::load_from(&Self::path()) }

    pub fn load_from(p: &std::path::Path) -> Self {
        let mut s: Settings = std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        // new sources added in later versions join the end of the user's priority list
        for id in SourceId::all() { if !s.source_order.contains(id) { s.source_order.push(*id); } }
        s
    }

    pub fn save(&self) { let _ = self.save_to(&Self::path()); }

    /// Write-then-rename so a crash mid-save can't leave a half-written file (which would reset everything).
    pub fn save_to(&self, p: &std::path::Path) -> std::io::Result<()> {
        if let Some(d) = p.parent() { std::fs::create_dir_all(d)?; }
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap())?;
        std::fs::rename(tmp, p)
    }

    pub fn lyrics_dir(&self) -> PathBuf {
        if self.lyrics_dir.trim().is_empty() { app_dir().join("lyrics") } else { PathBuf::from(self.lyrics_dir.trim()) }
    }

    pub fn fonts_dir() -> PathBuf { app_dir().join("fonts") }

    /// Enabled sources in priority order.
    pub fn sources(&self) -> Vec<SourceId> {
        self.source_order.iter().copied().filter(|s| !self.source_disabled.contains(s)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_every_field() {
        // change every field away from its default, save, load, compare
        let mut s = Settings::default();
        let mut v = serde_json::to_value(&s).unwrap();
        for (k, val) in v.as_object_mut().unwrap().iter_mut() {
            let nv = match &*val {
                serde_json::Value::Bool(b) => serde_json::Value::Bool(!*b),
                serde_json::Value::Number(_) => serde_json::json!(7),
                serde_json::Value::String(_) => serde_json::Value::String(format!("x{k}")),
                serde_json::Value::Array(_) => serde_json::json!(["Kugou"]),
                serde_json::Value::Object(_) => serde_json::json!({"song": 1.5}),
                other => other.clone(),
            };
            *val = nv;
        }
        s = serde_json::from_value(v.clone()).unwrap();
        let p = std::env::temp_dir().join("floatlyrics-rs-test.json");
        s.save_to(&p).unwrap();
        let back = Settings::load_from(&p);
        let _ = std::fs::remove_file(&p);
        let mut expect = s.clone();
        for id in SourceId::all() { if !expect.source_order.contains(id) { expect.source_order.push(*id); } }
        assert_eq!(back, expect);
    }
}
