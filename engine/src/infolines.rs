//! Credit/info line detection ("作词:", "Written by:", "Title - Artist" headers, copyright notices).
//! Ported from Lyricify Lyrics Helper (Apache-2.0), https://github.com/WXRIW/Lyricify-Lyrics-Helper
//! shortcut: no traditional->simplified Chinese conversion (ToSC); add it if traditional credits slip through.

/// Track title/artist of the matched result, used for the "Title - Artist" header check.
pub struct Info<'a> { pub title: &'a str, pub artist: &'a str }

const DICT: &[&str] = &[
    "音", "声", "词", "曲", "鼓", "笛", "编曲", "吉他", "吉它", "贝斯", "缩混", "胡琴", "扬琴", "提琴", "录音", "弦乐", "键盘",
    "钢琴", "童声", "助理", "古筝", "琵琶", "笛子", "二胡", "唢呐", "出品", "和声", "和音", "编写", "混音", "封面", "营销", "发行",
    "制作", "监制", "策划", "企划", "推广", "母带", "文案", "编辑", "统筹", "总监", "鸣谢", "感谢", "设计", "视觉", "小号", "长号",
    "管乐", "管弦", "工程", "簧管", "巴松", "原唱", "配唱", "伴唱", "轨道", "演唱", "尺八", "乐队", "调校", "伴奏", "主唱", "曲绘",
    "呼麦", "校对", "商务", "合作", "合唱", "指挥", "经纪", "戏腔", "团队", "协作", "顾问", "翻译", "摄影", "协力", "艺人", "行销",
    "媒介", "运营", "宣传", "管理", "单位", "支持", "平台", "录混", "宣发", "念白", "厂牌", "分轨", "贴混", "古琴", "调教", "题字",
    "海报", "图绘", "影像", "导演", "造型", "妆发", "执行", "拍摄", "服装", "制片", "灯光", "道具", "剪辑", "调色", "合成", "承制",
    "公司", "单簧管", "萨克斯", "打击乐", "合成器", "马头琴", "热瓦普", "葫芦丝", "冬不拉", "工作室", "科布兹", "专辑介绍", "巴拉莱卡",
    "OA", "OP", "OT", "PV", "SP", "A&R", "PGM", "ISRC", "Bass", "Drum", "Pads", "Brass", "Cello", "Choir", "Horns", "Mixed",
    "Mixer", "Piano", "Synth", "Viola", "Vocal", "Winds", "Kobza", "Assist", "Violin", "Mixing", "String", "Guitar", "Master",
    "Chorus", "Record", "violins", "Arrange", "Conduct", "Editing", "Hormony", "Ocarina", "Produce", "Strings", "Whistle",
    "Stylist", "Engineer", "Keyboard", "Director", "Harmonica", "Mastering", "Recording", "Balalaika", "Percussion",
    "Programing", "Production", "Programming", "Additional programming", "Irish whistle",
];

fn ci_contains(s: &str, p: &str) -> bool { s.to_lowercase().contains(&p.to_lowercase()) }

fn blank(s: &str) -> bool { s.trim().is_empty() }

pub fn is_info_line(text: &str, info: Option<&Info>) -> bool {
    if blank(text) { return false; }
    let s = text.replace('：', ": ");
    let s = s.trim();
    // "Artist:" speaker label in a duet is lyrics, not a credit
    if let Some(i) = info {
        let t = text.trim();
        if t.ends_with(':') && !i.artist.trim().is_empty() && (i.artist.trim().to_lowercase() + ":") == t.to_lowercase() { return false; }
    }
    if (s.contains("腾讯") || s.contains("TME")) && s.contains("享有") && s.contains("翻译") && s.contains('权') { return true; }
    if ["st:", "or:", "Lyrics:", " by:", " By:"].iter().any(|p| ci_contains(s, p)) { return true; }
    if ci_contains(s, "er:") && !ci_contains(s, "Tedder:") && !ci_contains(s, "Bieber:") { return true; }
    let hit = DICT.iter().any(|k| ci_contains(s, k));
    if hit && s.contains(':') { return true; }
    ["未经", "许可", "授权", "不得", "请勿", "使用", "版权"].iter().filter(|k| s.contains(*k)).count() >= 4
}

fn title_artist_line(line: &str, i: &Info) -> bool {
    if blank(line) { return false; }
    let norm = |s: &str| s.to_lowercase().replace('’', "'");
    let (l, title, artist) = (norm(line), norm(i.title), norm(i.artist).replace(", ", "/"));
    let title_hit = !title.trim().is_empty() && {
        let before = |s: &str, sep: &str| s.find(sep).filter(|&k| k > 0).map(|k| s[..k].trim().to_string());
        l.contains(&title)
            || before(&l, "(").is_some_and(|b| b.contains(&title))
            || before(&title, "(").is_some_and(|b| !b.is_empty() && l.contains(&b))
            || before(&title, " - ").is_some_and(|b| !b.is_empty() && l.contains(&b))
    };
    let artist_hit = !artist.trim().is_empty() && l.contains(&artist);
    title_hit && artist_hit
}

/// One flag per line: true = credit/info line to drop.
pub fn check(texts: &[&str], info: Option<&Info>) -> Vec<bool> {
    let n = texts.len();
    let mut flags = vec![false; n];
    let is = |k: usize| is_info_line(texts[k], info);
    let near = |range: &mut dyn Iterator<Item = usize>| {
        range.filter(|&k| !blank(texts[k])).take(2).any(|k| is(k))
    };
    // heading block
    let mut i = 0;
    if let Some(inf) = info {
        if n > 0 && title_artist_line(texts[0], inf) { i = 1; } else if n >= 3 && is(1) { i = 2; }
    }
    while i < n {
        if blank(texts[i]) {
            if near(&mut (i + 1..n)) { i += 1; continue; }
            break;
        }
        if is(i) { i += 1; continue; }
        if i + 1 < n && is(i + 1) { i += 2; continue; }
        break;
    }
    let start = i.min(n);
    // ending block
    let mut end = 0;
    for k in (0..n).rev() {
        if (blank(texts[k]) && near(&mut (0..k).rev())) || is(k) { end += 1; } else { break; }
    }
    for (k, f) in flags.iter_mut().enumerate() {
        *f = k < start || k >= n.saturating_sub(end) || is(k);
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits() {
        let info = Info { title: "7 Years", artist: "Lukas Graham" };
        let t = ["7 Years - Lukas Graham", "Lyrics by: A", "作曲：B", "Once I was", "seven years old", "Producer: X", "Mid line", "more", "Written by: C"];
        let f = check(&t, Some(&info));
        assert_eq!(f, [true, true, true, false, false, true, false, false, true]);
        assert!(!is_info_line("Lukas Graham:", Some(&info)));
        assert!(!is_info_line("Hello there", None));
    }
}
