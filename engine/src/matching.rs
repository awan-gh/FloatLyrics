//! Search-result scoring (title, artists, album, duration) and the multi-level search strategy.
//! Ported from Lyricify Lyrics Helper (Apache-2.0), https://github.com/WXRIW/Lyricify-Lyrics-Helper
//! shortcut: no traditional->simplified Chinese folding (ToSC); add it if traditional titles miss matches.

use crate::Error;

/// What we are looking for (C# TrackMultiArtistMetadata as filled by FloatLyrics).
pub struct Query<'a> { pub title: &'a str, pub artists: Vec<String>, pub album: &'a str, pub duration_ms: Option<i64> }

/// One search hit; `id`/`extra` are source specific (QQ id, NetEase id, Kugou hash ...).
#[derive(Clone, Debug)]
pub struct Hit { pub title: String, pub artists: Vec<String>, pub album: String, pub duration_ms: i64, pub id: String }

/// Lyricify MatchType: 100 Perfect, 99 VeryHigh, 95 High, 90 PrettyHigh, 70 Medium, 30 Low, 10 VeryLow, -1 NoMatch.
pub const MEDIUM: i32 = 70;

// Level scores: Perfect 7, VeryHigh 6, High 5, Medium 4, Low 2, NoMatch 0.

fn lcs_percent(x: &[char], y: &[char]) -> f64 {
    if x.is_empty() || y.is_empty() { return 0.0; }
    let mut dp = vec![vec![0usize; y.len() + 1]; x.len() + 1];
    for i in 0..x.len() {
        for j in 0..y.len() {
            dp[i + 1][j + 1] = if x[i] == y[j] { dp[i][j] + 1 } else { dp[i][j + 1].max(dp[i + 1][j]) };
        }
    }
    (dp[x.len()][y.len()] as f64 / x.len().max(y.len()) as f64 * 10000.0).round() / 100.0
}

fn norm_name(s: &str) -> String {
    let mut s = s.to_lowercase().trim().replace('’', "'").replace('，', ",").replace('（', "(").replace('）', ")").replace('[', "(").replace(']', ")");
    while s.contains("  ") { s = s.replace("  ", " "); }
    s.replace(" (", "(").replace("( ", "(").replace(" )", ")")
}

/// Title/album similarity score (0..7), None if either side is blank.
pub fn compare_name(a: &str, b: &str) -> Option<u32> {
    if a.trim().is_empty() || b.trim().is_empty() { return None; }
    let (a, b) = (norm_name(a), norm_name(b));
    if a == b { return Some(7); }
    let (a, b) = (a.replace("acoustic version", "acoustic"), b.replace("acoustic version", "acoustic"));
    let squash = |s: &str| (s.replace(" - ", " (").trim().to_string() + ")").replace(' ', "");
    if squash(&a) == squash(&b) { return Some(6); }
    let before = |s: &str, sp: &str| s.find(sp).map(|i| s[..i].trim().to_string());
    let special = |sp: &str| {
        let sp = format!("({sp}");
        let (ca, cb) = (a.contains(&sp), b.contains(&sp));
        (ca && !cb && before(&a, &sp).as_deref() == Some(b.as_str())) || (cb && !ca && before(&b, &sp).as_deref() == Some(a.as_str()))
    };
    let single = |sp: &str| {
        let sp = format!("({sp}");
        a.contains(&sp) && b.contains(&sp) && before(&a, &sp) == before(&b, &sp)
    };
    let duo = |s1: &str, s2: &str| {
        let (s1, s2) = (format!("({s1}"), format!("({s2}"));
        (a.contains(&s1) && b.contains(&s2) && before(&a, &s1) == before(&b, &s2))
            || (a.contains(&s2) && b.contains(&s1) && before(&a, &s2) == before(&b, &s1))
    };
    if ["deluxe", "explicit", "special edition", "bonus track", "feat", "with"].iter().any(|s| special(s)) { return Some(6); }
    if duo("feat", "explicit") || duo("with", "explicit") || single("feat") || single("with") { return Some(5); }
    let brackets = (a.contains('(') && !b.contains('(') && before(&a, "(").as_deref() == Some(b.as_str()))
        || (b.contains('(') && !a.contains('(') && before(&b, "(").as_deref() == Some(a.as_str()));
    if brackets { return Some(4); }
    let (ca, cb): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if ca.len() == cb.len() {
        let same = ca.iter().zip(&cb).filter(|(x, y)| x == y).count() as f64 / ca.len() as f64;
        if (same >= 0.8 && ca.len() >= 4) || (same >= 0.5 && (2..=3).contains(&ca.len())) { return Some(5); }
    }
    let p = lcs_percent(&ca, &cb);
    Some(if p > 90.0 { 6 } else if p > 80.0 { 5 } else if p > 68.0 { 4 } else if p > 55.0 { 2 } else { 0 })
}

