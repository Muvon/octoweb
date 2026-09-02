//! Trusted input for MCP actions — real AppKit events delivered to the tab's
//! WKWebView instead of synthetic DOM events.
//!
//! `dispatchEvent`-based clicks carry `isTrusted=false`. Plenty of real sites
//! gate the action that matters on trusted input (Google Drive's "Request
//! access", anything behind `navigator.userActivation`: popups, clipboard,
//! fullscreen, autoplay). Feeding `NSEvent`s straight into
//! `-[WKWebView mouseDown:]` / `keyDown:` runs WebKit's own input pipeline, so
//! the page sees `isTrusted=true`, a user gesture, CSS `:hover`, and native
//! default actions (Enter submits, Space activates, Tab moves focus).
//!
//! The methods are invoked directly on the view, bypassing window hit-testing
//! and first-responder routing — that is what makes this work on hidden
//! background-tab webviews (the MCP's default) without stealing focus from
//! the tab the user is looking at.
//!
//! Coordinates: the DOM harness reports the target's centre in CSS px of the
//! top document; view points = CSS px × page zoom; `convertPoint:toView:nil`
//! turns view points into the window coordinates NSEvent expects.

use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use objc2_core_foundation::CGPoint;
use objc2_foundation::NSString;

// NSEventType
const NS_LEFT_MOUSE_DOWN: usize = 1;
const NS_LEFT_MOUSE_UP: usize = 2;
const NS_MOUSE_MOVED: usize = 5;
const NS_KEY_DOWN: usize = 10;
const NS_KEY_UP: usize = 11;

// NSEventModifierFlags
const FLAG_SHIFT: usize = 1 << 17;
const FLAG_CONTROL: usize = 1 << 18;
const FLAG_OPTION: usize = 1 << 19;
const FLAG_COMMAND: usize = 1 << 20;

/// Virtual key code + the `characters` string AppKit attaches to the event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    pub code: u16,
    pub chars: String,
    /// Characters with Shift released (AppKit's `charactersIgnoringModifiers`).
    pub base_chars: String,
    /// Shift is implied by the key itself (`"A"`, `"!"`).
    pub shift: bool,
}

/// Map a `KeyboardEvent.key` value to macOS virtual key code + characters.
/// Returns `None` for names we don't know — the caller rejects the call so a
/// typo like `' "'` errors instead of "pressing" garbage.
pub fn key_info(key: &str) -> Option<KeyInfo> {
    let special = |code: u16, ch: char| {
        Some(KeyInfo {
            code,
            chars: ch.to_string(),
            base_chars: ch.to_string(),
            shift: false,
        })
    };
    match key {
        "Enter" | "Return" => return special(36, '\r'),
        "Tab" => return special(48, '\t'),
        "Escape" | "Esc" => return special(53, '\u{1b}'),
        "Backspace" => return special(51, '\u{7f}'),
        "Delete" => return special(117, '\u{F728}'),
        "Space" | "Spacebar" => return special(49, ' '),
        "ArrowUp" | "Up" => return special(126, '\u{F700}'),
        "ArrowDown" | "Down" => return special(125, '\u{F701}'),
        "ArrowLeft" | "Left" => return special(123, '\u{F702}'),
        "ArrowRight" | "Right" => return special(124, '\u{F703}'),
        "Home" => return special(115, '\u{F729}'),
        "End" => return special(119, '\u{F72B}'),
        "PageUp" => return special(116, '\u{F72C}'),
        "PageDown" => return special(121, '\u{F72D}'),
        _ => {}
    }
    if let Some(n) = key.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        const F_CODES: [u16; 12] = [122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111];
        if (1..=12).contains(&n) {
            let ch = char::from_u32(0xF704 + n - 1)?;
            return special(F_CODES[n as usize - 1], ch);
        }
    }
    let mut it = key.chars();
    let ch = it.next()?;
    if it.next().is_some() {
        return None;
    }
    let (base, shift) = us_layout(ch);
    Some(KeyInfo {
        code: us_key_code(base),
        chars: ch.to_string(),
        base_chars: base.to_string(),
        shift,
    })
}

/// Unshifted character on a US layout, and whether Shift produces `ch`.
fn us_layout(ch: char) -> (char, bool) {
    if ch.is_ascii_uppercase() {
        return (ch.to_ascii_lowercase(), true);
    }
    let shifted = "!@#$%^&*()_+{}|:\"<>?~";
    let base = "1234567890-=[]\\;',./`";
    match shifted.find(ch) {
        Some(i) => (base.chars().nth(i).unwrap(), true),
        None => (ch, false),
    }
}

