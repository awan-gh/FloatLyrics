//! Global shortcuts and the hold-to-grab modifier check.
use crate::{with_app, App};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// "Ctrl+Alt+L" -> (modifiers, virtual key). vk 0 = modifiers only.
pub fn parse(s: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let (mut m, mut vk) = (HOT_KEY_MODIFIERS(0), 0u32);
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let p = part.to_ascii_lowercase();
        match p.as_str() {
            "ctrl" | "control" => m |= MOD_CONTROL,
            "alt" => m |= MOD_ALT,
            "shift" => m |= MOD_SHIFT,
            "win" => m |= MOD_WIN,
            _ => {
                if vk != 0 { return None; }
                vk = match p.as_str() {
                    "left" => 0x25, "up" => 0x26, "right" => 0x27, "down" => 0x28, "space" => 0x20, "enter" => 0x0D,
                    "tab" => 0x09, "home" => 0x24, "end" => 0x23, "pageup" => 0x21, "pagedown" => 0x22, "insert" => 0x2D,
                    "delete" => 0x2E, "esc" | "escape" => 0x1B,
                    k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => k.to_ascii_uppercase().as_bytes()[0] as u32,
                    k if k.starts_with('f') && k[1..].parse::<u32>().is_ok_and(|n| (1..=24).contains(&n)) => 0x6F + k[1..].parse::<u32>().unwrap(),
                    _ => return None,
                };
            }
        }
    }
    if m.0 == 0 && vk == 0 { None } else { Some((m, vk)) }
}

fn actions(a: &App) -> [&str; 8] {
    let s = &a.s;
    [&s.key_click_through, &s.key_hide, &s.key_sooner, &s.key_later, &s.key_tap_sync, &s.key_reset_offset, &s.key_centre, &s.key_taskbar]
}

/// (Re)register all shortcuts; returns a message listing the ones that could not be registered.
pub fn register_all(a: &App) -> String {
    let mut bad = vec![];
    for (i, k) in actions(a).iter().enumerate() {
        unsafe { let _ = UnregisterHotKey(Some(a.hwnd), i as i32 + 1); }
        if k.trim().is_empty() { continue; }
        match parse(k) {
            Some((m, vk)) if vk != 0 && unsafe { RegisterHotKey(Some(a.hwnd), i as i32 + 1, m | MOD_NOREPEAT, vk) }.is_ok() => {}
            _ => bad.push(k.to_string()),
        }
    }
    if bad.is_empty() { String::new() } else { format!("Shortcut unavailable: {}", bad.join(", ")) }
}

pub fn on_hotkey(id: i32) {
    with_app(|a| match id {
        1 => { a.s.click_through = !a.s.click_through; crate::mark_dirty(a); crate::ui::push_to_settings(a); }
        2 => a.manual_hidden = !a.manual_hidden,
        3 => crate::set_song_offset(a, a.song_offset + 0.5),
        4 => crate::set_song_offset(a, a.song_offset - 0.5),
        5 => crate::tap_sync(a),
        6 => crate::set_song_offset(a, 0.0),
        7 => crate::win::move_to(a, 0.5, f64::NAN, None),
        8 => { a.s.taskbar_lyrics = !a.s.taskbar_lyrics; crate::mark_dirty(a); crate::ui::push_to_settings(a); }
        _ => {}
    });
}

pub fn peek_held(keys: &str) -> bool {
    let Some((m, _)) = parse(keys) else { return false };
    if m.0 == 0 { return false; }
    let down = |vk: i32| unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000 != 0;
    (!m.contains(MOD_CONTROL) || down(0x11)) && (!m.contains(MOD_ALT) || down(0x12))
        && (!m.contains(MOD_SHIFT) || down(0x10)) && (!m.contains(MOD_WIN) || down(0x5B) || down(0x5C))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses() {
        assert_eq!(parse("Ctrl+Alt+L"), Some((MOD_CONTROL | MOD_ALT, 0x4C)));
        assert_eq!(parse("Ctrl+Alt+0").map(|x| x.1), Some(0x30));
        assert_eq!(parse("Ctrl+Alt+Right").map(|x| x.1), Some(0x27));
        assert_eq!(parse("Ctrl+Shift+F9").map(|x| x.1), Some(0x78));
        assert_eq!(parse("Ctrl+Alt"), Some((MOD_CONTROL | MOD_ALT, 0)));
        assert_eq!(parse("Ctrl+Bogus"), None);
    }
}
