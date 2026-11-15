//! Translation + romanisation through Google's free translate endpoint (unofficial; ported from FloatLyrics C#).

use crate::{agent, Error, Lyric};
use serde_json::Value;

fn non_latin(s: &str) -> bool { s.chars().any(|c| c as u32 > 0x2FF) }

/// Fill `Line.trans` (target `lang`, skipped when the detected language already is `lang`) and,
/// if `roma`, `Line.roma` for lines with non-Latin chars that have none. `lang` "zh*" first reuses `zh_trans`.
pub fn apply(l: &mut Lyric, lang: &str, roma: bool) -> Result<(), Error> {
    let lang = lang.trim();
    if lang.is_empty() && !roma { return Ok(()); }
    if lang.to_lowercase().starts_with("zh") {
        for x in &mut l.lines {
            if x.trans.is_none() { x.trans = x.zh_trans.clone(); }
        }
    }
    let need_trans = !lang.is_empty() && l.lines.iter().any(|x| x.trans.is_none());
    let need_roma = roma && l.lines.iter().any(|x| x.roma.is_none() && non_latin(&x.text));
    if !need_trans && !need_roma { return Ok(()); }

    let texts: Vec<String> = l.lines.iter().map(|x| if x.text.trim().is_empty() { "♪".into() } else { x.text.trim().replace('|', "/") }).collect();
    let tl = if lang.is_empty() { "en" } else { lang };
    let body = agent()
        .post("https://translate.googleapis.com/translate_a/single")
        .query_pairs([("client", "gtx"), ("sl", "auto"), ("tl", tl), ("dt", "t"), ("dt", "rm")])
        .timeout(std::time::Duration::from_secs(15))
        // romanisation drops newlines but keeps "|"
        .send_form(&[("q", &texts.join("\n|\n"))])?
        .into_string()?;
    let (trans, translit, detected) = parse_google(&serde_json::from_str(&body)?);

    let tr: Vec<&str> = trans.split('|').collect();
    if need_trans && tr.len() == texts.len() && !detected.to_lowercase().starts_with(&lang.to_lowercase()) {
        for (i, x) in l.lines.iter_mut().enumerate() {
            if x.trans.is_none() && tr[i].trim() != texts[i] { x.trans = Some(tr[i].trim().to_string()); }
        }
    }
    let rl: Vec<&str> = translit.split('|').collect();
    if need_roma && rl.len() == texts.len() {
        for (i, x) in l.lines.iter_mut().enumerate() {
            if x.roma.is_none() && non_latin(&texts[i]) && !rl[i].trim().is_empty() { x.roma = Some(rl[i].trim().to_string()); }
        }
    }
    Ok(())
}

/// (translation, transliteration, detected language) from a translate_a/single response.
pub(crate) fn parse_google(root: &Value) -> (String, String, String) {
    let mut trans = String::new();
    let mut translit = String::new();
    for seg in root[0].as_array().into_iter().flatten() {
        if let Some(t) = seg[0].as_str() {
            trans.push_str(t);
        } else if let Some(r) = seg[3].as_str() {
            translit = r.to_string();
        }
    }
    (trans, translit, root[2].as_str().unwrap_or("").to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn google_response() {
        let v = serde_json::from_str(r#"[[["Hello\n","こんにちは\n"],["World","世界"],[null,null,null,"konnichiwa\nsekai"]],null,"ja"]"#).unwrap();
        let (t, r, d) = super::parse_google(&v);
        assert_eq!((t.as_str(), r.as_str(), d.as_str()), ("Hello\nWorld", "konnichiwa\nsekai", "ja"));
    }
}
