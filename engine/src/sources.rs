//! Online sources: QQ Music, NetEase, Kugou, Soda Music (word-timed, via Lyricify's APIs), LRCLIB, lyrics.ovh.
//! QQ/NetEase/Kugou/Soda ported from Lyricify Lyrics Helper (Apache-2.0), https://github.com/WXRIW/Lyricify-Lyrics-Helper
//! LRCLIB logic ported from FloatLyrics C# (Lyrics.cs / Sources.cs).

use crate::infolines::{self, Info};
use crate::lrc::{attach, parse_lrc, re};
use crate::matching::{best_hit, Hit, Query};
use crate::{agent, decrypt, parsers, Error, Line, Lyric, SourceId, Track};
use serde_json::{json, Value};

const PC_UA: &str = "Mozilla/5.0 (Windows NT 10.0; WOW64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/63.0.3239.132 Safari/537.36";

fn read_json(r: ureq::Response) -> Result<Value, Error> {
    Ok(serde_json::from_str(&r.into_string()?)?)
}

fn s(v: &Value) -> String {
    match v {
        Value::String(x) => x.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn names(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().map(|a| s(&a["name"])).collect()
}

pub fn search(src: SourceId, artist: &str, title: &str, t: &Track) -> Result<Option<Lyric>, Error> {
    let q = Query {
        title,
        artists: if artist.is_empty() { vec![] } else { vec![artist.to_string()] },
        album: &t.album,
        duration_ms: (t.duration > 0.0).then(|| (t.duration * 1000.0) as i64),
    };
    let hay = format!("{} {}", t.title, t.artist);
    match src {
        SourceId::Lrclib if artist.is_empty() => lrclib(&[("q", title)], t.duration, &hay),
        SourceId::Lrclib => lrclib(&[("track_name", title), ("artist_name", artist)], t.duration, &hay),
        SourceId::LyricsOvh if artist.is_empty() => ovh_query(title, t.duration),
        SourceId::LyricsOvh => ovh(artist, title, t.duration),
        _ => match best_hit(&q, |kw| searcher(src, kw))? {
            Some(h) => lyrics(src, &h, t.duration),
            None => Ok(None),
        },
    }
}

pub fn search_query(src: SourceId, query: &str, duration: f64) -> Result<Option<Lyric>, Error> {
    match src {
        SourceId::Lrclib => lrclib_query(query, duration, query),
        SourceId::LyricsOvh => ovh_query(query, duration),
        // free text can't be scored against a title, so trust the service's own top hit
        _ => match searcher(src, query)?.into_iter().next() {
            Some(h) => lyrics(src, &h, duration),
            None => Ok(None),
        },
    }
}

fn searcher(src: SourceId, kw: &str) -> Result<Vec<Hit>, Error> {
    match src {
        SourceId::QQMusic => qq_search(kw),
        SourceId::Netease => netease_search(kw),
        SourceId::Kugou => kugou_search(kw),
        SourceId::SodaMusic => soda_search(kw),
        _ => Ok(vec![]),
    }
}

fn lyrics(src: SourceId, h: &Hit, duration: f64) -> Result<Option<Lyric>, Error> {
    let label = format!("{}: {} - {}", src.name(), h.artists.join(", "), h.title);
    let info = Info { title: &h.title, artist: &h.artists.join(", ") };
    match src {
        SourceId::QQMusic => qq_lyrics(h, &label, &info),
        SourceId::Netease => netease_lyrics(h, &label, &info),
        SourceId::Kugou => kugou_lyrics(h, &label, &info),
        SourceId::SodaMusic => soda_lyrics(h, &label, &info, duration),
        _ => Ok(None),
    }
}

/// Drop credit lines, sort, wrap (C# Sources.FromData).
fn finish(lines: Vec<Line>, source: &str, info: &Info) -> Option<Lyric> {
    let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    let flags = infolines::check(&texts, Some(info));
    let mut lines: Vec<Line> = lines.into_iter().zip(flags).filter(|(_, f)| !f).map(|(l, _)| l).collect();
    lines.sort_by(|a, b| a.time.total_cmp(&b.time));
    (!lines.is_empty()).then(|| Lyric { lines, synced: true, source: source.to_string() })
}

// ---------------- QQ Music ----------------

fn qq_song(v: &Value) -> Hit {
    let strip = |x: &Value| s(x).replace("<em>", "").replace("</em>", "");
    Hit {
        title: strip(&v["title"]),
        artists: v["singer"].as_array().into_iter().flatten().map(|a| strip(&a["name"])).collect(),
        album: strip(&v["album"]["title"]),
        duration_ms: v["interval"].as_i64().unwrap_or(0) * 1000,
        id: s(&v["id"]),
    }
}

fn qq_hits(list: &Value) -> Vec<Hit> {
    let mut out = Vec::new();
    for v in list.as_array().into_iter().flatten() {
        out.push(qq_song(v));
        out.extend(v["grp"].as_array().into_iter().flatten().map(qq_song));
    }
    out.retain(|h| !h.id.is_empty());
    out
}

fn qq_search(kw: &str) -> Result<Vec<Hit>, Error> {
    const KEY: &str = "music.search.SearchCgiService.DoSearchForQQMusicMobile";
    let body = json!({
        "comm": { "ct": "11", "v": "20080508", "tmeLoginType": "0", "tmeLoginMethod": "0" },
        KEY: { "module": "music.search.SearchCgiService", "method": "DoSearchForQQMusicMobile", "param": {
            "query": kw, "highlight": 1, "searchid": "", "sub_searchid": 0, "search_type": 0, "nqc_flag": 0, "sin": 0, "ein": 30,
            "page_num": 1, "num_per_page": 30, "cat": 2, "grp": 1, "remoteplace": "", "from": "", "multi_zhida": 1, "sem": 0,
            "related_data": "", "ambi_data": "" } }
    });
    let mobile = agent()
        .post("https://u6.y.qq.com/cgi-bin/musicu.fcg")
        .set("User-Agent", "QQMusic 20080508(android)")
        .set("Accept", "*/*")
        .set("Content-Type", "application/x-www-form-urlencoded") // the app sends JSON under this type
        .send_string(&body.to_string())
        .map_err(Error::from)
        .and_then(read_json)
        .map(|v| qq_hits(&v[KEY]["data"]["body"]["item_song"]));
    if let Ok(h) = &mobile {
        if !h.is_empty() { return mobile; }
    }
    // desktop endpoint fallback
    let body = json!({ "req_1": { "method": "DoSearchForQQMusicDesktop", "module": "music.search.SearchCgiService",
        "param": { "num_per_page": "20", "page_num": "1", "query": kw, "search_type": 0 } } });
    let v = read_json(agent().post("https://u6.y.qq.com/cgi-bin/musicu.fcg").set("User-Agent", PC_UA).set("Referer", "https://c.y.qq.com/").set("Content-Type", "application/json").send_string(&body.to_string())?)?;
    // code 2001 = anti-crawl throttle (bursts of requests from one IP); report it instead of "no match"
    match v["req_1"]["code"].as_i64() {
        Some(0) => Ok(qq_hits(&v["req_1"]["data"]["body"]["song"]["list"])),
        c => Err(Error(format!("QQ Music search refused (code {})", c.unwrap_or(-1)))),
    }
}

fn xml_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = xml.find(&format!("<{tag}"))?;
    let rest = &xml[open + tag.len() + 1..];
    if !rest.starts_with(['>', ' ', '\t', '\r', '\n']) { return xml_tag(rest, tag); }
    let start = rest.find('>')? + 1;
    let end = rest.find(&format!("</{tag}>"))?;
    let inner = rest.get(start..end)?.trim();
    Some(inner.strip_prefix("<![CDATA[").and_then(|x| x.strip_suffix("]]>")).unwrap_or(inner))
}

/// Decrypt one QQ lyric payload; plain LRC passes through.
fn qq_payload(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() { return None; }
    let plain = match decrypt::hex_decode(text) {
        Some(_) => decrypt::qrc(text).ok()?,
        None if re!(r"\[\d+:\d+").is_match(text) => text.to_string(),
        None => return None,
    };
    let s = parsers::qq_lyric_text(&plain);
    (!s.trim().is_empty()).then_some(s)
}

fn qq_lyrics(h: &Hit, label: &str, info: &Info) -> Result<Option<Lyric>, Error> {
    let xml = agent()
        .post("https://c.y.qq.com/qqmusic/fcgi-bin/lyric_download.fcg")
        .set("User-Agent", PC_UA)
        .set("Referer", "https://c.y.qq.com/")
        .send_form(&[("version", "15"), ("miniversion", "82"), ("lrctype", "4"), ("musicid", &h.id)])?
        .into_string()?
        .replace("<!--", "")
        .replace("-->", "");
    let Some(orig) = xml_tag(&xml, "content").and_then(qq_payload) else { return Ok(None) };
    let lines = if parsers::is_qrc(&orig) { parsers::qrc(&orig) } else { parse_lrc(&orig, 0.0).lines };
    let mut l = finish(lines, label, info);
    if let Some(l) = &mut l {
        attach(l, xml_tag(&xml, "contentts").and_then(qq_payload).as_deref(), |x, v| x.zh_trans = Some(v));
    }
    Ok(l)
}

// ---------------- NetEase ----------------

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
}

