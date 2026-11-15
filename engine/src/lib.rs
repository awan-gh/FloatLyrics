//! FloatLyrics lyrics engine: sources, decryption, parsers, matching, translation.
//! Public interface agreed with the app — see ENGINE_SPEC.md.

/// One timed word/syllable. `pos`/`len` are in Unicode scalar values (chars) within `Line::text`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Syl { pub start: f64, pub end: f64, pub pos: usize, pub len: usize }

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Line {
    /// seconds
    pub time: f64,
    pub text: String,
    #[serde(default)] pub syls: Option<Vec<Syl>>,
    #[serde(default)] pub trans: Option<String>,
    #[serde(default)] pub zh_trans: Option<String>,
    #[serde(default)] pub roma: Option<String>,
    /// 0 unspecified, 1 left, 2 right (duets)
    #[serde(default)] pub align: u8,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Lyric { pub lines: Vec<Line>, pub synced: bool, pub source: String }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track { pub title: String, pub artist: String, pub album: String, pub duration: f64 }

mod decrypt;
mod infolines;
mod lrc;
mod matching;
mod parsers;
mod sources;
pub mod translate;

pub use lrc::{candidates, clean_artist, clean_title, parse_lrc};

use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SourceId { QQMusic, Netease, Kugou, Lrclib, SodaMusic, LyricsOvh }

impl SourceId {
    pub fn all() -> &'static [SourceId] {
        use SourceId::*;
        &[QQMusic, Netease, Kugou, Lrclib, SodaMusic, LyricsOvh]
    }
    pub fn name(&self) -> &'static str {
        match self {
            SourceId::QQMusic => "QQ Music",
            SourceId::Netease => "NetEase",
            SourceId::Kugou => "Kugou",
            SourceId::Lrclib => "LRCLIB",
            SourceId::SodaMusic => "Soda Music",
            SourceId::LyricsOvh => "lyrics.ovh",
        }
    }
    /// Whether the source can return word-timed (karaoke) lyrics.
    pub fn word_timed(&self) -> bool {
        matches!(self, SourceId::QQMusic | SourceId::Netease | SourceId::Kugou | SourceId::SodaMusic)
    }
}

/// Network or parse failure (message only; callers just log/skip it).
#[derive(Debug, Clone)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}
impl std::error::Error for Error {}
impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Self { Error(format!("http: {e}")) }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self { Error(format!("io: {e}")) }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self { Error(format!("json: {e}")) }
}

pub(crate) fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build())
}

/// Search one source for a track; best match or None. Errors are network/parse failures.
pub fn search(src: SourceId, artist: &str, title: &str, t: &Track) -> Result<Option<Lyric>, Error> {
    sources::search(src, artist, title, t)
}

/// Free-text manual search on one source (used by "Search lyrics manually").
pub fn search_query(src: SourceId, query: &str, duration: f64) -> Result<Option<Lyric>, Error> {
    sources::search_query(src, query, duration)
}

fn rank(l: &Lyric) -> u8 {
    if l.lines.iter().any(|x| x.syls.is_some()) { 3 } else if l.synced { 2 } else { 1 }
}

/// Best of one candidate's results: rank, then (optionally) human romanisation, then `order` position.
fn pick(found: Vec<(usize, Lyric)>, prefer_roma: bool) -> Option<Lyric> {
    found
        .into_iter()
        .max_by_key(|(i, l)| (rank(l), prefer_roma && l.lines.iter().any(|x| x.roma.is_some()), std::cmp::Reverse(*i)))
        .map(|(_, l)| l)
}

/// Orchestrated search over `order` (enabled sources, priority order) for the first two `candidates(t)`.
pub fn find(t: &Track, order: &[SourceId], prefer_roma: bool) -> Option<Lyric> {
    let mut best: Option<Lyric> = None;
    for (artist, title) in candidates(t).into_iter().take(2) {
        let (tx, rx) = mpsc::channel();
        for (i, &src) in order.iter().enumerate() {
            let (tx, artist, title, t) = (tx.clone(), artist.clone(), title.clone(), t.clone());
            std::thread::spawn(move || {
                let _ = tx.send((i, search(src, &artist, &title, &t).ok().flatten()));
            });
        }
        drop(tx);
        // shortcut: a source still running at the deadline is abandoned (its thread ends on ureq's own timeout)
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut found = Vec::new();
        while let Ok((i, l)) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            if let Some(l) = l { found.push((i, l)); }
        }
        if let Some(p) = pick(found, prefer_roma) {
            if best.as_ref().map_or(true, |b| rank(&p) > rank(b)) { best = Some(p); }
        }
        if best.as_ref().is_some_and(|b| rank(b) >= 2) { return best; }
    }
    // last resort: LRCLIB fuzzy search on the cleaned title alone
    if order.contains(&SourceId::Lrclib) {
        if let Ok(Some(l)) = sources::lrclib_query(&clean_title(&t.title), t.duration, &format!("{} {}", t.title, t.artist)) {
            if best.as_ref().map_or(true, |b| rank(&l) > rank(b)) { best = Some(l); }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lyric(syl: bool, synced: bool, roma: bool) -> Lyric {
        Lyric {
            lines: vec![Line {
                syls: syl.then(|| vec![]),
                roma: roma.then(|| "r".into()),
                ..Default::default()
            }],
            synced,
            source: String::new(),
        }
    }

    #[test]
    fn pick_ranks_then_order_then_roma() {
        let p = pick(vec![(1, lyric(false, true, false)), (0, lyric(false, false, false)), (2, lyric(true, true, false))], false);
        assert_eq!(rank(&p.unwrap()), 3);
        let p = pick(vec![(0, lyric(false, true, false)), (1, lyric(false, true, true))], true).unwrap();
        assert!(p.lines[0].roma.is_some());
        let p = pick(vec![(0, lyric(false, true, false)), (1, lyric(false, true, true))], false).unwrap();
        assert!(p.lines[0].roma.is_none());
        assert_eq!(SourceId::all().len(), 6);
        assert_eq!(serde_json::to_string(&SourceId::QQMusic).unwrap(), "\"QQMusic\"");
    }
}
