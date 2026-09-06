/// Returns the HTML for a custom error page (WKWebView load failure).
/// The URL and error code are baked directly into the HTML so no JS call is needed after load.
pub fn html(url: &str, error_code: &str) -> String {
    let (reason, is_certificate_error) = match error_code {
        "-1001" => ("The request timed out before the page responded.", false),
        "-1003" => ("The server for this address could not be found.", false),
        "-1004" => ("Octoweb could not connect to the server.", false),
        "-1005" => (
            "The network connection was lost while loading the page.",
            false,
        ),
        "-1009" => ("Your Mac appears to be offline.", false),
        "-1100" => (
            "The address refers to a missing file or a URL Octoweb cannot open.",
            false,
        ),
        "-1200" => (
            "A secure connection to the server could not be established.",
            false,
        ),
        "-1201" => (
            "The website’s certificate has expired or is not yet valid.",
            true,
        ),
        "-1202" => ("The website’s certificate is not trusted.", true),
        "-1203" => ("The website’s certificate has an unknown root.", true),
        // Not a WebKit code — octoweb's own, for a page whose renderer keeps
        // dying. See AppEvent::WebContentTerminated.
        "renderer-crash" => (
            "This page repeatedly crashed the renderer process. Reload to try again.",
            false,
        ),
        _ => (
            "The page could not be loaded because of an unknown error.",
            false,
        ),
    };

    let safe_url = html_escape(url);
    let safe_code = html_escape(error_code);
    let safe_reason = html_escape(reason);
    let json_url = serde_json::to_string(url)
        .unwrap_or_else(|_| "\"\"".to_string())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    let back_button = if is_certificate_error {
        r#"<button class="secondary-btn" id="backBtn" type="button">Go back</button>"#
    } else {
        ""
    };

    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Page Load Error</title>
<style>/*@@THEME@@*/
{css}</style>
</head>
<body>
<main class="card">
  <div class="error-symbol" aria-hidden="true">@@ERROR_SYMBOL@@</div>
  <h1>This page can't be opened</h1>
  <p class="message">{safe_reason}</p>
  <div class="address-row">
    <span class="address">{safe_url}</span>
    <button class="copy-btn" id="copyBtn" type="button">Copy address</button>
  </div>
  <p class="error-detail">Error {safe_code}</p>
  <div class="actions">
    <button class="primary-btn" id="retryBtn" type="button">Try again</button>
    {back_button}
  </div>
</main>
<script>
document.getElementById('retryBtn').addEventListener('click', function() {{
  window.ipc.postMessage(JSON.stringify({{ type: 'error_retry', url: {json_url} }}));
}});
document.getElementById('copyBtn').addEventListener('click', function() {{
  window.ipc.postMessage(JSON.stringify({{ type: 'copy_text', text: {json_url} }}));
}});
var backBtn = document.getElementById('backBtn');
if (backBtn) backBtn.addEventListener('click', function() {{ history.back(); }});
</script>
</body>
</html>"#,
        css = ERROR_PAGE_CSS,
        safe_reason = safe_reason,
        safe_url = safe_url,
        safe_code = safe_code,
        json_url = json_url,
        back_button = back_button,
    )
    .replace("/*@@THEME@@*/", crate::theme::CSS)
    .replace("@@ERROR_SYMBOL@@", crate::icons::SHIELD_ALERT)
}

const ERROR_PAGE_CSS: &str = r#"  * { box-sizing: border-box; margin: 0; padding: 0; }

  html, body {
    width: 100%;
    min-height: 100%;
    background: var(--canvas);
    font-family: var(--font-text);
    -webkit-font-smoothing: antialiased;
    color: var(--label);
  }

  body {
    display: flex;
    min-height: 100vh;
    overflow-y: auto;
    padding: 48px 24px;
  }

  .card {
    width: min(100%, 520px);
    margin: auto;
    text-align: center;
    animation: errorIn var(--t-pop) var(--spring);
  }
  @keyframes errorIn {
    from { opacity: 0; transform: translateY(8px); }
    to { opacity: 1; transform: none; }
  }

  .error-symbol {
    width: 40px;
    height: 40px;
    margin: 0 auto 14px;
    color: var(--warn);
  }
  .error-symbol svg { width: 100%; height: 100%; }

  h1 {
    font-family: var(--font-display);
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.025em;
    margin-bottom: 8px;
  }

  .message {
    font-size: 13px;
    line-height: 1.45;
    color: var(--label-2);
    margin-bottom: 20px;
  }

  .address-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-width: 0;
    margin-bottom: 8px;
    padding: 8px 8px 8px 12px;
    border-radius: var(--r-ctl);
    background: var(--fill);
  }

  .address {
    flex: 1;
    min-width: 0;
    font: 13px var(--font-text);
    color: var(--label);
    overflow-wrap: anywhere;
    text-align: left;
  }

  button {
    min-height: 22px;
    border: 0;
    font: 13px var(--font-text);
    cursor: pointer;
  }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }

  .copy-btn {
    flex-shrink: 0;
    padding: 2px 6px;
    border-radius: var(--r-ctl);
    background: transparent;
    color: var(--accent);
  }
  .copy-btn:hover { background: var(--fill-hover); }

  .error-detail {
    margin-bottom: 20px;
    font-size: 11px;
    color: var(--label-2);
  }

  .actions { display: flex; justify-content: center; gap: 8px; }

  .primary-btn,
  .secondary-btn {
    min-height: 32px;
    padding: 6px 16px;
    border-radius: var(--r-capsule);
  }
  .primary-btn { background: var(--accent); color: var(--on-accent); }
  .primary-btn:hover { background: color-mix(in srgb, var(--accent) 90%, var(--fill-hover)); }
  .secondary-btn { background: var(--fill); color: var(--label); }
  .secondary-btn:hover { background: var(--fill-hover); }

  @media (prefers-reduced-motion: reduce) { .card { animation: none; } }
"#;

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
