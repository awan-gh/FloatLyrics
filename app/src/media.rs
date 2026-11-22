//! "Now playing" from Windows (SMTC) — any player that shows in the volume/media flyout.
use engine::Track;
use std::time::Instant;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager as Manager, GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};

#[derive(Clone, Default)]
pub struct NowPlaying {
    pub track: Option<Track>,
    pub playing: bool,
    anchor_pos: f64,
    anchor: Option<Instant>,
    duration: f64,
}

impl NowPlaying {
    /// Players only report position occasionally; extrapolate from the last report.
    pub fn position(&self) -> f64 {
        let p = match (self.playing, self.anchor) {
            (true, Some(a)) => self.anchor_pos + a.elapsed().as_secs_f64(),
            _ => self.anchor_pos,
        };
        if self.duration > 0.0 { p.min(self.duration) } else { p }
    }
}

pub struct Media {
    mgr: Option<Manager>,
    last_raw: f64,
    last_upd: f64,
    last_key: String,
    pub state: NowPlaying,
}

/// Windows timestamps are 100 ns ticks since 1601.
fn ticks_to_unix_secs(ticks: i64) -> f64 { ticks as f64 / 1e7 - 11_644_473_600.0 }

impl Media {
    pub fn new() -> Self { Self { mgr: None, last_raw: f64::NAN, last_upd: f64::NAN, last_key: String::new(), state: NowPlaying::default() } }

    pub fn poll(&mut self) -> windows::core::Result<()> {
        if self.mgr.is_none() { self.mgr = Some(Manager::RequestAsync()?.join()?); }
        let mgr = self.mgr.as_ref().unwrap();
        let sessions = mgr.GetSessions()?;
        let mut session = None;
        for s in &sessions {
            if s.GetPlaybackInfo().and_then(|p| p.PlaybackStatus()).ok() == Some(Status::Playing) { session = Some(s); break; }
        }
        let session = match session.or_else(|| mgr.GetCurrentSession().ok()) {
            Some(s) => s,
            None => { self.state.track = None; self.state.playing = false; return Ok(()); }
        };
        let props = session.TryGetMediaPropertiesAsync()?.join()?;
        let tl = session.GetTimelineProperties()?;
        let playing = session.GetPlaybackInfo()?.PlaybackStatus()? == Status::Playing;
        let dur = (tl.EndTime()?.Duration - tl.StartTime()?.Duration) as f64 / 1e7;
        let title = props.Title()?.to_string_lossy();
        self.state.duration = dur;
        self.state.track = if title.trim().is_empty() { None } else {
            Some(Track {
                title: title.trim().to_string(),
                artist: props.Artist()?.to_string_lossy().trim().to_string(),
                album: props.AlbumTitle()?.to_string_lossy().trim().to_string(),
                duration: dur,
            })
        };
        let raw = (tl.Position()?.Duration - tl.StartTime()?.Duration) as f64 / 1e7;
        let upd = ticks_to_unix_secs(tl.LastUpdatedTime()?.UniversalTime);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        let age = now - upd;
        let sane_age = (0.0..86_400.0).contains(&age);
        let key = self.state.track.as_ref().map(|t| format!("{}\u{1}{}", t.title, t.artist)).unwrap_or_default();
        // where the player says we are now (a report is "position P at time T")
        let reported_now = if playing && sane_age { raw + age } else { raw };
        // Re-sync when the reported position changes, the song changes (players often report 0 for every new
        // song, so the number alone doesn't change), or a fresh report disagrees with our estimate (a seek).
        let resync = should_resync(raw, self.last_raw, &key, &self.last_key, upd, self.last_upd, reported_now, self.state.position());
        if resync {
            self.state.anchor_pos = raw;
            self.state.anchor = Some(if sane_age {
                Instant::now().checked_sub(std::time::Duration::from_secs_f64(age)).unwrap_or_else(Instant::now)
            } else { Instant::now() });
            self.last_raw = raw;
            self.last_key = key;
            self.last_upd = upd;
        } else if playing != self.state.playing {
            self.state.anchor_pos = self.state.position();
            self.state.anchor = Some(Instant::now());
        }
        self.last_upd = upd;
        self.state.playing = playing;
        Ok(())
    }
}

