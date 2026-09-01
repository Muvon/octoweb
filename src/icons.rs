//! Inline SVG icon strings — Lucide set (lucide.dev), MIT-licensed.
//!
//! Each constant is a complete `<svg>` element with `width`/`height` omitted so
//! callers control sizing via CSS (`.icon`, `width=` attribute, or wrapping
//! element font-size). Stroke is `currentColor` everywhere, so colour follows
//! the parent's `color`. Normalize against a 24×24 viewBox with stroke=2 —
//! matches Lucide's published defaults so updates are mechanical.
//!
//! Usage:
//!
//! ```ignore
//! format!(r#"<span class="icon">{}</span>"#, icons::CLOCK)
//! ```
//!
//! Or splice directly into HTML strings via `format!`. Templates that return
//! `&'static str` should switch to `String` if they want to embed icons.
//!
//! The brand mark (OCTOPUS_BRAND) is hand-drawn, not Lucide — used for the
//! new-tab hero and error-page hero. The Lucide icons are for chrome.

// ── Lucide chrome icons (24×24, stroke 2, round caps & joins) ───────────────

const PREFIX: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#;
const SUFFIX: &str = "</svg>";

/// `download` — transfer-size indicator (replaces ⬇).
pub const DOWNLOAD: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>"#,
    r#"<polyline points="7 10 12 15 17 10"/>"#,
    r#"<line x1="12" y1="15" x2="12" y2="3"/>"#,
    "</svg>"
);

/// `clock` — page-load-time indicator (replaces ⏱).
pub const CLOCK: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<circle cx="12" cy="12" r="10"/>"#,
    r#"<polyline points="12 6 12 12 16 14"/>"#,
    "</svg>"
);

/// `activity` — CPU-usage indicator (replaces ⚡). Pulse waveform.
pub const ACTIVITY: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<polyline points="22 12 18 12 15 21 9 3 6 12 2 12"/>"#,
    "</svg>"
);

/// `cpu` — memory/RSS indicator (replaces ◉).
pub const CPU: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<rect x="4" y="4" width="16" height="16" rx="2"/>"#,
    r#"<rect x="9" y="9" width="6" height="6"/>"#,
    r#"<line x1="9" y1="1" x2="9" y2="4"/><line x1="15" y1="1" x2="15" y2="4"/>"#,
    r#"<line x1="9" y1="20" x2="9" y2="23"/><line x1="15" y1="20" x2="15" y2="23"/>"#,
    r#"<line x1="20" y1="9" x2="23" y2="9"/><line x1="20" y1="15" x2="23" y2="15"/>"#,
    r#"<line x1="1" y1="9" x2="4" y2="9"/><line x1="1" y1="15" x2="4" y2="15"/>"#,
    "</svg>"
);

/// `sparkles` — AI toggle button (replaces the 🐙 emoji in the address bar).
pub const SPARKLES: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<path d="M12 3l1.9 4.6L18.5 9.5 13.9 11.4 12 16l-1.9-4.6L5.5 9.5 10.1 7.6z"/>"#,
    r#"<path d="M19 14l.8 1.9L21.7 16.7l-1.9.8L19 19.5l-.8-1.9L16.3 16.7l1.9-.8z"/>"#,
    r#"<path d="M5 16l.6 1.4L7 18l-1.4.6L5 20l-.6-1.4L3 18l1.4-.6z"/>"#,
    "</svg>"
);

/// `check-circle-2` — success toast icon.
pub const CHECK_CIRCLE: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<circle cx="12" cy="12" r="10"/>"#,
    r#"<polyline points="8 12 11 15 16 9"/>"#,
    "</svg>"
);

/// `x-circle` — error toast icon.
pub const X_CIRCLE: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<circle cx="12" cy="12" r="10"/>"#,
    r#"<line x1="15" y1="9" x2="9" y2="15"/>"#,
    r#"<line x1="9" y1="9" x2="15" y2="15"/>"#,
    "</svg>"
);

/// `check` — inline checkmark.
pub const CHECK: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<polyline points="4 12 10 18 20 6"/>"#,
    "</svg>"
);

