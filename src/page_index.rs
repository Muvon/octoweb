//! Full-text memory of visited pages — "I saw it somewhere last week".
//!
//! An in-memory inverted index over the first few KB of each page's visible
//! text. Persisted as plain JSON next to history.json and rebuilt on load.
//! Private tabs never reach it — callers skip them before indexing.
//!
//! ponytail: capped at MAX_DOCS × MAX_TEXT chars (~12 MB worst case) and
//! rebuilt on startup; move to SQLite FTS5 if the cap bites.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

const MAX_DOCS: usize = 2000;
const MAX_TEXT: usize = 6000;
const MIN_TERM_LEN: usize = 2;
const SNIPPET_CHARS: usize = 140;
/// Prefix expansion stops here so a one-letter query can't fan out to the
/// whole vocabulary.
const MAX_PREFIX_KEYS: usize = 200;

#[derive(Serialize, Deserialize, Clone)]
struct Doc {
    url: String,
    title: String,
    visited_at: u64,
    text: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub visited_at: u64,
    pub snippet: String,
}

#[derive(Default)]
pub struct PageIndex {
    /// Insertion order; oldest first so eviction is a front pop.
    docs: Vec<(u64, Doc)>,
    /// term → doc ids, ascending. Ids of removed docs linger until the next
    /// load rebuilds the map — search drops them.
    terms: BTreeMap<String, Vec<u64>>,
    next_id: u64,
}

impl PageIndex {
    pub fn load() -> Self {
        let docs: Vec<Doc> = std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut idx = Self::default();
        for d in docs {
            idx.push(d);
        }
        idx
    }

    pub fn save(&self) {
        let p = path();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let docs: Vec<&Doc> = self.docs.iter().map(|(_, d)| d).collect();
        match serde_json::to_string(&docs) {
            Ok(s) => {
                let _ = std::fs::write(&p, s);
            }
            Err(e) => tracing::warn!(error = %e, "Failed to serialize page index"),
        }
    }

    /// Index (or re-index) one page. Empty text is ignored so a blank paint
    /// never overwrites a good snapshot of the same URL.
    pub fn index(&mut self, url: &str, title: &str, text: &str) {
        self.index_capped(url, title, text, MAX_DOCS);
    }

    fn index_capped(&mut self, url: &str, title: &str, text: &str, max_docs: usize) {
        let text = collapse_ws(text);
        if text.is_empty() {
            return;
        }
        let key = url.trim_end_matches('/');
        self.docs
            .retain(|(_, d)| d.url.trim_end_matches('/') != key);
        while self.docs.len() >= max_docs {
            self.docs.remove(0);
        }
        let mut text = text;
        text.truncate(floor_char_boundary(&text, MAX_TEXT));
        self.push(Doc {
            url: url.to_string(),
            title: title.to_string(),
            visited_at: unix_now(),
            text,
        });
    }

    fn push(&mut self, doc: Doc) {
        let id = self.next_id;
        self.next_id += 1;
        for term in tokenize(&format!("{} {}", doc.title, doc.text)) {
            self.terms.entry(term).or_default().push(id);
        }
        self.docs.push((id, doc));
    }

    /// AND over every query word, each matched as a prefix. Most recent first.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let words: Vec<String> = tokenize(query).into_iter().collect();
        if words.is_empty() {
            return Vec::new();
        }
        let live: HashMap<u64, &Doc> = self.docs.iter().map(|(id, d)| (*id, d)).collect();
        let mut matched: Option<HashSet<u64>> = None;
        for w in &words {
            let mut ids = HashSet::new();
            for (key, posting) in self.terms.range(w.clone()..).take(MAX_PREFIX_KEYS) {
                if !key.starts_with(w.as_str()) {
                    break;
                }
                ids.extend(posting.iter().copied().filter(|id| live.contains_key(id)));
            }
            matched = Some(match matched {
                None => ids,
                Some(prev) => prev.intersection(&ids).copied().collect(),
            });
            if matched.as_ref().is_some_and(|m| m.is_empty()) {
                return Vec::new();
            }
        }
        let mut docs: Vec<&Doc> = matched
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| live.get(&id).copied())
            .collect();
        docs.sort_by(|a, b| b.visited_at.cmp(&a.visited_at));
        docs.into_iter()
            .take(limit)
            .map(|d| Hit {
                title: d.title.clone(),
                url: d.url.clone(),
                visited_at: d.visited_at,
                snippet: snippet(&d.text, &words[0]),
            })
            .collect()
    }
}

fn path() -> PathBuf {
    crate::config::base_dir()
        .join("octoweb")
        .join("page_index.json")
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Lowercase alphanumeric runs, deduplicated. Order is irrelevant for the
/// index; a set keeps postings free of duplicates.
fn tokenize(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= MIN_TERM_LEN)
        .map(str::to_string)
        .collect()
}

fn collapse_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// A window of text around the first occurrence of `word` (case-insensitive),
/// or the start of the text when the match is only in the title.
fn snippet(text: &str, word: &str) -> String {
    let lower = text.to_lowercase();
    let start = match lower.find(word) {
        Some(pos) => pos.saturating_sub(SNIPPET_CHARS / 3),
        None => 0,
    };
    let start = floor_char_boundary(text, start);
    // `find` on the lowercased copy can drift past a multi-byte case fold;
    // clamp to the source string.
    let start = start.min(text.len());
    let end = floor_char_boundary(text, (start + SNIPPET_CHARS).min(text.len()));
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.push_str(text[start..end].trim());
    if end < text.len() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx() -> PageIndex {
        let mut i = PageIndex::default();
        i.index(
            "https://a.example/pricing",
            "Acme pricing",
            "Team plan costs 49 dollars per seat monthly",
        );
        i.index(
            "https://b.example/blog",
            "Rust notes",
            "Borrow checker essay about lifetimes and seats",
        );
        i
    }

    #[test]
    fn prefix_and_terms_intersect() {
        let i = idx();
        let hits = i.search("seat", 10);
        assert_eq!(hits.len(), 2, "prefix 'seat' matches seat and seats");
        let hits = i.search("seat dollar", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://a.example/pricing");
        assert!(hits[0].snippet.contains("49 dollars"));
        assert!(i.search("nothinghere", 10).is_empty());
        assert!(i.search("", 10).is_empty());
    }

    #[test]
    fn reindex_replaces_and_cap_evicts_oldest() {
        let mut i = idx();
        i.index(
            "https://a.example/pricing/",
            "Acme pricing",
            "Now free for everyone",
        );
        assert!(i.search("dollar", 10).is_empty(), "old text gone");
        assert_eq!(i.search("free", 10).len(), 1);
        assert_eq!(
            i.docs.len(),
            2,
            "same URL with trailing slash replaced, not duplicated"
        );

        i.index_capped("https://c.example", "C", "third page", 2);
        assert_eq!(i.docs.len(), 2);
        assert!(i.search("lifetimes", 10).is_empty(), "oldest doc evicted");
        assert_eq!(i.search("third", 10).len(), 1);
        // Empty text never overwrites.
        i.index("https://c.example", "C", "   ");
        assert_eq!(i.search("third", 10).len(), 1);
    }
}
