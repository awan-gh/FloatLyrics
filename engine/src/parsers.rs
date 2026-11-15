//! QRC / KRC / YRC parsers (+ YRC standardisation) producing engine `Line`s.
//! Ported from Lyricify Lyrics Helper (Apache-2.0), https://github.com/WXRIW/Lyricify-Lyrics-Helper

use crate::lrc::re;
use crate::{Line, Syl};

/// (text, start ms, end ms)
type RawSyl = (String, i64, i64);

/// Syllable line: `text` = concatenated syllables (trim end only), Syl pos/len in chars.
fn syl_line(syls: &[RawSyl]) -> Option<Line> {
    let first = syls.first()?;
    let mut text = String::new();
    let mut pos = 0;
    let mut out = Vec::with_capacity(syls.len());
    for (t, s, e) in syls {
        let len = t.chars().count();
        out.push(Syl { start: *s as f64 / 1000.0, end: *e as f64 / 1000.0, pos, len });
        pos += len;
        text.push_str(t);
    }
    Some(Line { time: first.1 as f64 / 1000.0, text: text.trim_end().to_string(), syls: Some(out), ..Default::default() })
}

fn is_attribute_line(l: &str) -> bool {
    let l = l.trim();
    l.starts_with('[') && l.ends_with(']') && l.contains(':')
}

/// Leading `[key:value]` block: returns the `[offset:]` value (ms) and how many lines it spans.
fn head_attributes(lines: &[&str]) -> (i64, usize) {
    let mut offset = 0;
    let n = lines.iter().take_while(|l| is_attribute_line(l)).count();
    for l in &lines[..n] {
        if let Some(v) = l.trim().strip_prefix("[offset:") {
            offset = v.trim_end_matches(']').trim().parse().unwrap_or(0);
        }
    }
    (offset, n)
}

fn shift(syls: &mut [RawSyl], offset: i64) {
    for s in syls { s.1 -= offset; s.2 -= offset; }
}

pub(crate) fn xml_unescape(s: &str) -> String {
    if !s.contains('&') { return s.to_string(); }
    re!(r"&(#x[0-9a-fA-F]+|#\d+|amp|lt|gt|quot|apos);").replace_all(s, |c: &regex_lite::Captures| {
        match &c[1] {
            "amp" => "&".into(), "lt" => "<".into(), "gt" => ">".into(), "quot" => "\"".into(), "apos" => "'".into(),
            n => {
                let v = if let Some(h) = n.strip_prefix("#x") { u32::from_str_radix(h, 16).ok() } else { n[1..].parse().ok() };
                v.and_then(char::from_u32).map(String::from).unwrap_or_default()
            }
        }
    }).into_owned()
}

/// Decrypted QQ payload: QRC XML -> its `LyricContent` attribute; anything else as-is.
pub fn qq_lyric_text(s: &str) -> String {
    if !s.contains("<?xml") { return s.to_string(); }
    // QQ leaves raw quotes inside the attribute, so take everything up to the quote that closes the tag
    match re!(r#"(?s)LyricContent="(.*?)"\s*/?>"#).captures(s) {
        Some(c) => xml_unescape(&c[1]),
        None => String::new(),
    }
}

/// QRC text looks like `[start,dur]word(start,dur)word(start,dur)`.
pub fn is_qrc(s: &str) -> bool {
    re!(r"(?m)^[ \t]*\[\d+,\d+\][^\r\n]*\(-?\d+,\d+\)").is_match(s)
}

pub fn qrc(text: &str) -> Vec<Line> {
    let all: Vec<&str> = text.trim().split('\n').collect();
    let (offset, skip) = head_attributes(&all);
    let word = re!(r"(.*?)\((\d+),(\d+)\)");
    all[skip..]
        .iter()
        .filter_map(|line| {
            let line = line.find(']').map_or(*line, |i| &line[i + 1..]);
            let mut syls: Vec<RawSyl> = word
                .captures_iter(line)
                .map(|m| {
                    let s: i64 = m[2].parse().unwrap_or(0);
                    (m[1].to_string(), s, s + m[3].parse::<i64>().unwrap_or(0))
                })
                .collect();
            shift(&mut syls, offset);
            syl_line(&syls)
        })
        .collect()
}

/// KRC: `[lineStart,dur]<off,dur,0>word<off,dur,0>word`; `[language:base64 json]` carries a Chinese translation.
pub fn krc(text: &str) -> Vec<Line> {
    let text = text.replace("\r\n", "\n").replace('\r', "");
    let all: Vec<&str> = text.split('\n').filter(|l| l.starts_with('[')).collect();
    let (offset, skip) = head_attributes(&all);
    let mut lines: Vec<Line> = all[skip..]
        .iter()
        .filter_map(|line| {
            let close = line.find(']')?;
            let line_start: i64 = line[1..close].split(',').next()?.parse().ok()?;
            let mut syls: Vec<RawSyl> = re!(r"<(-?\d+),(\d+),\d+>([^<]*)")
                .captures_iter(&line[close + 1..])
                .map(|m| {
                    let s = line_start + m[1].parse::<i64>().unwrap_or(0);
                    (m[3].to_string(), s, s + m[2].parse::<i64>().unwrap_or(0))
                })
                .collect();
            shift(&mut syls, offset);
            syl_line(&syls)
        })
        .collect();
    for (line, t) in lines.iter_mut().zip(krc_translation(&text).unwrap_or_default()) {
        if !t.is_empty() && t != "//" { line.zh_trans = Some(t); }
    }
    lines
}

