//! cargo run -p engine --example probe -- "Artist" "Title" [seconds] [translate-lang]
//! Prints each source's result (rank, line count, first lines) and the orchestrated `find` pick.

use engine::{find, search, SourceId, Track};
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let t = Track {
        artist: a.first().cloned().unwrap_or_else(|| "Lukas Graham".into()),
        title: a.get(1).cloned().unwrap_or_else(|| "7 Years".into()),
        album: String::new(),
        duration: a.get(2).and_then(|d| d.parse().ok()).unwrap_or(0.0),
    };
    for &src in SourceId::all() {
        let t0 = Instant::now();
        let r = search(src, &t.artist, &t.title, &t);
        print!("== {:<10} {:>5}ms  ", src.name(), t0.elapsed().as_millis());
        match r {
            Err(e) => println!("ERROR {e}"),
            Ok(None) => println!("no match"),
            Ok(Some(l)) => show(&l),
        }
    }
    let t0 = Instant::now();
    let r = find(&t, SourceId::all(), false);
    print!("== find()     {:>5}ms  ", t0.elapsed().as_millis());
    let Some(mut l) = r else { return println!("nothing") };
    show(&l);
    if let Some(lang) = a.get(3) {
        match engine::translate::apply(&mut l, lang, true) {
            Err(e) => println!("translate ERROR {e}"),
            Ok(()) => {
                for x in l.lines.iter().take(3) {
                    println!("     {} | trans: {:?} | roma: {:?}", x.text, x.trans, x.roma);
                }
            }
        }
    }
}

fn show(l: &engine::Lyric) {
    let rank = if l.lines.iter().any(|x| x.syls.is_some()) { 3 } else if l.synced { 2 } else { 1 };
    let zh = l.lines.iter().filter(|x| x.zh_trans.is_some()).count();
    let roma = l.lines.iter().filter(|x| x.roma.is_some()).count();
    println!("rank {rank}, {} lines, zh_trans {zh}, roma {roma} | {}", l.lines.len(), l.source);
    for x in l.lines.iter().take(3) {
        let syls = x.syls.as_ref().map_or(String::new(), |s| format!(" ({} syls)", s.len()));
        println!("     [{:7.2}] {}{syls}", x.time, x.text);
    }
}
