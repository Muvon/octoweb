//! Shared Liquid Glass design tokens — macOS Tahoe (26+) visual language.
//!
//! Every overlay template splices this block right after its `<style>` tag:
//!
//! ```ignore
//! template.replace("/*@@THEME@@*/", crate::theme::CSS)
//! ```
//!
//! Overlays consume the tokens instead of declaring local palettes, so all
//! chrome surfaces stay in lockstep: one vibrancy ladder for text, one glass
//! recipe for panels, one concentric radius scale, one motion curve. The
//! accent follows the user's system accent color where WebKit supports the
//! `AccentColor` keyword, falling back to macOS system blue.
//!
//! Token groups:
//!   --label..--label-4      text vibrancy ladder (NSColor label* equivalents)
//!   --glass/-thick/-thin    translucent panel fills (pair with --glass-blur)
//!   --glass-shine           specular top edge + inner hairline of a glass pane
//!   --fill/-hover/-press    neutral control fills
//!   --accent/--on-accent    system accent + text on accent
//!   --ok/--warn/--err       semantic status colors
//!   --brand                 octomind purple for the brand mark only
//!   --r-panel/-card/-ctl/-capsule  concentric radius scale
//!   --spring/--ease, --t-fast/--t-pop  motion
//!
//! Shared classes: `.glass-panel` (floating pane), `.kbd` (shortcut chip).
//! Focus, selection, and reduced-motion rules apply globally.

pub const CSS: &str = r#"
  :root {
    color-scheme: light dark;

    --font-text: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
    --font-display: -apple-system, BlinkMacSystemFont, "SF Pro Display", "Helvetica Neue", sans-serif;
    --font-mono: ui-monospace, "SF Mono", SFMono-Regular, Menlo, monospace;

    --fs-body: 13px;
    --fs-caption: 11px;
    --fs-title: 15px;
    --fs-sheet: 17px;
    --fs-hero: 22px;
    --ctl-min: 22px;

    --label:   rgba(0, 0, 0, 0.85);
    --label-2: rgba(0, 0, 0, 0.50);
    --label-3: rgba(0, 0, 0, 0.26);
    --label-4: rgba(0, 0, 0, 0.10);

    --canvas: #f5f5f7;
    /* Floating panels are opaque: they live in transparent WebViews layered
       over the page, where backdrop-filter has no backdrop to sample, so a
       translucent fill reads as whatever happens to sit behind the window —
       the panel's opacity appeared to change at random. --glass-thin stays
       translucent: it only ever paints over an opaque panel. */
    --glass:       rgb(248, 248, 250);
    --glass-thick: rgb(246, 246, 248);
    --glass-thin:  rgba(252, 252, 254, 0.48);
    --glass-blur:  blur(40px) saturate(200%);
    --glass-shine: inset 0 1px 0 rgba(255, 255, 255, 0.65),
                   inset 0 0 0 0.5px rgba(255, 255, 255, 0.40);
    --hairline: rgba(0, 0, 0, 0.10);
    --shadow-float: 0 0 0 0.5px rgba(0, 0, 0, 0.10),
                    0 24px 60px rgba(0, 0, 0, 0.20),
                    0 3px 10px rgba(0, 0, 0, 0.07);

    --fill:       rgba(0, 0, 0, 0.045);
    --fill-hover: rgba(0, 0, 0, 0.08);
    --fill-press: rgba(0, 0, 0, 0.12);

    --accent: #0a84ff;
    --on-accent: #ffffff;
    /* octomind brand purple — only for the brand mark, never for controls */
    --brand: #9333ea;
    --ok: #34c759;
    --warn: #ff9500;
    --err: #ff3b30;

    --r-panel: 20px;
    --r-card: 14px;
    --r-ctl: 8px;
    --r-capsule: 999px;

    --spring: cubic-bezier(0.34, 1.56, 0.64, 1);
    --ease: cubic-bezier(0.25, 0.1, 0.25, 1);
    --t-fast: 0.12s;
    --t-pop: 0.2s;
  }

  /* AccentColorText resolves to black in WKWebView regardless of accent, so
     text on accent stays white; only the accent itself follows the system. */
  @supports (color: AccentColor) {
    :root { --accent: AccentColor; }
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --label:   rgba(255, 255, 255, 0.85);
      --label-2: rgba(255, 255, 255, 0.55);
      --label-3: rgba(255, 255, 255, 0.28);
      --label-4: rgba(255, 255, 255, 0.10);

      --canvas: #1c1c1e;
      --glass:       rgb(30, 30, 34);
      --glass-thick: rgb(36, 36, 40);
      --glass-thin:  rgba(44, 44, 48, 0.42);
      --glass-shine: inset 0 1px 0 rgba(255, 255, 255, 0.12),
                     inset 0 0 0 0.5px rgba(255, 255, 255, 0.08);
      --hairline: rgba(255, 255, 255, 0.14);
      --shadow-float: 0 0 0 0.5px rgba(0, 0, 0, 0.45),
                      0 24px 60px rgba(0, 0, 0, 0.55),
                      0 3px 10px rgba(0, 0, 0, 0.30);

      --fill:       rgba(255, 255, 255, 0.065);
      --fill-hover: rgba(255, 255, 255, 0.11);
      --fill-press: rgba(255, 255, 255, 0.16);

      --ok: #30d158;
      --warn: #ff9f0a;
      --err: #ff453a;
      --brand: #a855f7;
    }
  }

  @media (prefers-contrast: more) {
    :root {
      --label-2: rgba(0, 0, 0, 0.70);
      --label-3: rgba(0, 0, 0, 0.50);
      --hairline: rgba(0, 0, 0, 0.35);
      --glass-thin:  rgba(248, 248, 250, 0.85);
    }
  }
  @media (prefers-contrast: more) and (prefers-color-scheme: dark) {
    :root {
      --label-2: rgba(255, 255, 255, 0.70);
      --label-3: rgba(255, 255, 255, 0.50);
      --hairline: rgba(255, 255, 255, 0.40);
      --glass-thin:  rgba(30, 30, 34, 0.88);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    *, *::before, *::after { animation: none !important; transition: none !important; }
  }

  :focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  ::selection {
    background: color-mix(in srgb, var(--accent) 28%, transparent);
  }

  .glass-panel {
    background: var(--glass);
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    border-radius: var(--r-panel);
    box-shadow: var(--shadow-float), var(--glass-shine);
  }

  .kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 17px;
    min-height: 18px;
    padding: 1px 4px;
    font-family: var(--font-text);
    font-size: var(--fs-caption);
    font-weight: 500;
    letter-spacing: 0.2px;
    color: var(--label-2);
    background: var(--fill);
    border-radius: 5px;
    box-shadow: 0 0 0 0.5px var(--hairline);
  }
"#;