fn eapi(url: &str, mut data: serde_json::Map<String, Value>) -> Result<Value, Error> {
    let ms = now_ms();
    let header = json!({
        "__csrf": "", "appver": "8.0.0", "buildver": (ms / 1000).to_string(), "channel": "", "deviceId": "", "mobilename": "",
        "resolution": "1920x1080", "os": "android", "osver": "", "requestId": format!("{ms}_{:04}", ms % 1000),
        "versioncode": "140", "MUSIC_U": ""
    });
    let cookie = header.as_object().unwrap().iter().map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or(""))).collect::<Vec<_>>().join("; ");
    data.insert("header".into(), Value::String(header.to_string()));
    let path = url.replacen("https://interface3.music.163.com/e", "/", 1).replacen("https://interface.music.163.com/e", "/", 1);
    let params = decrypt::eapi_params(&path, &Value::Object(data).to_string());
    read_json(
        agent()
            .post(url)
            .set("User-Agent", "Mozilla/5.0 (Linux; Android 9; PCT-AL10) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/70.0.3538.64 HuaweiBrowser/10.0.3.311 Mobile Safari/537.36")
            .set("Referer", "https://music.163.com/")
            .set("Cookie", &cookie)
            .send_form(&[("params", &params)])?,
    )
}

fn map(pairs: &[(&str, &str)]) -> serde_json::Map<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

