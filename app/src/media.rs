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
    pub state: NowPlaying,
}

/// Windows timestamps are 100 ns ticks since 1601.
fn ticks_to_unix_secs(ticks: i64) -> f64 { ticks as f64 / 1e7 - 11_644_473_600.0 }

impl Media {
    pub fn new() -> Self { Self { mgr: None, last_raw: f64::NAN, state: NowPlaying::default() } }

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
        if raw != self.last_raw {
            // anchor at the player's own report time when it is sane, else now
            let upd = ticks_to_unix_secs(tl.LastUpdatedTime()?.UniversalTime);
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
            let age = now - upd;
            self.state.anchor_pos = raw;
            self.state.anchor = Some(if (0.0..86_400.0).contains(&age) {
                Instant::now().checked_sub(std::time::Duration::from_secs_f64(age)).unwrap_or_else(Instant::now)
            } else { Instant::now() });
            self.last_raw = raw;
        } else if playing != self.state.playing {
            self.state.anchor_pos = self.state.position();
            self.state.anchor = Some(Instant::now());
        }
        self.state.playing = playing;
        Ok(())
    }
}
