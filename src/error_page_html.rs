/// Returns the HTML for a custom error page (WKWebView load failure).
/// The URL and error code are baked directly into the HTML so no JS call is needed after load.
/// Light/dark adaptive via prefers-color-scheme.
pub fn html(url: &str, error_code: &str) -> String {
    // Map NSURLError codes to human-readable messages and names
    let (message, error_name) = match error_code {
        "-1001" => (
            "The request timed out. The server might be slow or unreachable.",
            "Timeout",
        ),
        "-1003" => (
            "Cannot find server. Check the address or your internet connection.",
            "DNS Failure",
        ),
        "-1004" => (
            "Could not connect to the server. It might be down.",
            "Connection Refused",
        ),
        "-1005" => ("The network connection was lost.", "Network Lost"),
        "-1009" => (
            "You appear to be offline. Check your internet connection.",
            "Offline",
        ),
        "-1100" => ("The URL is not valid.", "Invalid URL"),
        "-1200" => ("A secure connection could not be established.", "SSL Error"),
        "-1201" => ("The certificate is not trusted.", "Certificate Untrusted"),
        "-1202" => ("The certificate has expired.", "Certificate Expired"),
        "-1203" => (
            "The certificate domain does not match.",
            "Certificate Mismatch",
        ),
        _ => (
            "The page couldn't be loaded. Even the best swimmers get caught in currents sometimes.",
            "Unknown",
        ),
    };

    // Escape for safe HTML embedding
    let safe_url = html_escape(url);
    let safe_code = html_escape(error_code);
    let safe_msg = html_escape(message);
    let safe_name = html_escape(error_name);
    // JSON-encode URL for the retry button script
    let json_url = js_string_escape(url);

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
<div class="card">
  <div class="octopus">@@OCTOPUS_BRAND@@</div>
  <h1>Oops! Tentacles got tangled</h1>
  <p class="message">{safe_msg}</p>
  <div class="error-code">{safe_url}</div>
  <div class="error-detail">{safe_name} ({safe_code})</div>
  <button class="retry-btn" id="retryBtn">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
      <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/>
      <path d="M3 3v5h5"/>
    </svg>
    Try Again
  </button>
</div>
<script>
document.getElementById('retryBtn').addEventListener('click', function() {{
  window.ipc.postMessage(JSON.stringify({{ type: 'error_retry', url: '{json_url}' }}));
}});
</script>
</body>
</html>"#,
        css = ERROR_PAGE_CSS,
        safe_msg = safe_msg,
        safe_url = safe_url,
        safe_name = safe_name,
        safe_code = safe_code,
        json_url = json_url,
    )
    .replace("/*@@THEME@@*/", crate::theme::CSS)
    .replace("@@OCTOPUS_BRAND@@", crate::icons::OCTOPUS_BRAND)
}

// Static CSS kept separate to avoid escaping all braces in format!
const ERROR_PAGE_CSS: &str = r#"  * { box-sizing: border-box; margin: 0; padding: 0; }

  html, body {
    width: 100%;
    height: 100%;
    background: var(--canvas);
    font-family: var(--font-text);
    -webkit-font-smoothing: antialiased;
    color: var(--label);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .card {
    padding: 40px 48px;
    max-width: 420px;
    text-align: center;
    animation: errorIn var(--t-pop) var(--spring);
  }
  @keyframes errorIn {
    from { opacity: 0; transform: translateY(8px) scale(0.98); }
    to { opacity: 1; transform: none; }
  }
  .octopus {
    width: 72px;
    height: 72px;
    margin: 0 auto 16px;
    color: color-mix(in srgb, var(--err) 65%, var(--warn));
    animation: float 3s var(--ease) infinite;
  }
  .octopus svg { width: 100%; height: 100%; }
  @keyframes float {
    0%, 100% { transform: translateY(0); }
    50% { transform: translateY(-8px); }
  }

  h1 {
    font-family: var(--font-display);
    font-size: 28px;
    font-weight: 650;
    margin-bottom: 8px;
    letter-spacing: -0.035em;
  }

  .message {
    font-size: 15px;
    color: var(--label-2);
    margin-bottom: 24px;
    line-height: 1.5;
  }

  .error-code {
    font-size: 12px;
    font-family: var(--font-mono);
    color: var(--label-2);
    background: var(--fill);
    padding: 6px 12px;
    border-radius: var(--r-ctl);
    margin-bottom: 8px;
    word-break: break-all;
  }

  .error-detail {
    font-size: 11px;
    font-family: var(--font-mono);
    color: var(--err);
    background: color-mix(in srgb, var(--err) 9%, transparent);
    padding: 4px 10px;
    border-radius: var(--r-ctl);
    margin-bottom: 20px;
  }

  .retry-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 36px;
    background: var(--accent);
    color: var(--on-accent);
    border: none;
    border-radius: var(--r-capsule);
    padding: 12px 24px;
    font-size: 15px;
    font-weight: 500;
    cursor: pointer;
    box-shadow: inset 0 1px 0 color-mix(in srgb, white 30%, transparent),
                0 4px 14px color-mix(in srgb, var(--accent) 35%, transparent);
    transition: background var(--t-fast) var(--ease), transform var(--t-fast) var(--ease),
                box-shadow var(--t-fast) var(--ease);
  }

  .retry-btn:hover {
    background: color-mix(in srgb, var(--accent) 92%, var(--fill-hover));
    box-shadow: inset 0 1px 0 color-mix(in srgb, white 32%, transparent),
                0 6px 18px color-mix(in srgb, var(--accent) 42%, transparent);
    transform: translateY(-1px);
  }
  .retry-btn:active { background: color-mix(in srgb, var(--accent) 86%, var(--fill-press)); transform: scale(0.97); }
  .retry-btn svg { width: 16px; height: 16px; }
"#;

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn js_string_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
