//! Google Fonts browser: catalogue, tiny preview subsets, full downloads. No API key needed.
use crate::settings::Settings;
use std::io::Read;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Family { pub name: String, pub category: String, pub popularity: i64 }

/// A non-browser UA makes the CSS API serve plain TTF (browsers get WOFF/WOFF2, which DirectWrite can't load).
const UA: &str = "Wget/1.21";

fn cache_dir() -> PathBuf { Settings::fonts_dir().join(".preview") }

/// Family list, most popular first. Cached for a week.
pub fn catalog() -> Result<Vec<Family>, String> {
    let file = cache_dir().join("catalog.json");
    let fresh = std::fs::metadata(&file).and_then(|m| m.modified()).ok()
        .and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() < 7 * 86400);
    let text = if fresh { std::fs::read_to_string(&file).map_err(|e| e.to_string())? } else {
        let t = ureq::get("https://fonts.google.com/metadata/fonts").call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
        let _ = std::fs::create_dir_all(cache_dir());
        let _ = std::fs::write(&file, &t);
        t
    };
    parse_catalog(&text)
}

pub fn parse_catalog(text: &str) -> Result<Vec<Family>, String> {
    let json = text.trim_start_matches(")]}'").trim();
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut out: Vec<Family> = v["familyMetadataList"].as_array().ok_or("unexpected catalogue format")?.iter().filter_map(|f| Some(Family {
        name: f["family"].as_str()?.to_string(),
        category: f["category"].as_str().unwrap_or("").to_string(),
        popularity: f["popularity"].as_i64().unwrap_or(i64::MAX),
    })).collect();
    out.sort_by_key(|f| f.popularity); // 1 = most popular
    Ok(out)
}

fn css_urls(css: &str) -> Vec<String> {
    css.split("url(").skip(1).filter_map(|p| p.split(')').next()).map(|u| u.trim_matches(|c| c == '\'' || c == '"').to_string()).collect()
}

fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    let mut b = vec![];
    ureq::get(url).set("User-Agent", UA).call().map_err(|e| e.to_string())?.into_reader().read_to_end(&mut b).map_err(|e| e.to_string())?;
    if !is_sfnt(&b) { return Err("Google Fonts sent a format Windows can't load".into()); }
    Ok(b)
}

/// TrueType / OpenType / collection magic.
pub fn is_sfnt(b: &[u8]) -> bool { matches!(b.get(..4), Some([0, 1, 0, 0]) | Some(b"OTTO") | Some(b"true") | Some(b"ttcf")) }

fn css(query: &str) -> Result<String, String> {
    ureq::get(&format!("https://fonts.googleapis.com/css2?{query}")).set("User-Agent", UA).call()
        .map_err(|_| "font not found on Google Fonts".to_string())?.into_string().map_err(|e| e.to_string())
}

fn fam_param(name: &str) -> String { name.trim().replace(' ', "+") }

/// A font file containing only the glyphs of `text` (a few KB) — enough to preview it. Cached.
pub fn preview_subset(name: &str, text: &str) -> Result<PathBuf, String> {
    let key: String = format!("{name}-{:x}", text.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64)));
    let file = cache_dir().join(format!("{}.ttf", key.replace(' ', "")));
    if file.exists() { return Ok(file); }
    let enc: String = text.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    let url = css_urls(&css(&format!("family={}&text={enc}", fam_param(name)))?).into_iter().next().ok_or("no font file")?;
    let _ = std::fs::create_dir_all(cache_dir());
    std::fs::write(&file, get_bytes(&url)?).map_err(|e| e.to_string())?;
    Ok(file)
}

/// Download regular + bold into the fonts folder so the overlay can use it.
pub fn install(name: &str) -> Result<usize, String> {
    let urls = css_urls(&css(&format!("family={}:wght@400;700", fam_param(name)))
        .or_else(|_| css(&format!("family={}", fam_param(name))))?); // some families have a single weight
    let dir = Settings::fonts_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (i, u) in urls.iter().enumerate() {
        let ext = if u.ends_with(".otf") { "otf" } else { "ttf" };
        std::fs::write(dir.join(format!("{}-{i}.{ext}", name.replace(' ', ""))), get_bytes(u)?).map_err(|e| e.to_string())?;
    }
    if urls.is_empty() { Err("font not found on Google Fonts".into()) } else { Ok(urls.len()) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_catalog_and_css() {
        let t = r#")]}'
{"familyMetadataList":[{"family":"B","category":"Serif","popularity":5},{"family":"A","category":"Sans Serif","popularity":1}]}"#;
        let c = super::parse_catalog(t).unwrap();
        assert_eq!(c[0].name, "A");
        assert_eq!(c[1].category, "Serif");
        assert!(super::is_sfnt(&[0, 1, 0, 0, 9]) && super::is_sfnt(b"OTTO..") && !super::is_sfnt(b"wOFF.."));
        assert_eq!(super::css_urls("src: url(https://x/a.ttf) format('truetype'); src: url('https://x/b.ttf')"), vec!["https://x/a.ttf", "https://x/b.ttf"]);
    }
}

#[cfg(test)]
mod live {
    #[test]
    #[ignore] // network: cargo test -p floatlyrics -- --ignored
    fn catalog_and_preview() {
        let c = super::catalog().unwrap();
        assert!(c.len() > 1000, "only {} families", c.len());
        let p = super::preview_subset(&c[0].name, &c[0].name).unwrap();
        let n = std::fs::metadata(&p).unwrap().len();
        println!("{} families; top = {}; preview subset {} bytes", c.len(), c[0].name, n);
        assert!(n > 0 && n < 200_000);
    }
}
