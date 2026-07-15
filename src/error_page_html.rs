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
<style>{css}</style>
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
    .replace("@@OCTOPUS_BRAND@@", crate::icons::OCTOPUS_BRAND)
}

// Static CSS kept separate to avoid escaping all braces in format!
const ERROR_PAGE_CSS: &str = r#"
  * { box-sizing: border-box; margin: 0; padding: 0; }

  @media (prefers-reduced-motion: reduce) {
    *, *::before, *::after { animation: none !important; transition: none !important; }
  }

  :root {
    --bg: #f5f5f7;
    --card-bg: rgba(255, 255, 255, 0.85);
    --card-border: rgba(0, 0, 0, 0.08);
    --card-shadow: 0 8px 32px rgba(0, 0, 0, 0.08);
    --text-primary: rgba(0, 0, 0, 0.85);
    --text-secondary: rgba(0, 0, 0, 0.50);
    --accent: #007aff;
    --accent-hover: #0066d6;
    --error-color: #ff3b30;
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --bg: #1c1c1e;
      --card-bg: rgba(44, 44, 48, 0.90);
      --card-border: rgba(255, 255, 255, 0.08);
      --card-shadow: 0 8px 32px rgba(0, 0, 0, 0.35);
      --text-primary: rgba(255, 255, 255, 0.90);
      --text-secondary: rgba(255, 255, 255, 0.50);
      --accent: #0a84ff;
      --accent-hover: #409cff;
      --error-color: #ff453a;
    }
  }

  html, body {
    width: 100%;
    height: 100%;
    background: var(--bg);
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
    -webkit-font-smoothing: antialiased;
    color: var(--text-primary);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .card {
    background: var(--card-bg);
    backdrop-filter: blur(20px) saturate(180%);
    -webkit-backdrop-filter: blur(20px) saturate(180%);
    border-radius: 20px;
    box-shadow: 0 0 0 0.5px var(--card-border), var(--card-shadow),
                inset 0 1px 0 rgba(255, 255, 255, 0.5);
    padding: 40px 48px;
    max-width: 420px;
    text-align: center;
  }
  @media (prefers-color-scheme: dark) {
    .card {
      box-shadow: 0 0 0 0.5px var(--card-border), var(--card-shadow),
                  inset 0 1px 0 rgba(255, 255, 255, 0.07);
    }
  }

  .octopus {
    width: 72px;
    height: 72px;
    margin: 0 auto 16px;
    color: rgba(255, 99, 71, 0.78);
    animation: float 3s ease-in-out infinite;
  }
  .octopus svg { width: 100%; height: 100%; }
  @media (prefers-color-scheme: dark) {
    .octopus { color: rgba(255, 122, 99, 0.78); }
  }

  @keyframes float {
    0%, 100% { transform: translateY(0); }
    50% { transform: translateY(-8px); }
  }

  h1 {
    font-size: 22px;
    font-weight: 600;
    margin-bottom: 8px;
    letter-spacing: -0.02em;
  }

  .message {
    font-size: 15px;
    color: var(--text-secondary);
    margin-bottom: 24px;
    line-height: 1.5;
  }

  .error-code {
    font-size: 12px;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    color: var(--text-secondary);
    background: rgba(0, 0, 0, 0.04);
    padding: 6px 12px;
    border-radius: 6px;
    margin-bottom: 8px;
    word-break: break-all;
  }

  .error-detail {
    font-size: 11px;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    color: var(--error-color);
    background: rgba(255, 59, 48, 0.08);
    padding: 4px 10px;
    border-radius: 4px;
    margin-bottom: 20px;
  }

  .retry-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    background: linear-gradient(180deg, #2e9bff 0%, var(--accent) 55%, #0070e8 100%);
    color: white;
    border: none;
    border-radius: 14px;
    padding: 12px 24px;
    font-size: 15px;
    font-weight: 500;
    cursor: pointer;
    box-shadow: 0 2px 10px rgba(0, 122, 255, 0.35),
                inset 0 1px 0 rgba(255, 255, 255, 0.3);
    transition: box-shadow 0.15s ease,
                transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1);
  }

  .retry-btn:hover {
    box-shadow: 0 4px 16px rgba(0, 122, 255, 0.45),
                inset 0 1px 0 rgba(255, 255, 255, 0.3);
    transform: translateY(-1px);
  }
  .retry-btn:active { transform: scale(0.97); transition-duration: 0.08s; }
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