// shortcut: only the eapi "cloudsearch/pc" search (Lyricify tries the older weapi search first, which needs RSA)
fn netease_search(kw: &str) -> Result<Vec<Hit>, Error> {
    let v = eapi("https://interface.music.163.com/eapi/cloudsearch/pc", map(&[("s", kw), ("type", "1"), ("limit", "30"), ("offset", "0"), ("total", "true")]))?;
    Ok(v["result"]["songs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|x| Hit { title: s(&x["name"]), artists: names(&x["ar"]), album: s(&x["al"]["name"]), duration_ms: x["dt"].as_i64().unwrap_or(0), id: s(&x["id"]) })
        .collect())
}

fn netease_lyrics(h: &Hit, label: &str, info: &Info) -> Result<Option<Lyric>, Error> {
    let v = eapi(
        "https://interface3.music.163.com/eapi/song/lyric/v1",
        map(&[("id", &h.id), ("cp", "false"), ("lv", "0"), ("kv", "0"), ("tv", "0"), ("rv", "0"), ("yv", "0"), ("ytv", "0"), ("yrv", "0"), ("csrf_token", "")]),
    )?;
    let text = |k: &str| v[k]["lyric"].as_str().filter(|x| !x.trim().is_empty());
    let mut l = text("yrc").and_then(|y| finish(parsers::yrc(y), label, info));
    if l.is_none() {
        l = text("lrc").and_then(|x| finish(parse_lrc(x, 0.0).lines, label, info));
    }
    if let Some(l) = &mut l {
        attach(l, text("tlyric"), |x, v| x.zh_trans = Some(v));
        attach(l, text("romalrc"), |x, v| x.roma = Some(v));
    }
    Ok(l)
}

// ---------------- Kugou ----------------

fn kugou_song(v: &Value) -> Hit {
    Hit {
        title: s(&v["songname"]),
        artists: s(&v["singername"]).split('、').map(str::to_string).collect(),
        album: s(&v["album_name"]),
        duration_ms: v["duration"].as_i64().unwrap_or(0) * 1000,
        id: s(&v["hash"]),
    }
}

fn kugou_search(kw: &str) -> Result<Vec<Hit>, Error> {
    let v = read_json(
        agent()
            .get("http://mobilecdn.kugou.com/api/v3/search/song")
            .query_pairs([("format", "json"), ("keyword", kw), ("page", "1"), ("pagesize", "20"), ("showtype", "1")])
            .call()?,
    )?;
    let mut out = Vec::new();
    for x in v["data"]["info"].as_array().into_iter().flatten() {
        out.push(kugou_song(x));
        out.extend(x["group"].as_array().into_iter().flatten().map(kugou_song));
    }
    Ok(out)
}

fn kugou_lyrics(h: &Hit, label: &str, info: &Info) -> Result<Option<Lyric>, Error> {
    let v = read_json(
        agent()
            .get("https://lyrics.kugou.com/search")
            .query_pairs([("ver", "1"), ("man", "yes"), ("client", "pc"), ("keyword", ""), ("hash", &h.id)])
            .call()?,
    )?;
    let c = &v["candidates"][0];
    if c.is_null() { return Ok(None); }
    let d = read_json(
        agent()
            .get("https://lyrics.kugou.com/download")
            .query_pairs([("ver", "1"), ("client", "pc"), ("id", &s(&c["id"])), ("accesskey", &s(&c["accesskey"])), ("fmt", "krc"), ("charset", "utf8")])
            .call()?,
    )?;
    let Some(content) = d["content"].as_str().filter(|x| !x.is_empty()) else { return Ok(None) };
    Ok(finish(parsers::krc(&decrypt::krc(content)?), label, info))
}

// ---------------- Soda Music (Douyin "汽水音乐") ----------------

fn soda_search_raw(kw: &str, salt: u128) -> Result<String, Error> {
    // random-ish 16-digit device/install ids, like Lyricify's GenerateClientId
    let n = now_ms() * 7919 + salt * 104_729;
    let (id, iid) = (format!("{}", 1_000_000_000_000_000 + n % 8_999_999_999_999_999), format!("{}", 9_999_999_999_999_999 - n % 8_999_999_999_999_999));
    Ok(agent()
        .get("https://api.qishui.com/luna/search/track")
        .query_pairs([
            ("device_platform", "android"), ("os", "android"), ("ssmix", "a"), ("cdid", "46556f98-1720-4248-83da-62b74b60b46a"),
            ("channel", "xiaomi_8478_64"), ("aid", "386088"), ("app_name", "luna"), ("version_code", "100198030"),
            ("version_name", "19.8.0"), ("manifest_version_code", "100198030"), ("update_version_code", "100198030"),
            ("resolution", "1080*1920"), ("dpi", "480"), ("device_type", "ABR-AL80"), ("device_brand", "HUAWEI"), ("language", "zh"),
            ("os_api", "35"), ("os_version", "15"), ("ac", "wifi"), ("device_model", "ABR-AL80"), ("tz_name", "Asia/Shanghai"),
            ("tz_offset", "28800"), ("package", "com.luna.music"), ("sim_region", "cn"), ("iid", &iid), ("device_id", &id),
            ("_rticket", &now_ms().to_string()), ("q", kw), ("cursor", "0"), ("count", "20"),
        ])
        .set("Accept", "*/*")
        .set("User-Agent", "com.luna.music/100198030 (Linux; U; Android 15; zh_CN_#Hans; ABR-AL80; Build/V417IR;tt-ok/3.12.13.19)")
        .call()?
        .into_string()?)
}

fn soda_search(kw: &str) -> Result<Vec<Hit>, Error> {
    // Soda answers many anonymous queries with an empty 200 body; a fresh device id usually gets through
    let mut body = String::new();
    for attempt in 0..5u128 {
        body = soda_search_raw(kw, attempt)?;
        if !body.trim().is_empty() { break; }
    }
    if body.trim().is_empty() { return Ok(vec![]); }
    let v: Value = serde_json::from_str(&body)?;
    Ok(v["result_groups"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|g| g["data"].as_array().into_iter().flatten())
        .filter(|d| d["meta"]["item_type"] == "track" && d["entity"]["track"].is_object())
        .map(|d| {
            let t = &d["entity"]["track"];
            Hit { title: s(&t["name"]), artists: names(&t["artists"]), album: s(&t["album"]["name"]), duration_ms: t["duration"].as_i64().unwrap_or(0), id: s(&t["id"]) }
        })
        .collect())
}

fn soda_lyrics(h: &Hit, label: &str, info: &Info, duration: f64) -> Result<Option<Lyric>, Error> {
    let v = read_json(
        agent()
            .get("https://beta-luna.douyin.com/luna/h5/seo_track")
            .query_pairs([("track_id", h.id.as_str()), ("device_platform", "web")])
            .set("Accept", "application/json")
            .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
            .call()?,
    )?;
    let lyric = if v["lyric"].is_object() { &v["lyric"] } else { &v["seo_track"]["lyric"] };
    let Some(text) = lyric["content"].as_str().filter(|x| !x.trim().is_empty()) else { return Ok(None) };
    let mut l = if re!(r"(?m)^\[\d+,\d+\]<").is_match(text) {
        finish(parsers::krc(text), label, info)
    } else {
        let mut p = parse_lrc(text, duration);
        p.source = label.to_string();
        Some(p)
    };
    if let Some(l) = &mut l {
        attach(l, lyric["translations"]["cn"].as_str(), |x, v| x.zh_trans = Some(v));
    }
    Ok(l)
}

// ---------------- LRCLIB (FloatLyrics C#) ----------------

fn norm(s: &str) -> String { s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect() }

pub fn lrclib_query(q: &str, duration: f64, hay: &str) -> Result<Option<Lyric>, Error> {
    lrclib(&[("q", q)], duration, hay)
}

/// One LRCLIB search; keeps only results whose track name appears in `hay` (guards against junk matches).
fn lrclib(params: &[(&str, &str)], duration: f64, hay: &str) -> Result<Option<Lyric>, Error> {
    let v = read_json(agent().get("https://lrclib.net/api/search").set("User-Agent", "FloatLyrics/1.0").query_pairs(params.iter().copied()).call()?)?;
    let hay = norm(hay);
    let mut best: Option<(f64, &Value)> = None;
    for r in v.as_array().into_iter().flatten() {
        let name = norm(r["trackName"].as_str().unwrap_or(""));
        if name.is_empty() || !(hay.contains(&name) || name.contains(&hay)) { continue; }
        let synced = !r["syncedLyrics"].as_str().unwrap_or("").is_empty();
        let instr = r["instrumental"] == true;
        if !synced && r["plainLyrics"].as_str().unwrap_or("").is_empty() && !instr { continue; }
        let d = r["duration"].as_f64().unwrap_or(0.0);
        let score = if synced || instr { 0.0 } else { 1000.0 } + if duration > 0.0 && d > 0.0 { (d - duration).abs() } else { 50.0 };
        if best.map_or(true, |(b, _)| score < b) { best = Some((score, r)); }
    }
    let Some((_, b)) = best else { return Ok(None) };
    let text = [b["syncedLyrics"].as_str(), b["plainLyrics"].as_str()]
        .into_iter()
        .flatten()
        .find(|x| !x.is_empty())
        .unwrap_or("[00:00.00]♪ instrumental ♪");
    let mut l = parse_lrc(text, duration);
    l.source = format!("lrclib: {} - {}", s(&b["artistName"]), s(&b["trackName"]));
    Ok(Some(l))
}

// ---------------- lyrics.ovh (plain lyrics, free, no key) ----------------

fn path_seg(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn ovh(artist: &str, title: &str, duration: f64) -> Result<Option<Lyric>, Error> {
    let url = format!("https://api.lyrics.ovh/v1/{}/{}", path_seg(artist), path_seg(title));
    let v = match agent().get(&url).call() {
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        r => read_json(r?)?,
    };
    let Some(text) = v["lyrics"].as_str().filter(|x| !x.trim().is_empty()) else { return Ok(None) };
    // lyrics.ovh often starts with "Paroles de la chanson X par Y" — a header line, not lyrics
    let text = text.strip_prefix("Paroles de la chanson").map_or(text, |r| r.split_once('\n').map_or("", |x| x.1));
    let mut l = parse_lrc(text, duration);
    if l.lines.is_empty() { return Ok(None); }
    l.source = format!("lyrics.ovh: {artist} - {title}");
    Ok(Some(l))
}

/// Free text: lyrics.ovh's Deezer-backed suggest picks artist + title, then fetches.
fn ovh_query(q: &str, duration: f64) -> Result<Option<Lyric>, Error> {
    let v = read_json(agent().get(&format!("https://api.lyrics.ovh/suggest/{}", path_seg(q))).call()?)?;
    let first = &v["data"][0];
    if first.is_null() { return Ok(None); }
    ovh(&s(&first["artist"]["name"]), &s(&first["title"]), duration)
}
