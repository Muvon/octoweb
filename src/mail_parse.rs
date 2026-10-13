//! Raw RFC 822 messages into what the mail store keeps: participants, the ids
//! that thread them, bodies, attachments, a preview, and whether the message
//! is bulk mail (spec INB-3). Pure functions, no I/O.

use base64::Engine;
use mail_parser::{HeaderValue, MessageParser, MimeHeaders};
use serde::{Deserialize, Serialize};

/// Characters of body text shown under the subject in the inbox.
const PREVIEW_CHARS: usize = 160;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Addr {
    pub name: String,
    pub addr: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub size: usize,
    pub mime: String,
}

#[derive(Debug, Clone, Default)]
pub struct Parsed {
    pub message_id: String,
    /// `In-Reply-To` then `References`, nearest parent first.
    pub parents: Vec<String>,
    pub from: Addr,
    pub reply_to: Option<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    /// Unix seconds.
    pub date: i64,
    pub text: Option<String>,
    /// With `cid:` images inlined as data URIs, so the body needs nothing from
    /// the network to render.
    pub html: Option<String>,
    pub attachments: Vec<Attachment>,
    pub preview: String,
    pub bulk: bool,
}

/// Parse a full message. `fallback_id` names it when it has no Message-ID.
pub fn parse(raw: &[u8], fallback_id: &str) -> Option<Parsed> {
    let msg = MessageParser::default().parse(raw)?;
    let message_id = msg
        .message_id()
        .map(normalize_id)
        .unwrap_or_else(|| fallback_id.to_string());
    let mut parents = ids(msg.in_reply_to());
    for id in ids(msg.references()).into_iter().rev() {
        if !parents.contains(&id) {
            parents.push(id);
        }
    }
    let first = |a: Option<&mail_parser::Address>| a.and_then(|a| a.first()).map(addr);
    let all = |a: Option<&mail_parser::Address>| -> Vec<Addr> {
        a.map(|a| a.iter().map(addr).filter(|a| !a.addr.is_empty()).collect())
            .unwrap_or_default()
    };
    let from = first(msg.from()).unwrap_or_default();
    let text = msg.body_text(0).map(|t| t.into_owned());
    let html = msg.body_html(0).map(|h| inline_cid_images(&h, &msg));
    let attachments = msg
        .attachments()
        .filter(|part| part.content_id().is_none() || part.attachment_name().is_some())
        .map(|part| Attachment {
            name: part.attachment_name().unwrap_or("attachment").to_string(),
            size: part.contents().len(),
            mime: part
                .content_type()
                .map(|ct| match ct.subtype() {
                    Some(sub) => format!("{}/{sub}", ct.ctype()),
                    None => ct.ctype().to_string(),
                })
                .unwrap_or_default(),
        })
        .collect();
    let header = |name: &'static str| msg.header_raw(name).map(str::trim);
    let bulk = is_bulk(
        header("List-Unsubscribe").is_some(),
        header("List-Id").is_some(),
        header("Precedence"),
        header("Auto-Submitted"),
        &from.addr,
    );
    let preview = preview(text.as_deref().unwrap_or_default(), html.as_deref());
    Some(Parsed {
        message_id,
        parents,
        reply_to: first(msg.reply_to()),
        to: all(msg.to()),
        cc: all(msg.cc()),
        subject: msg.subject().unwrap_or_default().trim().to_string(),
        date: msg.date().map_or(0, |d| d.to_timestamp()),
        text,
        html,
        attachments,
        preview,
        bulk,
        from,
    })
}

/// The attachments of `raw`, in the order `parse` lists them, with contents.
pub fn attachment_bytes(raw: &[u8], index: usize) -> Option<(String, Vec<u8>)> {
    let msg = MessageParser::default().parse(raw)?;
    let part = msg
        .attachments()
        .filter(|part| part.content_id().is_none() || part.attachment_name().is_some())
        .nth(index)?;
    Some((
        part.attachment_name().unwrap_or("attachment").to_string(),
        part.contents().to_vec(),
    ))
}

fn addr(a: &mail_parser::Addr) -> Addr {
    Addr {
        name: a.name().unwrap_or_default().trim().to_string(),
        addr: a.address().unwrap_or_default().trim().to_ascii_lowercase(),
    }
}

fn ids(value: &HeaderValue) -> Vec<String> {
    match value {
        HeaderValue::Text(id) => vec![normalize_id(id)],
        HeaderValue::TextList(list) => list.iter().map(|id| normalize_id(id)).collect(),
        _ => Vec::new(),
    }
}

/// Message-IDs without angle brackets or surrounding space.
pub fn normalize_id(id: &str) -> String {
    id.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string()
}

/// Spec INB-3: newsletters, notifications and other machine mail.
pub fn is_bulk(
    list_unsubscribe: bool,
    list_id: bool,
    precedence: Option<&str>,
    auto_submitted: Option<&str>,
    from: &str,
) -> bool {
    let precedence = precedence.map(str::to_ascii_lowercase);
    let auto = auto_submitted.map(str::to_ascii_lowercase);
    let local = from.split('@').next().unwrap_or_default();
    list_unsubscribe
        || list_id
        || matches!(precedence.as_deref(), Some("bulk" | "list"))
        || auto.is_some_and(|a| a != "no")
        || [
            "noreply",
            "no-reply",
            "no_reply",
            "donotreply",
            "do-not-reply",
            "notifications",
            "mailer-daemon",
        ]
        .iter()
        .any(|n| local == *n || local.starts_with(&format!("{n}+")))
}