/// ANSI US virtual key codes (Carbon `kVK_*`). Unknown characters map to 0;
/// WebKit still inserts them via `characters`, only `KeyboardEvent.code` is off.
fn us_key_code(ch: char) -> u16 {
    match ch {
        'a' => 0,
        's' => 1,
        'd' => 2,
        'f' => 3,
        'h' => 4,
        'g' => 5,
        'z' => 6,
        'x' => 7,
        'c' => 8,
        'v' => 9,
        'b' => 11,
        'q' => 12,
        'w' => 13,
        'e' => 14,
        'r' => 15,
        'y' => 16,
        't' => 17,
        '1' => 18,
        '2' => 19,
        '3' => 20,
        '4' => 21,
        '6' => 22,
        '5' => 23,
        '=' => 24,
        '9' => 25,
        '7' => 26,
        '-' => 27,
        '8' => 28,
        '0' => 29,
        ']' => 30,
        'o' => 31,
        'u' => 32,
        '[' => 33,
        'i' => 34,
        'p' => 35,
        'l' => 37,
        'j' => 38,
        '\'' => 39,
        'k' => 40,
        ';' => 41,
        '\\' => 42,
        ',' => 43,
        '/' => 44,
        'n' => 45,
        'm' => 46,
        '.' => 47,
        '`' => 50,
        ' ' => 49,
        _ => 0,
    }
}

/// NSEventModifierFlags for the MCP `modifiers` list (unknown names ignored).
pub fn modifier_flags(modifiers: &[String]) -> usize {
    modifiers.iter().fold(0, |acc, m| {
        acc | match m.as_str() {
            "shift" => FLAG_SHIFT,
            "ctrl" | "control" => FLAG_CONTROL,
            "alt" | "option" => FLAG_OPTION,
            "meta" | "cmd" | "command" => FLAG_COMMAND,
            _ => 0,
        }
    })
}

/// Seconds since boot — the timestamp base AppKit uses for events.
fn uptime() -> f64 {
    unsafe {
        let pi: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        msg_send![&*pi, systemUptime]
    }
}

/// (window point, window number) for a CSS-px point in the webview.
unsafe fn window_point(wk: *mut AnyObject, x_css: f64, y_css: f64, zoom: f64) -> (CGPoint, isize) {
    let local = CGPoint {
        x: x_css * zoom,
        y: y_css * zoom,
    };
    let nil_view: *const AnyObject = std::ptr::null();
    let pt: CGPoint = msg_send![&*wk, convertPoint: local, toView: nil_view];
    let win: *mut AnyObject = msg_send![&*wk, window];
    let num: isize = if win.is_null() {
        0
    } else {
        msg_send![&*win, windowNumber]
    };
    (pt, num)
}

unsafe fn mouse_event(ty: usize, pt: CGPoint, win: isize, clicks: isize) -> *mut AnyObject {
    let nil_ctx: *const AnyObject = std::ptr::null();
    msg_send![
        class!(NSEvent),
        mouseEventWithType: ty,
        location: pt,
        modifierFlags: 0usize,
        timestamp: uptime(),
        windowNumber: win,
        context: nil_ctx,
        eventNumber: 0isize,
        clickCount: clicks,
        pressure: if ty == NS_LEFT_MOUSE_DOWN { 1.0f32 } else { 0.0f32 }
    ]
}

/// Move the pointer to the point (hover state, `mouseover`, CSS `:hover`).
/// `wk` must be a live WKWebView pointer; main thread only.
///
/// Two moves, not one: WebKit derives `mouseover`/`mouseenter` and `:hover`
/// from the *transition* between the previous point and this one. A single
/// move from an unknown prior position often lands with no enter event, so we
/// first move just outside the target, then onto it — guaranteeing the cross.
pub fn hover(wk: *mut AnyObject, x_css: f64, y_css: f64, zoom: f64) {
    unsafe {
        let (pt, win) = window_point(wk, x_css, y_css, zoom);
        let approach = CGPoint {
            x: pt.x - 8.0,
            y: pt.y - 8.0,
        };
        let mv0 = mouse_event(NS_MOUSE_MOVED, approach, win, 0);
        let _: () = msg_send![&*wk, mouseMoved: mv0];
        let mv = mouse_event(NS_MOUSE_MOVED, pt, win, 0);
        let _: () = msg_send![&*wk, mouseMoved: mv];
    }
}