/// Diagnostic (`FloatLyrics.exe --probe-media`): print what the player reports every 0.5 s for 12 s,
/// skipping +20 s at 3 s and back at 8 s, to see whether the player reports seeks to Windows.
pub fn probe(seek: bool) -> windows::core::Result<()> {
    let mgr = Manager::RequestAsync()?.join()?;
    let pick = || -> windows::core::Result<Option<windows::Media::Control::GlobalSystemMediaTransportControlsSession>> {
        for s in &mgr.GetSessions()? {
            if s.GetPlaybackInfo().and_then(|p| p.PlaybackStatus()).ok() == Some(Status::Playing) { return Ok(Some(s)); }
        }
        Ok(mgr.GetCurrentSession().ok())
    };
    let mut m = Media::new();
    for i in 0..24 {
        let Some(s) = pick()? else { println!("no media session"); return Ok(()); };
        let tl = s.GetTimelineProperties()?;
        let raw = (tl.Position()?.Duration - tl.StartTime()?.Duration) as f64 / 1e7;
        let upd = ticks_to_unix_secs(tl.LastUpdatedTime()?.UniversalTime);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        let st = s.GetPlaybackInfo()?.PlaybackStatus()?;
        let _ = m.poll();
        if i == 0 { let p = s.TryGetMediaPropertiesAsync()?.join()?; println!("title={:?} artist={:?} album={:?} duration={:.1}s", p.Title()?.to_string_lossy(), p.Artist()?.to_string_lossy(), p.AlbumTitle()?.to_string_lossy(), (tl.EndTime()?.Duration - tl.StartTime()?.Duration) as f64 / 1e7); }
        println!("{:>5.1}s  app={}  status={:?}  reported={:7.2}s  report_age={:6.2}s  floatlyrics={:7.2}s",
            i as f64 * 0.5, s.SourceAppUserModelId()?.to_string_lossy(), st.0, raw, now - upd, m.state.position());
        if seek && (i == 6 || i == 16) {
            let target = if i == 6 { raw + 20.0 } else { (raw - 20.0).max(0.0) };
            let ok = s.TryChangePlaybackPositionAsync((target * 1e7) as i64)?.join().unwrap_or(false);
            println!("        -> asked player to seek to {target:.1}s (accepted: {ok})");
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Ok(())
}

/// Re-sync when the reported position changes, the song changes (players often report 0 for every new song,
/// so the number alone doesn't change), or a fresh report disagrees with our running estimate (a seek).
#[allow(clippy::too_many_arguments)]
fn should_resync(raw: f64, last_raw: f64, key: &str, last_key: &str, upd: f64, last_upd: f64, reported_now: f64, estimate: f64) -> bool {
    raw != last_raw || key != last_key || (upd != last_upd && (reported_now - estimate).abs() > 1.5)
}

#[cfg(test)]
mod tests {
    use super::should_resync;
    #[test]
    fn resync_rules() {
        // new song, both report 0: must re-sync (the bug that froze lyrics on song change)
        assert!(should_resync(0.0, 0.0, "B", "A", 100.0, 50.0, 3.0, 180.0));
        // same song, steady reports that agree with our estimate: keep counting
        assert!(!should_resync(0.0, 0.0, "A", "A", 100.0, 100.0, 60.0, 60.2));
        assert!(!should_resync(0.0, 0.0, "A", "A", 101.0, 100.0, 60.0, 60.4));
        // fresh report far from our estimate (a seek inside the player)
        assert!(should_resync(0.0, 0.0, "A", "A", 130.0, 100.0, 90.0, 60.0));
        // reported number changed
        assert!(should_resync(20.0, 0.0, "A", "A", 100.0, 100.0, 20.0, 60.0));
    }
}
