//! LRC parsing, title/artist cleanup and search candidates (ported from FloatLyrics C# Lyrics.cs).

use crate::{Line, Lyric, Track};
use regex_lite::Regex;

macro_rules! re {
    ($s:expr) => {{
        static R: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
        R.get_or_init(|| regex_lite::Regex::new($s).unwrap())
    }};
}
pub(crate) use re;

/// `[mm:ss.xx]` lines (multi-stamps allowed), `[offset:]`, `<word stamps>` stripped.
/// Unsynced text is spread evenly over `duration` (or 4 s per line when unknown).
pub fn parse_lrc(text: &str, duration: f64) -> Lyric {
    let stamp = re!(r"\[(\d+):(\d+(?:[.:]\d+)?)\]");
    let word = re!(r"<\d+:\d+(?:[.:]\d+)?>");
    let offset_tag = re!(r"(?i)^\[offset:\s*([+-]?\d+)\s*\]");
    let mut lines = Vec::new();
    let mut offset = 0.0;
    for raw in text.split('\n') {
        let line = raw.trim();
        if let Some(m) = offset_tag.captures(line) {
            offset = m[1].parse::<i64>().unwrap_or(0) as f64 / 1000.0;
            continue;
        }
        let txt = word.replace_all(&stamp.replace_all(line, ""), "").trim().to_string();
        for m in stamp.captures_iter(line) {
            let min: f64 = m[1].parse().unwrap_or(0.0);
            let sec: f64 = m[2].replace(':', ".").parse().unwrap_or(0.0);
            lines.push(Line { time: min * 60.0 + sec, text: txt.clone(), ..Default::default() });
        }
    }
    if !lines.is_empty() {
        // LRC [offset:+n] means lyrics appear n ms sooner
        for l in &mut lines { l.time -= offset; }
        lines.sort_by(|a, b| a.time.total_cmp(&b.time));
        return Lyric { lines, synced: true, source: String::new() };
    }
    // shortcut: unsynced lyrics are spread evenly over the song; user fixes timing with tap-sync
    let plain: Vec<&str> = text.split('\n').map(str::trim).filter(|s| !s.is_empty()).collect();
    let n = plain.len() as f64;
    let span = if duration > 0.0 { duration } else { n * 4.0 };
    let lines = plain
        .iter()
        .enumerate()
        .map(|(k, s)| Line { time: span * k as f64 / n, text: s.to_string(), ..Default::default() })
        .collect();
    Lyric { lines, synced: false, source: String::new() }
}

fn feat() -> &'static Regex { re!(r"(?i)\s+(feat\.?|ft\.?|featuring)\s.*$") }

/// Strips YouTube/cover noise: brackets, "| channel", feat., "official video", "- Remastered" ...
pub fn clean_title(s: &str) -> String {
    let s = re!(r"\s*[\(\[【（][^\)\]】）]*[\)\]】）]").replace_all(s, "");
    let s = re!(r"\s*(\||｜|//).*$").replace_all(&s, "");
    let s = feat().replace_all(&s, "");
    let s = re!(r"(?i)\s+((acoustic\s+)?cover(ed)?(\s+by\s.*)?|lyrics?(\s+video)?|official.*|m/?v)$|\s+[-–—]\s+[^-–—]*(remaster|version|edit|mix|live|mono|stereo|lyrics|cover)[^-–—]*$").replace_all(&s, "");
    re!(r"\s+").replace_all(&s, " ").trim_matches(&[' ', '-', '"', '\''][..]).to_string()
}

pub fn clean_artist(s: &str) -> String {
    let s = feat().replace_all(s, "");
    let s = re!(r"(?i)\s*-\s*Topic$|VEVO$|\s+Official$").replace_all(&s, "");
    s.split([',', ';']).next().unwrap_or("").trim().to_string()
}

/// (artist, title) pairs to try, best first. "Artist - Title" uploads also yield both splits.
pub fn candidates(t: &Track) -> Vec<(String, String)> {
    let title = clean_title(&t.title);
    let mut list = vec![(clean_artist(&t.artist), title.clone())];
    let parts: Vec<&str> = re!(r"\s+[-–—]\s+").split(&title).collect();
    if parts.len() >= 2 {
        // "Artist - Title" uploads, where the SMTC artist is just the channel name
        list.push((clean_artist(parts[0]), clean_title(parts[1])));
        list.push((clean_artist(parts[1]), clean_title(parts[0])));
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for c in list {
        if !c.1.is_empty() && !out.contains(&c) { out.push(c); }
    }
    out
}

/// Match an extra LRC (translation / romanisation) onto lines by timestamp (|dt| < 0.6 s).
pub(crate) fn attach(l: &mut Lyric, lrc: Option<&str>, set: fn(&mut Line, String)) {
    let Some(lrc) = lrc.filter(|s| !s.trim().is_empty()) else { return };
    let extra = parse_lrc(lrc, 0.0);
    if !extra.synced { return; }
    for line in &mut l.lines {
        if let Some(m) = extra.lines.iter().find(|e| (e.time - line.time).abs() < 0.6 && !e.text.is_empty()) {
            if m.text != line.text { set(line, m.text.clone()); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc() {
        let l = parse_lrc("[ar:x]\n[offset:500]\n[00:01.00][00:10.50]Hello <00:01.20>world\n[00:05.00]Mid\n", 100.0);
        assert!(l.synced && l.lines.len() == 3);
        assert!((l.lines[0].time - 0.5).abs() < 1e-9 && l.lines[0].text == "Hello world");
        assert!(l.lines[1].text == "Mid" && (l.lines[2].time - 10.0).abs() < 1e-9);
        let p = parse_lrc("a\n\nb\n", 10.0);
        assert!(!p.synced && p.lines.len() == 2 && p.lines[1].time == 5.0);
    }

    #[test]
    fn titles() {
        assert_eq!(clean_title("Adele - Hello (Official Music Video) [HD]"), "Adele - Hello");
        assert_eq!(clean_title("Hello ft. Someone | Cover by Jane"), "Hello");
        assert_eq!(clean_title("Yesterday - Remastered 2009"), "Yesterday");
        assert_eq!(clean_title("Hello acoustic cover"), "Hello");
        assert_eq!(clean_artist("Adele - Topic"), "Adele");
        let t = Track { title: "Adele - Hello (cover)".into(), artist: "JaneCovers".into(), album: String::new(), duration: 300.0 };
        let c = candidates(&t);
        assert_eq!(c[0], ("JaneCovers".to_string(), "Adele - Hello".to_string()));
        assert!(c.contains(&("Adele".to_string(), "Hello".to_string())));
    }

    #[test]
    fn attach_by_time() {
        let mut l = parse_lrc("[00:01.00]Hi\n[00:05.00]Yo", 0.0);
        attach(&mut l, Some("[00:01.30]Hai\n[00:09.00]x"), |x, v| x.zh_trans = Some(v));
        assert_eq!(l.lines[0].zh_trans.as_deref(), Some("Hai"));
        assert_eq!(l.lines[1].zh_trans, None);
    }
}
