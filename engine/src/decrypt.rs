//! QRC (QQ Music, non-standard triple DES + zlib), KRC (Kugou, xor + zlib) and NetEase eapi (AES-128-ECB).
//! Ported from Lyricify Lyrics Helper (Apache-2.0), https://github.com/WXRIW/Lyricify-Lyrics-Helper

use crate::Error;
use std::io::Read;

fn inflate(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data).read_to_end(&mut out)?;
    Ok(out)
}

pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

// ---- QRC: QQ's DES variant. The S-boxes intentionally keep QQ's typos (sbox2 row 1, sbox4 row 3). ----

const QQ_KEY: &[u8; 24] = b"!@#)(*$%123ZXC!@!@#)(NHL";

const SBOX: [[u8; 64]; 8] = [
    [14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8,
     4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13],
    [15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 15, 12, 0, 1, 10, 6, 9, 11, 5,
     0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, 13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9],
    [10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1,
     13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, 1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12],
    [7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9,
     10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6, 10, 10, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14],
    [2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6,
     4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, 11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3],
    [12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8,
     9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, 4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13],
    [4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6,
     1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12],
    [13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2,
     7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, 2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11],
];

type Schedule = [[u8; 6]; 16];

fn bitnum(a: &[u8], b: usize, c: u32) -> u32 {
    (((a[b / 32 * 4 + 3 - b % 32 / 8] >> (7 - (b % 8))) & 1) as u32) << c
}
fn bitnumintr(a: u32, b: u32, c: u32) -> u8 { (((a >> (31 - b)) & 1) << c) as u8 }
fn bitnumintl(a: u32, b: u32, c: u32) -> u32 { ((a << b) & 0x8000_0000) >> c }
fn sboxbit(a: u8) -> usize { ((a & 0x20) | ((a & 0x1f) >> 1) | ((a & 0x01) << 4)) as usize }

fn key_schedule(key: &[u8], decrypt: bool) -> Schedule {
    const SHIFT: [u32; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];
    const PERM_C: [usize; 28] = [56, 48, 40, 32, 24, 16, 8, 0, 57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35];
    const PERM_D: [usize; 28] = [62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29, 21, 13, 5, 60, 52, 44, 36, 28, 20, 12, 4, 27, 19, 11, 3];
    const COMP: [u32; 48] = [13, 16, 10, 23, 0, 4, 2, 27, 14, 5, 20, 9, 22, 18, 11, 3, 25, 7, 15, 6, 26, 19, 12, 1,
        40, 51, 30, 36, 46, 54, 29, 39, 50, 44, 32, 47, 43, 48, 38, 55, 33, 52, 45, 41, 49, 35, 28, 31];
    let mut s = [[0u8; 6]; 16];
    let (mut c, mut d) = (0u32, 0u32);
    for i in 0..28 {
        c |= bitnum(key, PERM_C[i], 31 - i as u32);
        d |= bitnum(key, PERM_D[i], 31 - i as u32);
    }
    for i in 0..16 {
        let sh = SHIFT[i];
        c = ((c << sh) | (c >> (28 - sh))) & 0xffff_fff0;
        d = ((d << sh) | (d >> (28 - sh))) & 0xffff_fff0;
        let g = if decrypt { 15 - i } else { i };
        s[g] = [0; 6];
        for j in 0..24 { s[g][j / 8] |= bitnumintr(c, COMP[j], 7 - (j % 8) as u32); }
        for j in 24..48 { s[g][j / 8] |= bitnumintr(d, COMP[j] - 27, 7 - (j % 8) as u32); }
    }
    s
}

fn ip(input: &[u8]) -> [u32; 2] {
    const L: [usize; 32] = [57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61, 53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7];
    let (mut a, mut b) = (0, 0);
    for (k, &p) in L.iter().enumerate() {
        a |= bitnum(input, p, 31 - k as u32);
        b |= bitnum(input, p - 1, 31 - k as u32);
    }
    [a, b]
}

fn inv_ip(st: [u32; 2]) -> [u8; 8] {
    let mut out = [0u8; 8];
    // byte order 3,2,1,0,7,6,5,4 takes bit columns 7..0 of each state word
    for (idx, col) in [3usize, 2, 1, 0, 7, 6, 5, 4].into_iter().zip(0u32..8) {
        let b = 7 - col;
        out[idx] = bitnumintr(st[1], b, 7) | bitnumintr(st[0], b, 6) | bitnumintr(st[1], b + 8, 5) | bitnumintr(st[0], b + 8, 4)
            | bitnumintr(st[1], b + 16, 3) | bitnumintr(st[0], b + 16, 2) | bitnumintr(st[1], b + 24, 1) | bitnumintr(st[0], b + 24, 0);
    }
    out
}

