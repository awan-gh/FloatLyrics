# FloatLyrics

Floating, always-on-top, **word-synced lyrics** for whatever is playing on Windows — Spotify, Zuno, browsers/YouTube,
foobar2000, any player that shows up in the Windows media flyout. Sing along without a second screen.

Inspired by **MiniLyrics**, the classic floating-lyrics plugin for Winamp.

## Install
Download `FloatLyrics-win-Setup.exe` from [Releases](../../releases) and run it. It installs per-user
(no admin rights, no runtime or other dependencies) and keeps itself up to date.

## Features
- **Any player**: reads "now playing" from Windows (SMTC); position is extrapolated between player reports.
- **Lyrics from many sources**, searched in parallel and ranked word-timed > line-timed > plain:
  QQ Music, NetEase Cloud Music, Kugou (word-by-word timing), LRCLIB, lyrics.ovh, Soda Music (off by default).
  Reorder or disable sources in Settings. YouTube-style titles ("Official Video", "Cover by …", "Artist - Title") are cleaned first.
- **Karaoke wipe** word by word — solid, gradient or rainbow fill.
- **Subtitle-style looks**: outline / drop shadow / raised / depressed edges, background as one block or a box behind
  each line, italic/bold, any opacity.
- **Fonts**: any Windows font, font files dropped into the fonts folder (no install), or browse **Google Fonts** with
  live previews and apply in one click.
- **Translation and romanisation** (Google Translate), including romaji as the main line for singing along.
- **Animations**: fade or slide between lines.
- **Click-through** overlay; hold Ctrl+Alt to grab it; double-click to centre; position presets; multi-monitor, per-monitor DPI.
- **Taskbar lyrics**: current + next line on the taskbar.
- **Timing fixes** for covers: tap-sync (next line starts now), ±0.5 s nudges, saved per song.
- **Your lyrics folder**: cached lyrics are plain `.json` in a folder you choose; drop `Artist - Title.lrc` to override.
- **Updates**: off / notify / download and install on exit / fully automatic.
- Small: native Rust + Direct2D, ~11 MB exe, low RAM.

## Shortcuts (editable in Settings)
| Key | Action |
|---|---|
| Ctrl+Alt+L | Toggle click-through |
| hold Ctrl+Alt | Grab the overlay while click-through is on |
| Ctrl+Alt+Space | Tap-sync: the next line starts now |
| Ctrl+Alt+Right / Left | Lyrics 0.5 s sooner / later |
| Ctrl+Alt+0 | Reset this song's offset |
| Ctrl+Alt+C | Centre horizontally |
| Ctrl+Alt+H | Hide / show |
| Ctrl+Alt+T | Taskbar lyrics |

## Build
Requires Rust (MSVC toolchain) and, for the installer, the .NET SDK (for Velopack's `vpk` tool).
```
cargo test --workspace
cargo build --release -p floatlyrics          # target/release/FloatLyrics.exe
./build.ps1 -Version 1.0.1 [-Publish]         # installer in releases/, optionally upload to GitHub Releases
```

## Credits
- **MiniLyrics** (Crintsoft) — the original floating lyrics experience this app is modelled on.
- **[Lyricify Lyrics Helper](https://github.com/WXRIW/Lyricify-Lyrics-Helper)** by WXRIW (Apache-2.0) — the lyrics
  engine (QQ Music / NetEase / Kugou search, decryption and parsers) is a Rust port of it. See `NOTICE`.
- **[LRCLIB](https://lrclib.net)** — open synced-lyrics database. **lyrics.ovh** — plain lyrics.
- Lyrics providers QQ Music, NetEase Cloud Music, Kugou and Soda Music; Google Translate; Google Fonts.
- Built with [Slint](https://slint.dev), [Velopack](https://velopack.io) and [windows-rs](https://github.com/microsoft/windows-rs).

## Licence
MIT (see `LICENSE`). Ported engine code keeps its Apache-2.0 licence (see `NOTICE`).
Lyrics are fetched from third-party services for personal use; their content belongs to their respective owners.
