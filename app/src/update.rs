//! Updates from GitHub Releases via Velopack (per-user install, no admin).
use crate::{App, Msg};
use velopack::{sources::GithubSource, UpdateCheck, UpdateInfo, UpdateManager};

pub const REPO: &str = "https://github.com/awan-gh/FloatLyrics";

pub type Pending = Box<UpdateInfo>;

pub enum Event { Status(String), Available(Pending), Downloaded(Pending) }

fn manager() -> Result<UpdateManager, String> {
    UpdateManager::new(GithubSource::new(REPO, None, false), None, None)
        .map_err(|_| "Updates work in the installed app (not this dev build)".to_string())
}

/// `manual` = the user pressed "Check now" (always reports, ignores the Off mode).
pub fn check_in_background(a: &mut App, manual: bool) {
    let mode = a.s.update_mode.clone();
    if mode == "Off" && !manual { return; }
    let tx = a.tx.clone();
    a.update_status = "Checking…".into();
    std::thread::spawn(move || {
        let send = |e| { let _ = tx.send(Msg::Update(e)); };
        let um = match manager() { Ok(m) => m, Err(e) => { send(Event::Status(e)); return; } };
        match um.check_for_updates() {
            Ok(UpdateCheck::UpdateAvailable(info)) => {
                let v = info.TargetFullRelease.Version.clone();
                if manual || mode == "Notify" { send(Event::Available(info)); return; }
                send(Event::Status(format!("Downloading {v}…")));
                match um.download_updates(&info, None) {
                    Ok(()) => {
                        if mode == "Auto" { let _ = um.apply_updates_and_restart(&info.TargetFullRelease); }
                        // "Download": install quietly after FloatLyrics exits
                        let _ = um.wait_exit_then_apply_updates(&info.TargetFullRelease, true, false, Vec::<String>::new());
                        send(Event::Downloaded(info));
                    }
                    Err(e) => send(Event::Status(format!("Download failed: {e}"))),
                }
            }
            Ok(_) => send(Event::Status(format!("Up to date ({})", crate::VERSION))),
            Err(e) => send(Event::Status(format!("Update check failed: {e}"))),
        }
    });
}

pub fn on_event(a: &mut App, e: Event) {
    match e {
        Event::Status(s) => a.update_status = s,
        Event::Available(info) => {
            let v = info.TargetFullRelease.Version.clone();
            a.update_status = format!("Version {v} available — click the notification or Check again to install");
            crate::win::balloon(a.hwnd, "FloatLyrics update", &format!("Version {v} is available. Click to install."));
            a.pending_update = Some(info);
        }
        Event::Downloaded(info) => {
            a.update_downloaded = true;
            let v = info.TargetFullRelease.Version.clone();
            a.update_status = format!("Version {v} downloaded — installs when FloatLyrics exits");
            crate::win::balloon(a.hwnd, "FloatLyrics update", &format!("Version {v} will be installed when you exit."));
        }
    }
    if let Some(w) = &a.settings_win { w.set_update_status(a.update_status.clone().into()); }
}

/// Notification clicked: download and restart into the new version.
pub fn install_now(a: &mut App) {
    let Some(info) = a.pending_update.take() else { return };
    a.s.save();
    a.update_status = "Installing…".into();
    std::thread::spawn(move || {
        if let Ok(um) = manager() {
            if um.download_updates(&info, None).is_ok() { let _ = um.apply_updates_and_restart(&info.TargetFullRelease); }
        }
    });
}
