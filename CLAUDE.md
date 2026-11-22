# FloatLyrics (Rust) — working rules

## Releases
- **Release only after at least 10 changes since the last release** (count user-visible changes: features,
  fixes, tweaks — not version bumps or docs-only commits). Check with `git log <last-tag>..HEAD --oneline`.
- Below 10, commit and push code only. Never run `./build.ps1 -Publish` and never build an installer unless the
  user explicitly asks for one; if they ask for a release with fewer than 10 changes, say how many there are and
  confirm first.
- Releasing: `./build.ps1 -Version x.y.z -Publish` (bumps app/Cargo.toml, builds, packs with Velopack, uploads to
  GitHub Releases, which installed copies auto-update from). Commit the version bump afterwards.
- The repo is public: never commit personal paths, emails or tokens.

## Layout
- `engine/` — lyrics sources, decryption, parsers, matching, translation (parts ported from Lyricify Lyrics Helper, Apache-2.0).
- `app/` — Win32 + Direct2D overlay, Slint settings/tray/popups (`app/ui/settings.slint`), Velopack updates.
- Tests: `cargo test --workspace`; network checks: `cargo test -p floatlyrics -- --ignored`.