fn krc_translation(krc: &str) -> Option<Vec<String>> {
    use base64::Engine;
    let rest = &krc[krc.find("[language:")? + 10..];
    let raw = base64::engine::general_purpose::STANDARD.decode(rest[..rest.find(']')?].trim()).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let c = v["content"].as_array()?.iter().find(|c| c["type"] == 1)?;
    Some(c["lyricContent"].as_array()?.iter().map(|x| x[0].as_str().unwrap_or("").to_string()).collect())
}

/// NetEase YRC: `[start,dur](start,dur,0)word(start,dur,0)word`; `{"t":ms,"c":[{"tx":..}]}` credit lines.
pub fn yrc(text: &str) -> Vec<Line> {
    let stamp = re!(r"\((\d+),(\d+),-?\d+\)");
    let mut out = Vec::new();
    for line in text.split('\n').map(|l| l.trim_end_matches('\r')) {
        if line.starts_with('{') {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                let text: String = v["c"].as_array().into_iter().flatten().filter_map(|c| c["tx"].as_str()).collect();
                out.push(Line { time: v["t"].as_f64().unwrap_or(0.0) / 1000.0, text, ..Default::default() });
            }
            continue;
        }
        if !line.starts_with('[') { continue; }
        let Some(close) = line.find(']') else { continue };
        let body = &line[close + 1..];
        let ms: Vec<_> = stamp.captures_iter(body).collect();
        let mut syls: Vec<RawSyl> = ms
            .iter()
            .enumerate()
            .map(|(k, m)| {
                let end = ms.get(k + 1).map_or(body.len(), |n| n.get(0).unwrap().start());
                let s: i64 = m[1].parse().unwrap_or(0);
                (body[m.get(0).unwrap().end()..end].to_string(), s, s + m[2].parse::<i64>().unwrap_or(0))
            })
            .collect();
        standardize_yrc(&mut syls);
        out.extend(syl_line(&syls));
    }
    out
}

/// Drop trailing/blank syllables, glue lone spaces and short punctuation onto the previous syllable.
fn standardize_yrc(list: &mut Vec<RawSyl>) {
    while list.last().is_some_and(|s| s.0 == " ") { list.pop(); }
    let mut i = 0;
    while i < list.len() {
        let t = &list[i].0;
        let punct = i > 0 && t.chars().count() <= 2 && t.starts_with([',', '.', '?', '!', '"']);
        if t.is_empty() {
            list.remove(i);
        } else if t == " " || punct {
            let t = list.remove(i).0;
            if i > 0 { list[i - 1].0.push_str(&t); }
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yrc_parse() {
        let s = "{\"t\":0,\"c\":[{\"tx\":\"作词: \"},{\"tx\":\"Someone\"}]}\n[1000,2000](1000,500,0)Hel(1500,500,0)lo (2000,300,0) (2300,200,0)world(2500,100,0)!\n[4000,1000](4000,500,0)(Yeah)(4500,500,0) ok\n";
        let l = yrc(s);
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].text, "作词: Someone");
        assert!(l[0].syls.is_none());
        assert_eq!(l[1].text, "Hello  world!");
        let sy = l[1].syls.as_ref().unwrap();
        assert_eq!(sy.len(), 3); // "Hel", "lo  " (space merged), "world!" (punct merged)
        assert_eq!((sy[1].pos, sy[1].len), (3, 4));
        assert_eq!((sy[2].pos, sy[2].len, sy[2].start), (7, 6, 2.3));
        assert_eq!(l[2].text, "(Yeah) ok");
        assert_eq!(l[2].time, 4.0);
    }

    #[test]
    fn qrc_parse_offset_and_xml() {
        let x = "<?xml version=\"1.0\"?><QrcInfos><LyricInfo><Lyric_1 LyricType=\"1\" LyricContent=\"[ti:a]\n[offset:100]\n[1000,500]Say &quot;hi&quot;(1000,200) you(1200,300)\n\"/></LyricInfo></QrcInfos>";
        let t = qq_lyric_text(x);
        assert!(is_qrc(&t));
        let l = qrc(&t);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].text, "Say \"hi\" you");
        assert_eq!(l[0].time, 0.9);
    }

    #[test]
    fn krc_parse() {
        let l = krc("[ar:x]\r\n[1000,800]<0,300,0>Hi <300,500,0>there\r\n");
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].text, "Hi there");
        assert_eq!(l[0].syls.as_ref().unwrap()[1].start, 1.3);
    }
}