/// Trusted left click at the point: move, press, release.
pub fn click(wk: *mut AnyObject, x_css: f64, y_css: f64, zoom: f64) {
    unsafe {
        let (pt, win) = window_point(wk, x_css, y_css, zoom);
        let mv = mouse_event(NS_MOUSE_MOVED, pt, win, 0);
        let _: () = msg_send![&*wk, mouseMoved: mv];
        let down = mouse_event(NS_LEFT_MOUSE_DOWN, pt, win, 1);
        let _: () = msg_send![&*wk, mouseDown: down];
        let up = mouse_event(NS_LEFT_MOUSE_UP, pt, win, 1);
        let _: () = msg_send![&*wk, mouseUp: up];
    }
}

unsafe fn key_event(ty: usize, key: &KeyInfo, flags: usize, win: isize) -> *mut AnyObject {
    let nil_ctx: *const AnyObject = std::ptr::null();
    let chars = NSString::from_str(&key.chars);
    let base = NSString::from_str(&key.base_chars);
    msg_send![
        class!(NSEvent),
        keyEventWithType: ty,
        location: CGPoint { x: 0.0, y: 0.0 },
        modifierFlags: flags,
        timestamp: uptime(),
        windowNumber: win,
        context: nil_ctx,
        characters: &*chars,
        charactersIgnoringModifiers: &*base,
        isARepeat: Bool::NO,
        keyCode: key.code
    ]
}

/// Trusted key press (down + up) delivered to the webview's focused element.
pub fn press_key(wk: *mut AnyObject, key: &KeyInfo, modifiers: &[String]) {
    let mut flags = modifier_flags(modifiers);
    if key.shift {
        flags |= FLAG_SHIFT;
    }
    unsafe {
        let win: *mut AnyObject = msg_send![&*wk, window];
        let num: isize = if win.is_null() {
            0
        } else {
            msg_send![&*win, windowNumber]
        };
        let down = key_event(NS_KEY_DOWN, key, flags, num);
        let _: () = msg_send![&*wk, keyDown: down];
        let up = key_event(NS_KEY_UP, key, flags, num);
        let _: () = msg_send![&*wk, keyUp: up];
    }
}

/// Type `text` as trusted key presses on the focused element — the one input
/// path every editor honours, used when synthetic paste and editing commands
/// both bounced (`browser_type` mode "auto") or on request (mode "keys").
/// `\n` is Enter (a new paragraph in rich editors, submit in a single-line
/// input), `\r` is dropped, `\t` is a real Tab.
/// ponytail: synchronous; chunk across event-loop ticks if long articles stall the UI.
pub fn type_text(wk: *mut AnyObject, text: &str) {
    for ch in text.chars() {
        let key = match ch {
            '\r' => continue,
            '\n' => key_info("Enter"),
            '\t' => key_info("Tab"),
            c => key_info(c.encode_utf8(&mut [0; 4])),
        };
        if let Some(key) = key {
            press_key(wk, &key, &[]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_keys_map_to_carbon_codes() {
        assert_eq!(key_info("Enter").unwrap().code, 36);
        assert_eq!(key_info("Enter").unwrap().chars, "\r");
        assert_eq!(key_info("Escape").unwrap().code, 53);
        assert_eq!(key_info("Tab").unwrap().chars, "\t");
        assert_eq!(key_info("ArrowDown").unwrap().chars, "\u{F701}");
        assert_eq!(key_info("F5").unwrap().code, 96);
        assert_eq!(key_info("F5").unwrap().chars, "\u{F708}");
        assert_eq!(key_info("Space").unwrap().code, 49);
    }

    #[test]
    fn printable_characters_carry_shift_state() {
        let a = key_info("a").unwrap();
        assert_eq!((a.code, a.shift, a.base_chars.as_str()), (0, false, "a"));
        let upper = key_info("A").unwrap();
        assert_eq!(
            (upper.code, upper.shift, upper.base_chars.as_str()),
            (0, true, "a")
        );
        let bang = key_info("!").unwrap();
        assert_eq!(
            (bang.code, bang.shift, bang.base_chars.as_str()),
            (18, true, "1")
        );
        assert_eq!(key_info("é").unwrap().code, 0);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(key_info("").is_none());
        assert!(key_info(" \"").is_none());
        assert!(key_info("Entr").is_none());
        assert!(key_info("F13").is_none());
    }

    #[test]
    fn modifier_flags_combine() {
        let m = |s: &[&str]| modifier_flags(&s.iter().map(|x| x.to_string()).collect::<Vec<_>>());
        assert_eq!(m(&["shift"]), FLAG_SHIFT);
        assert_eq!(m(&["ctrl", "meta"]), FLAG_CONTROL | FLAG_COMMAND);
        assert_eq!(m(&["alt", "bogus"]), FLAG_OPTION);
    }
}