/// Subject without reply and forward prefixes, lowercased — for threading.
pub fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_ascii_lowercase();
        let Some(rest) = ["re:", "fw:", "fwd:", "aw:", "sv:", "re :", "fwd :"]
            .iter()
            .find_map(|p| lower.starts_with(p).then(|| s[p.len()..].trim_start()))
        else {
            break;
        };
        s = rest;
    }
    s.to_lowercase()
}

/// First words of the body: quoted lines and the signature dropped, space
/// collapsed. Falls back to the HTML's text.
pub fn preview(text: &str, html: Option<&str>) -> String {
    let source = if text.trim().is_empty() {
        html.map(html_to_text).unwrap_or_default()
    } else {
        text.to_string()
    };
    let mut out = String::new();
    for line in source.lines() {
        let line = line.trim();
        if line == "--" || line == "-- " {
            break;
        }
        if line.starts_with('>') || line.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&line.split_whitespace().collect::<Vec<_>>().join(" "));
        if out.chars().count() >= PREVIEW_CHARS {
            break;
        }
    }
    out.chars().take(PREVIEW_CHARS).collect()
}

/// Visible text of an HTML body: tags, styles and scripts gone, entities
/// for the common characters decoded. For previews and search, not display.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while i < html.len() {
        if lower[i..].starts_with("<style") || lower[i..].starts_with("<script") {
            let close = if lower[i..].starts_with("<style") {
                "</style>"
            } else {
                "</script>"
            };
            i = lower[i..]
                .find(close)
                .map_or(html.len(), |j| i + j + close.len());
            continue;
        }
        if html.as_bytes()[i] == b'<' {
            let end = html[i..].find('>').map_or(html.len(), |j| i + j + 1);
            let tag = &lower[i..end];
            if ["<br", "<p", "</p", "<div", "</div", "<tr", "<li", "<h"]
                .iter()
                .any(|t| tag.starts_with(t))
            {
                out.push('\n');
            }
            i = end;
            continue;
        }
        let ch = html[i..].chars().next().expect("in bounds");
        out.push(ch);
        i += ch.len_utf8();
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// `src="cid:…"` references replaced by data URIs of the matching parts.
fn inline_cid_images(html: &str, msg: &mail_parser::Message) -> String {
    let mut out = html.to_string();
    for part in msg.parts.iter() {
        let Some(cid) = part.content_id() else {
            continue;
        };
        let cid = normalize_id(cid);
        let mime = part
            .content_type()
            .map(|ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("octet-stream")))
            .unwrap_or_else(|| "application/octet-stream".into());
        let data = base64::engine::general_purpose::STANDARD.encode(part.contents());
        out = out.replace(&format!("cid:{cid}"), &format!("data:{mime};base64,{data}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = "Message-ID: <abc@x>\r\n\
In-Reply-To: <p1@x>\r\n\
References: <root@x> <p1@x>\r\n\
From: \"Anna Smith\" <Anna@Example.com>\r\n\
To: me@example.org, Bob <bob@example.org>\r\n\
Subject: Re: Lunch?\r\n\
Date: Tue, 13 Oct 2026 10:00:00 +0000\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Sure, Tuesday works.\r\n\
\r\n\
> earlier quoted line\r\n\
--\r\n\
Anna\r\n";

    #[test]
    fn parses_people_threading_and_preview() {
        let p = parse(RAW.as_bytes(), "fallback").unwrap();
        assert_eq!(p.message_id, "abc@x");
        assert_eq!(p.parents, vec!["p1@x", "root@x"]);
        assert_eq!(
            p.from,
            Addr {
                name: "Anna Smith".into(),
                addr: "anna@example.com".into()
            }
        );
        assert_eq!(p.to.len(), 2);
        assert_eq!(p.subject, "Re: Lunch?");
        assert_eq!(p.preview, "Sure, Tuesday works.");
        assert!(!p.bulk);
        assert_eq!(p.date, 1_791_885_600);
    }

    #[test]
    fn bulk_mail_is_recognised_by_headers_and_sender() {
        assert!(is_bulk(true, false, None, None, "a@b"));
        assert!(is_bulk(false, true, None, None, "a@b"));
        assert!(is_bulk(false, false, Some("Bulk"), None, "a@b"));
        assert!(is_bulk(false, false, None, Some("auto-generated"), "a@b"));
        assert!(!is_bulk(false, false, None, Some("no"), "a@b"));
        assert!(is_bulk(false, false, None, None, "noreply@github.com"));
        assert!(is_bulk(false, false, None, None, "no-reply+x@y.z"));
        assert!(!is_bulk(
            false,
            false,
            Some("first-class"),
            None,
            "anna@example.com"
        ));
    }

    #[test]
    fn subjects_normalise_for_threading() {
        assert_eq!(normalize_subject("Re: RE: Fwd: Lunch?"), "lunch?");
        assert_eq!(normalize_subject("  Lunch? "), "lunch?");
    }

    #[test]
    fn html_previews_skip_markup() {
        assert_eq!(
            preview(
                "",
                Some("<style>p{}</style><p>Hello&nbsp;<b>there</b></p><p>bye</p>")
            ),
            "Hello there bye"
        );
    }

    #[test]
    fn missing_message_id_uses_the_fallback() {
        let raw = "From: a@b\r\nSubject: x\r\n\r\nbody\r\n";
        assert_eq!(
            parse(raw.as_bytes(), "7.1@acct").unwrap().message_id,
            "7.1@acct"
        );
    }
}