/// `pencil` — URL edit affordance in the address bar.
pub const PENCIL: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<path d="M12 20h9"/>"#,
    r#"<path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4z"/>"#,
    "</svg>"
);

/// `plus` — empty quickslot affordance / "add" hint.
pub const PLUS: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<line x1="12" y1="5" x2="12" y2="19"/>"#,
    r#"<line x1="5" y1="12" x2="19" y2="12"/>"#,
    "</svg>"
);

/// `lock` — secure-site indicator (replaces 🔒).
pub const LOCK: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<rect x="5" y="11" width="14" height="10" rx="2"/>"#,
    r#"<path d="M8 11V8a4 4 0 0 1 8 0v3"/>"#,
    "</svg>"
);

/// `layers` — workspace switcher toolbar button.
pub const LAYERS: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<path d="M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z"/>"#,
    r#"<path d="M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12"/>"#,
    r#"<path d="M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17"/>"#,
    "</svg>"
);

/// `trash-2` — delete-workspace affordance in the workspace switcher.
pub const TRASH: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<path d="M10 11v6"/>"#,
    r#"<path d="M14 11v6"/>"#,
    r#"<path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/>"#,
    r#"<path d="M3 6h18"/>"#,
    r#"<path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>"#,
    "</svg>"
);

/// `shield-alert` — insecure-site indicator (replaces ⚠️).
pub const SHIELD_ALERT: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
    r#"<path d="M20 13c0 5-3.5 7.5-8 9-4.5-1.5-8-4-8-9V5l8-3 8 3z"/>"#,
    r#"<line x1="12" y1="8" x2="12" y2="13"/>"#,
    r#"<circle cx="12" cy="16" r="0.6" fill="currentColor"/>"#,
    "</svg>"
);

// ── Brand mark ─────────────────────────────────────────────────────────────
//
// Hand-drawn octopus for new-tab and error-page heros. Eight tentacles
// curving outward, two dots for eyes. Not a copy of the .icns mascot, just
// a friendly mark that renders identically across macOS versions (unlike the
// 🐙 emoji which varies).

/// Octopus brand mark. 64×64 viewBox. Uses `currentColor` so it inherits the
/// page's text colour and adapts to dark mode automatically.
pub const OCTOPUS_BRAND: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" fill="currentColor">"#,
    // Head/body
    r#"<ellipse cx="32" cy="26" rx="16" ry="14"/>"#,
    // Eyes (negative space)
    r#"<circle cx="26" cy="24" r="2.4" fill="rgba(255,255,255,0.95)"/>"#,
    r#"<circle cx="38" cy="24" r="2.4" fill="rgba(255,255,255,0.95)"/>"#,
    r#"<circle cx="26.5" cy="24.5" r="1.1" fill="rgba(0,0,0,0.6)"/>"#,
    r#"<circle cx="38.5" cy="24.5" r="1.1" fill="rgba(0,0,0,0.6)"/>"#,
    // Eight tentacles — outer curl, paired
    r#"<path d="M18 36 q -6 4 -8 12 q 5 -2 9 -7 z"/>"#,
    r#"<path d="M22 38 q -4 7 -3 14 q 5 -5 6 -12 z"/>"#,
    r#"<path d="M27 39 q -1 8 1 16 q 4 -7 3 -15 z"/>"#,
    r#"<path d="M32 39 q 1 8 0 17 q 2 -8 3 -17 z"/>"#,
    r#"<path d="M37 39 q 2 9 5 16 q 1 -9 -2 -16 z"/>"#,
    r#"<path d="M42 38 q 5 6 7 13 q 0 -8 -4 -14 z"/>"#,
    r#"<path d="M46 36 q 7 4 10 11 q -2 -8 -7 -12 z"/>"#,
    "</svg>"
);

// Tiny silencer for the lint that prefix/suffix consts are unused — they
// document the canonical shape used by the constants above.
#[allow(dead_code)]
const _ASSERT_UNUSED: (&str, &str) = (PREFIX, SUFFIX);