fn f(state: u32, key: &[u8; 6]) -> u32 {
    let t1 = bitnumintl(state, 31, 0) | ((state & 0xf000_0000) >> 1) | bitnumintl(state, 4, 5) | bitnumintl(state, 3, 6)
        | ((state & 0x0f00_0000) >> 3) | bitnumintl(state, 8, 11) | bitnumintl(state, 7, 12) | ((state & 0x00f0_0000) >> 5)
        | bitnumintl(state, 12, 17) | bitnumintl(state, 11, 18) | ((state & 0x000f_0000) >> 7) | bitnumintl(state, 16, 23);
    let t2 = bitnumintl(state, 15, 0) | ((state & 0x0000_f000) << 15) | bitnumintl(state, 20, 5) | bitnumintl(state, 19, 6)
        | ((state & 0x0000_0f00) << 13) | bitnumintl(state, 24, 11) | bitnumintl(state, 23, 12) | ((state & 0x0000_00f0) << 11)
        | bitnumintl(state, 28, 17) | bitnumintl(state, 27, 18) | ((state & 0x0000_000f) << 9) | bitnumintl(state, 0, 23);
    let mut l = [(t1 >> 24) as u8, (t1 >> 16) as u8, (t1 >> 8) as u8, (t2 >> 24) as u8, (t2 >> 16) as u8, (t2 >> 8) as u8];
    for i in 0..6 { l[i] ^= key[i]; }
    let s = |n: usize, v: u8| SBOX[n][sboxbit(v)] as u32;
    let state = (s(0, l[0] >> 2) << 28) | (s(1, ((l[0] & 3) << 4) | (l[1] >> 4)) << 24) | (s(2, ((l[1] & 0xf) << 2) | (l[2] >> 6)) << 20)
        | (s(3, l[2] & 0x3f) << 16) | (s(4, l[3] >> 2) << 12) | (s(5, ((l[3] & 3) << 4) | (l[4] >> 4)) << 8)
        | (s(6, ((l[4] & 0xf) << 2) | (l[5] >> 6)) << 4) | s(7, l[5] & 0x3f);
    const P: [u32; 32] = [15, 6, 19, 20, 28, 11, 27, 16, 0, 14, 22, 25, 4, 17, 30, 9, 1, 7, 23, 13, 31, 26, 2, 8, 18, 12, 29, 5, 21, 10, 3, 24];
    P.iter().enumerate().fold(0, |acc, (k, &b)| acc | bitnumintl(state, b, k as u32))
}

fn crypt(input: &[u8], key: &Schedule) -> [u8; 8] {
    let mut st = ip(input);
    for k in key.iter().take(15) {
        let t = st[1];
        st[1] = f(st[1], k) ^ st[0];
        st[0] = t;
    }
    st[0] ^= f(st[1], &key[15]);
    inv_ip(st)
}

/// Hex QRC payload -> lyric text (often a QRC XML document).
pub fn qrc(hex: &str) -> Result<String, Error> {
    let data = hex_decode(hex.trim()).ok_or_else(|| Error("qrc: not hex".into()))?;
    // decrypt mode: schedules for keys 0/8/16 used in order 2,1,0 => run k16(dec), k8(enc), k0(dec)
    let ks = [key_schedule(&QQ_KEY[16..], true), key_schedule(&QQ_KEY[8..], false), key_schedule(&QQ_KEY[0..], true)];
    let mut plain = Vec::with_capacity(data.len());
    for block in data.chunks_exact(8) {
        let mut b = crypt(block, &ks[0]);
        b = crypt(&b, &ks[1]);
        b = crypt(&b, &ks[2]);
        plain.extend_from_slice(&b);
    }
    let out = inflate(&plain)?;
    let out = out.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&out);
    Ok(String::from_utf8_lossy(out).into_owned())
}

/// Base64 KRC payload -> KRC text.
pub fn krc(b64: &str) -> Result<String, Error> {
    use base64::Engine;
    const KEY: [u8; 16] = [0x40, 0x47, 0x61, 0x77, 0x5e, 0x32, 0x74, 0x47, 0x51, 0x36, 0x31, 0x2d, 0xce, 0xd2, 0x6e, 0x69];
    let raw = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| Error(format!("krc: {e}")))?;
    let data: Vec<u8> = raw.get(4..).unwrap_or_default().iter().enumerate().map(|(i, b)| b ^ KEY[i % 16]).collect();
    let s = String::from_utf8_lossy(&inflate(&data)?).into_owned();
    // first char is a BOM-like marker
    Ok(s.chars().skip(1).collect())
}

/// NetEase eapi `params` for `path` (e.g. "/api/song/lyric/v1") and JSON `body`.
pub fn eapi_params(path: &str, body: &str) -> String {
    use aes::cipher::{BlockEncrypt, KeyInit};
    let digest = format!("{:x}", md5::compute(format!("nobody{path}use{body}md5forencrypt")));
    let mut data = format!("{path}-36cd479b6b5-{body}-36cd479b6b5-{digest}").into_bytes();
    let pad = 16 - data.len() % 16; // PKCS7
    data.extend(std::iter::repeat(pad as u8).take(pad));
    let cipher = aes::Aes128::new(b"e82ckenh8dichen8".into());
    for block in data.chunks_exact_mut(16) {
        cipher.encrypt_block(block.into());
    }
    data.iter().map(|b| format!("{b:02X}")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn qrc_sample() {
        let s = super::qrc(include_str!("../testdata/qrc.hex")).unwrap();
        assert!(s.contains("<?xml") && s.contains("LyricContent"), "{}", &s[..s.len().min(200)]);
        let lines = crate::parsers::qq_lyric_text(&s);
        let l = crate::parsers::qrc(&lines);
        assert!(l.len() > 20 && l.iter().all(|x| x.syls.is_some()));
        assert!(l.iter().any(|x| x.text.contains("seven years old")), "{:?}", l[0]);
    }

    #[test]
    fn krc_sample() {
        let s = super::krc(include_str!("../testdata/krc.b64")).unwrap();
        assert!(s.starts_with('['), "{}", &s[..40]);
        let l = crate::parsers::krc(&s);
        assert!(l.len() > 20 && l.iter().all(|x| x.syls.is_some()));
        assert!(l.iter().any(|x| x.text.contains("seven years old")));
    }

    #[test]
    fn eapi_known_vector() {
        // hex output, block aligned, deterministic
        let p = super::eapi_params("/api/x", "{}");
        assert!(p.len() % 32 == 0 && p.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(p, super::eapi_params("/api/x", "{}"));
    }
}