pub fn compare_artist(a: &[String], b: &[String]) -> Option<u32> {
    let clean = |v: &[String]| -> Vec<String> { v.iter().filter(|s| !s.trim().is_empty()).map(|s| s.to_lowercase()).collect() };
    let (l1, l2) = (clean(a), clean(b));
    if l1.is_empty() || l2.is_empty() { return None; }
    let count = l2.iter().filter(|x| l1.contains(x)).count();
    let (n1, n2) = (l1.len(), l2.len());
    let ratio = count as f64 / n1 as f64;
    Some(if count == n1 && n1 == n2 { 7 }
    else if (count + 1 >= n1 && n1 >= 2) || (n1 > 6 && ratio > 0.8) { 6 }
    else if count == 1 && n1 == 1 && n2 == 2 { 5 }
    // C# compares the lowercased name with "Various", so only "群星" can ever hit
    else if n1 > 5 && (l2[0].contains("Various") || l2[0].contains("群星")) { 6 }
    else if n1 > 7 && n2 > 7 && ratio > 0.66 { 5 }
    else if n1 == 1 && n2 > 1 && l1[0].starts_with(&l2[0]) { 5 }
    else if n1 == 1 && n2 > 1 && l2[0].chars().count() > 3 && l1[0].contains(&l2[0]) { 5 }
    else if n1 == 1 && n2 > 1 && l2[0].chars().count() > 1 && l1[0].contains(&l2[0]) { 4 }
    else if count == 1 && n1 == 1 && n2 >= 3 { 4 }
    else if count >= 2 { 2 }
    else { 0 })
}

pub fn compare_duration(a: Option<i64>, b: Option<i64>) -> Option<u32> {
    let (a, b) = (a.filter(|&x| x != 0)?, b.filter(|&x| x != 0)?);
    Some(match (a - b).abs() { 0 => 7, d if d < 300 => 6, d if d < 700 => 5, d if d < 1500 => 4, d if d < 3500 => 2, _ => 0 })
}

/// Match level for a hit (album weighted 0.4, missing fields re-weighted out).
pub fn compare_track(q: &Query, h: &Hit) -> i32 {
    let t = compare_name(q.title, &h.title);
    let ar = compare_artist(&q.artists, &h.artists);
    let al = compare_name(q.album, &h.album);
    let du = compare_duration(q.duration_ms, Some(h.duration_ms));
    let mut total = t.unwrap_or(0) as f64 + ar.unwrap_or(0) as f64 + al.unwrap_or(0) as f64 * 0.4 + du.unwrap_or(0) as f64;
    let full = (1.0 + 1.0 + 0.4 + 0.2 + 1.0) * 7.0;
    let mut avail = 14.0;
    if al.is_some() { avail += 0.4 * 7.0; }
    if du.is_some() { avail += 7.0; }
    total *= full / avail; // album artists are never known here, so their 0.2 weight is always re-weighted out
    match total {
        x if x > 21.0 => 100,
        x if x > 19.0 => 99,
        x if x > 17.0 => 95,
        x if x > 15.0 => 90,
        x if x > 11.0 => 70,
        x if x > 8.0 => 30,
        x if x > 3.0 => 10,
        _ => -1,
    }
}

/// Lyricify Searcher.SearchForResult(track, Medium): query "title artists album", falling back to
/// "title artists" and "title"; a full search over all levels runs when the best hit is below Medium.
pub fn best_hit(q: &Query, mut search: impl FnMut(&str) -> Result<Vec<Hit>, Error>) -> Result<Option<Hit>, Error> {
    let mut err = None;
    let mut run = |full: bool, err: &mut Option<Error>| {
        let artists = q.artists.join(", ").replace(", ", " ");
        let mut s = format!("{} {} {}", q.title, artists, q.album).replace(" - ", " ").trim().to_string();
        let mut title = q.title.to_string();
        for cut in ["(feat.", " - feat."] {
            if let Some(i) = title.find(cut) { title = title[..i].trim().to_string(); }
        }
        let mut hits = Vec::new();
        for level in 1..3 {
            let got = search(&s).unwrap_or_else(|e| { *err = Some(e); vec![] });
            let empty = got.is_empty();
            hits.extend(got);
            if !full && !empty { break; }
            let next = if level == 1 { format!("{title} {artists}") } else { title.clone() }.replace(" - ", " ").trim().to_string();
            if next == s { break; }
            s = next;
        }
        let mut scored: Vec<(i32, Hit)> = hits.into_iter().map(|h| (compare_track(q, &h), h)).collect();
        scored.sort_by_key(|(m, _)| -m); // stable: ties keep search order
        scored
    };
    let mut r = run(false, &mut err);
    if r.first().map_or(true, |(m, _)| *m < MEDIUM) { r = run(true, &mut err); }
    match r.into_iter().next() {
        Some((m, h)) if m >= MEDIUM => Ok(Some(h)),
        Some(_) => Ok(None),
        None => err.map_or(Ok(None), Err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(t: &str, a: &str, ms: i64) -> Hit {
        Hit { title: t.into(), artists: vec![a.into()], album: String::new(), duration_ms: ms, id: String::new() }
    }

    #[test]
    fn scoring() {
        assert_eq!(compare_name("7 Years", "7 years"), Some(7));
        assert_eq!(compare_name("Hello (feat. X)", "Hello"), Some(6));
        assert_eq!(compare_name("Hello", "Completely different"), Some(0));
        let q = Query { title: "7 Years", artists: vec!["Lukas Graham".into()], album: "", duration_ms: Some(237_000) };
        assert_eq!(compare_track(&q, &hit("7 Years", "Lukas Graham", 237_460)), 100);
        assert!(compare_track(&q, &hit("7 Years", "Someone Else", 190_000)) < MEDIUM);
        let mut calls = vec![];
        let r = best_hit(&q, |s| { calls.push(s.to_string()); Ok(vec![hit("Other", "X", 1000), hit("7 Years", "Lukas Graham", 237_000)]) }).unwrap();
        assert_eq!(r.unwrap().title, "7 Years");
        assert_eq!(calls, ["7 Years Lukas Graham"]);
    }
}
