//! Bounded offline RSS/Atom, OPML and persistent read-state.
//! Acquisition and browser execution belong to other lanes. This module never
//! performs network requests, process invocation or HTML rendering.
use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_FEED_BYTES: usize = 256 * 1024;
pub const MAX_OPML_BYTES: usize = 128 * 1024;
pub const MAX_ENTRIES: usize = 128;
pub const MAX_SUBSCRIPTIONS: usize = 128;
pub const MAX_READ_ITEMS: usize = 512;
pub const MAX_STATE_BYTES: usize = 256 * 1024;
const MAX_DEPTH: usize = 24;
const MAX_NODES: usize = 4096;
const MAX_ATTR: usize = 2048;
const MAX_TEXT: usize = 16 * 1024;
const MAX_ID: usize = 512;
const STATE_HEADER: &str = "nuraloumi-feed-reads-v1\n";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    Limit(&'static str),
    Invalid(&'static str),
    Io(String),
}
impl fmt::Display for FeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(s) => write!(f, "feed limit: {s}"),
            Self::Invalid(s) => write!(f, "invalid feed: {s}"),
            Self::Io(s) => write!(f, "feed I/O: {s}"),
        }
    }
}
impl std::error::Error for FeedError {}
fn io_error(e: std::io::Error) -> FeedError {
    FeedError::Io(e.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedFormat {
    Rss,
    Atom,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedEntry {
    pub id: String,
    pub title: String,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub published: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedDocument {
    pub format: FeedFormat,
    pub title: String,
    pub site_url: Option<String>,
    pub entries: Vec<FeedEntry>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subscription {
    pub title: String,
    pub url: String,
}

/// Semantic request only: shell validates action authority and dispatches an
/// argv-based browser adapter (never shell-expands the URL). No executor here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenInBrowser {
    url: String,
}
impl OpenInBrowser {
    pub fn new(url: &str) -> Result<Self, FeedError> {
        if !safe_url(url) {
            return Err(FeedError::Invalid("unsafe browser URL"));
        }
        Ok(Self {
            url: url.to_owned(),
        })
    }
    pub fn url(&self) -> &str {
        &self.url
    }
}
fn safe_url(url: &str) -> bool {
    if url.is_empty()
        || url.len() > MAX_ATTR
        || url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return false;
    }
    let tail = match url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        Some(t) => t,
        None => return false,
    };
    let authority = tail.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || !authority.is_ascii() || authority.contains('@') {
        return false;
    }
    let (host, port, has_port) = if let Some(inner) = authority.strip_prefix('[') {
        let Some((host, suffix)) = inner.split_once(']') else {
            return false;
        };
        if !host.contains(':')
            || !host
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
        {
            return false;
        }
        match suffix.strip_prefix(':') {
            Some(p) => (host, Some(p), true),
            None if suffix.is_empty() => (host, None, false),
            _ => return false,
        }
    } else {
        let mut parts = authority.split(':');
        let host = parts.next().unwrap_or("");
        let port = parts.next();
        if parts.next().is_some()
            || host.is_empty()
            || host.starts_with('.')
            || host.ends_with('.')
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        {
            return false;
        }
        (host, port, port.is_some())
    };
    !host.is_empty()
        && (!has_port
            || port.is_some_and(|p| !p.is_empty() && p.len() <= 5 && p.parse::<u16>().is_ok()))
}

#[derive(Debug)]
enum XmlToken {
    Start(String, Vec<(String, String)>),
    End(String),
    Text(String),
}
fn local(s: &str) -> &str {
    s.rsplit(':').next().unwrap_or(s)
}
fn valid_name(s: &str) -> bool {
    let mut b = s.bytes();
    matches!(b.next(), Some(b'_' | b':' | b'a'..=b'z' | b'A'..=b'Z'))
        && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b':' | b'.' | b'-'))
}
fn push_bounded(dst: &mut String, src: &str, limit: usize) -> Result<(), FeedError> {
    if src.len() > limit.saturating_sub(dst.len()) {
        return Err(FeedError::Limit("decoded text"));
    }
    dst.push_str(src);
    Ok(())
}
fn unescape(s: &str, limit: usize) -> Result<String, FeedError> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        push_bounded(&mut out, &rest[..pos], limit)?;
        let after = &rest[pos + 1..];
        let end = after
            .find(';')
            .ok_or(FeedError::Invalid("unterminated entity"))?;
        if end > 12 {
            return Err(FeedError::Invalid("oversize entity"));
        }
        let ent = &after[..end];
        let ch = match ent {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "apos" => '\'',
            "quot" => '"',
            _ => ent
                .strip_prefix("#x")
                .or_else(|| ent.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse::<u32>().ok()))
                .and_then(char::from_u32)
                .ok_or(FeedError::Invalid("unknown entity"))?,
        };
        if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
            return Err(FeedError::Invalid("XML control entity"));
        }
        push_bounded(&mut out, &ch.to_string(), limit)?;
        rest = &after[end + 1..];
    }
    push_bounded(&mut out, rest, limit)?;
    Ok(out)
}
fn tag(mut src: &str) -> Result<(String, Vec<(String, String)>), FeedError> {
    src = src.trim();
    let split = src.find(char::is_whitespace).unwrap_or(src.len());
    let name = &src[..split];
    if !valid_name(name) {
        return Err(FeedError::Invalid("element name"));
    }
    let mut attrs = Vec::new();
    src = &src[split..];
    while !src.trim_start().is_empty() {
        src = src.trim_start();
        let n = src
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(src.len());
        let key = &src[..n];
        if !valid_name(key) || attrs.len() >= 32 || attrs.iter().any(|(k, _)| k == key) {
            return Err(FeedError::Invalid("attribute name/count"));
        }
        src = src[n..].trim_start();
        src = src
            .strip_prefix('=')
            .ok_or(FeedError::Invalid("attribute equals"))?
            .trim_start();
        let quote = src
            .chars()
            .next()
            .ok_or(FeedError::Invalid("attribute quote"))?;
        if quote != '\'' && quote != '"' {
            return Err(FeedError::Invalid("unquoted attribute"));
        }
        src = &src[1..];
        let end = src
            .find(quote)
            .ok_or(FeedError::Invalid("unclosed attribute"))?;
        if src[..end].contains('<') {
            return Err(FeedError::Invalid("raw '<' in attribute"));
        }
        attrs.push((key.to_owned(), unescape(&src[..end], MAX_ATTR)?));
        src = &src[end + 1..];
    }
    Ok((name.to_owned(), attrs))
}
fn attribute<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}
fn xml(bytes: &[u8], byte_limit: usize) -> Result<Vec<XmlToken>, FeedError> {
    if bytes.len() > byte_limit {
        return Err(FeedError::Limit("XML size"));
    }
    let raw = std::str::from_utf8(bytes).map_err(|_| FeedError::Invalid("XML UTF-8"))?;
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    if raw
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(FeedError::Invalid("raw XML control character"));
    }
    let mut tokens = Vec::new();
    let mut stack = Vec::<String>::new();
    let mut i = 0;
    let mut done = false;
    while i < raw.len() {
        if raw[i..].starts_with("<!--") {
            let end = raw[i + 4..]
                .find("-->")
                .ok_or(FeedError::Invalid("comment"))?;
            i += end + 7;
            continue;
        }
        if raw[i..].starts_with("<![CDATA[") {
            let end = raw[i + 9..]
                .find("]]>")
                .ok_or(FeedError::Invalid("CDATA"))?;
            if stack.is_empty() {
                return Err(FeedError::Invalid("CDATA outside root"));
            }
            let data = &raw[i + 9..i + 9 + end];
            if data.len() > MAX_TEXT {
                return Err(FeedError::Limit("CDATA bytes"));
            }
            tokens.push(XmlToken::Text(data.to_owned()));
            i += end + 12;
        } else if raw[i..].starts_with("<?") {
            let end = raw[i + 2..]
                .find("?>")
                .ok_or(FeedError::Invalid("XML declaration"))?;
            let pi = &raw[i + 2..i + 2 + end];
            if !pi.starts_with("xml ") || !tokens.is_empty() {
                return Err(FeedError::Invalid("unsupported processing instruction"));
            }
            if let Some(p) = pi.find("encoding") {
                let after = pi[p + 8..]
                    .trim_start()
                    .strip_prefix('=')
                    .ok_or(FeedError::Invalid("encoding"))?
                    .trim_start();
                let quote = after.chars().next().ok_or(FeedError::Invalid("encoding"))?;
                if quote != '\'' && quote != '"' {
                    return Err(FeedError::Invalid("encoding"));
                }
                let value = after[1..].split(quote).next().unwrap_or("");
                if !value.eq_ignore_ascii_case("utf-8") {
                    return Err(FeedError::Invalid("non-UTF-8 encoding"));
                }
            }
            i += end + 4;
            continue;
        } else if raw[i..].starts_with("<!") {
            return Err(FeedError::Invalid("DTD/entities forbidden"));
        } else if raw.as_bytes()[i] != b'<' {
            let end = raw[i..].find('<').map_or(raw.len(), |n| i + n);
            if !raw[i..end].trim().is_empty() {
                if stack.is_empty() {
                    return Err(FeedError::Invalid("text outside root"));
                }
                tokens.push(XmlToken::Text(unescape(&raw[i..end], MAX_TEXT)?));
            }
            i = end;
        } else if raw[i..].starts_with("</") {
            let end = i + raw[i..].find('>').ok_or(FeedError::Invalid("end tag"))?;
            let name = raw[i + 2..end].trim();
            if stack.pop().as_deref() != Some(name) {
                return Err(FeedError::Invalid("mismatched end tag"));
            }
            tokens.push(XmlToken::End(name.to_owned()));
            if stack.is_empty() {
                done = true;
            }
            i = end + 1;
        } else {
            let mut quote = None;
            let mut end = None;
            for (offset, ch) in raw[i + 1..].char_indices() {
                match (quote, ch) {
                    (None, '\'' | '"') => quote = Some(ch),
                    (Some(q), c) if c == q => quote = None,
                    (None, '>') => {
                        end = Some(i + 1 + offset);
                        break;
                    }
                    _ => {}
                }
            }
            let end = end.ok_or(FeedError::Invalid("unclosed start tag"))?;
            if done {
                return Err(FeedError::Invalid("multiple roots"));
            }
            let inside = raw[i + 1..end].trim_end();
            let empty = inside.ends_with('/');
            let inside = if empty {
                &inside[..inside.len() - 1]
            } else {
                inside
            };
            let (name, attrs) = tag(inside)?;
            if stack.len() >= MAX_DEPTH {
                return Err(FeedError::Limit("XML depth"));
            }
            stack.push(name.clone());
            tokens.push(XmlToken::Start(name.clone(), attrs));
            if empty {
                stack.pop();
                tokens.push(XmlToken::End(name));
                if stack.is_empty() {
                    done = true;
                }
            }
            i = end + 1;
        }
        if tokens.len() > MAX_NODES {
            return Err(FeedError::Limit("XML events"));
        }
    }
    if !done || !stack.is_empty() || !matches!(tokens.first(), Some(XmlToken::Start(..))) {
        return Err(FeedError::Invalid("incomplete XML root"));
    }
    Ok(tokens)
}
fn plain(s: &str, max: usize) -> Result<String, FeedError> {
    let clean = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.len() > max {
        return Err(FeedError::Limit("field size"));
    }
    Ok(clean)
}
#[derive(Default)]
struct EntryBuilder {
    id: Option<String>,
    title: String,
    url: Option<String>,
    summary: Option<String>,
    published: Option<String>,
}
#[derive(Clone, Copy)]
enum Field {
    FeedTitle,
    FeedLink,
    Id,
    Title,
    Link,
    Summary,
    Published,
}
struct Capture {
    depth: usize,
    field: Field,
    text: String,
}
fn field(format: FeedFormat, parent: &str, name: &str) -> Option<Field> {
    match (format, parent, name) {
        (FeedFormat::Rss, "channel", "title") | (FeedFormat::Atom, "feed", "title") => {
            Some(Field::FeedTitle)
        }
        (FeedFormat::Rss, "channel", "link") => Some(Field::FeedLink),
        (FeedFormat::Rss, "item", "guid") | (FeedFormat::Atom, "entry", "id") => Some(Field::Id),
        (_, "item" | "entry", "title") => Some(Field::Title),
        (FeedFormat::Rss, "item", "link") => Some(Field::Link),
        (_, "item" | "entry", "summary" | "content" | "description") => Some(Field::Summary),
        (_, "item" | "entry", "updated" | "published" | "pubDate") => Some(Field::Published),
        _ => None,
    }
}
pub fn parse_feed(data: &[u8]) -> Result<FeedDocument, FeedError> {
    let tokens = xml(data, MAX_FEED_BYTES)?;
    let format = match tokens.first() {
        Some(XmlToken::Start(name, _)) if local(name) == "rss" => FeedFormat::Rss,
        Some(XmlToken::Start(name, _)) if local(name) == "feed" => FeedFormat::Atom,
        _ => return Err(FeedError::Invalid("RSS/Atom root")),
    };
    let mut doc = FeedDocument {
        format,
        title: String::new(),
        site_url: None,
        entries: Vec::new(),
    };
    let mut saw_rss_channel = false;
    let mut stack = Vec::<String>::new();
    let mut entry: Option<EntryBuilder> = None;
    let mut capture: Option<Capture> = None;
    let mut seen = BTreeSet::new();
    for t in tokens {
        match t {
            XmlToken::Start(name, attrs) => {
                let name = local(&name);
                let parent = stack.last().cloned().unwrap_or_default();
                if format == FeedFormat::Rss && parent == "rss" && name == "channel" {
                    saw_rss_channel = true;
                }
                if format == FeedFormat::Rss && parent == "channel" && name == "item"
                    || format == FeedFormat::Atom && parent == "feed" && name == "entry"
                {
                    entry = Some(EntryBuilder::default());
                }
                if format == FeedFormat::Atom
                    && name == "link"
                    && (parent == "feed" || parent == "entry")
                    && attribute(&attrs, "rel").is_none_or(|r| r == "alternate")
                {
                    if let Some(url) = attribute(&attrs, "href").filter(|u| safe_url(u)) {
                        if parent == "feed" {
                            doc.site_url.get_or_insert_with(|| url.to_owned());
                        } else if let Some(e) = entry.as_mut() {
                            e.url.get_or_insert_with(|| url.to_owned());
                        }
                    }
                }
                stack.push(name.to_owned());
                if capture.is_none() {
                    if let Some(f) = field(format, &parent, name) {
                        capture = Some(Capture {
                            depth: stack.len(),
                            field: f,
                            text: String::new(),
                        });
                    }
                }
            }
            XmlToken::Text(s) => {
                if let Some(c) = capture.as_mut() {
                    push_bounded(&mut c.text, &s, MAX_TEXT)?;
                }
            }
            XmlToken::End(name) => {
                let name = local(&name);
                if capture.as_ref().is_some_and(|c| c.depth == stack.len()) {
                    let c = capture.take().ok_or(FeedError::Invalid("capture"))?;
                    let max = match c.field {
                        Field::FeedTitle | Field::Title => 256,
                        Field::Id => MAX_ID,
                        Field::Summary => 2048,
                        Field::Published => 128,
                        Field::FeedLink | Field::Link => MAX_ATTR,
                    };
                    let value = plain(&c.text, max)?;
                    match c.field {
                        Field::FeedTitle => doc.title = value,
                        Field::FeedLink => {
                            if safe_url(&value) {
                                doc.site_url = Some(value);
                            }
                        }
                        Field::Id => {
                            if let Some(e) = entry.as_mut() {
                                e.id = Some(value);
                            }
                        }
                        Field::Title => {
                            if let Some(e) = entry.as_mut() {
                                e.title = value;
                            }
                        }
                        Field::Link => {
                            if let Some(e) = entry.as_mut() {
                                if safe_url(&value) {
                                    e.url = Some(value);
                                }
                            }
                        }
                        Field::Summary => {
                            if let Some(e) = entry.as_mut() {
                                e.summary = Some(value);
                            }
                        }
                        Field::Published => {
                            if let Some(e) = entry.as_mut() {
                                e.published = Some(value);
                            }
                        }
                    }
                }
                if format == FeedFormat::Rss && name == "item"
                    || format == FeedFormat::Atom && name == "entry"
                {
                    if let Some(e) = entry.take() {
                        let id = e
                            .id
                            .filter(|s| !s.is_empty())
                            .or_else(|| e.url.clone())
                            .ok_or(FeedError::Invalid("entry missing stable id and safe link"))?;
                        if seen.insert(id.clone()) {
                            if doc.entries.len() >= MAX_ENTRIES {
                                return Err(FeedError::Limit("entry count"));
                            }
                            doc.entries.push(FeedEntry {
                                id,
                                title: if e.title.is_empty() {
                                    "(untitled)".into()
                                } else {
                                    e.title
                                },
                                url: e.url,
                                summary: e.summary,
                                published: e.published,
                            });
                        }
                    }
                }
                stack.pop();
            }
        }
    }
    if format == FeedFormat::Rss && !saw_rss_channel {
        return Err(FeedError::Invalid("RSS channel required"));
    }
    Ok(doc)
}
pub fn import_opml(data: &[u8]) -> Result<Vec<Subscription>, FeedError> {
    let tokens = xml(data, MAX_OPML_BYTES)?;
    if !matches!(tokens.first(), Some(XmlToken::Start(s, _)) if local(s) == "opml") {
        return Err(FeedError::Invalid("OPML root"));
    }
    let mut outlines = 0usize;
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for t in tokens {
        match t {
            XmlToken::Start(s, attrs) if local(&s) == "outline" => {
                outlines += 1;
                if outlines > 12 {
                    return Err(FeedError::Limit("OPML depth"));
                }
                if let Some(url) = attribute(&attrs, "xmlUrl") {
                    let request = OpenInBrowser::new(url)?;
                    if seen.insert(request.url.clone()) {
                        if result.len() >= MAX_SUBSCRIPTIONS {
                            return Err(FeedError::Limit("OPML subscriptions"));
                        }
                        result.push(Subscription {
                            title: plain(
                                attribute(&attrs, "title")
                                    .or_else(|| attribute(&attrs, "text"))
                                    .unwrap_or(url),
                                256,
                            )?,
                            url: request.url,
                        });
                    }
                }
            }
            XmlToken::End(s) if local(&s) == "outline" => outlines -= 1,
            _ => {}
        }
    }
    Ok(result)
}

/// Key = (feed URL, item GUID/Atom ID). Versioned, strict line format:
/// header plus lowercase hex(UTF-8 feed URL) TAB hex(UTF-8 ID) NEWLINE.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadState {
    read: BTreeSet<(String, String)>,
}
impl ReadState {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.read.len()
    }
    pub fn is_empty(&self) -> bool {
        self.read.is_empty()
    }
    pub fn is_read(&self, feed: &str, id: &str) -> bool {
        self.read.contains(&(feed.to_owned(), id.to_owned()))
    }
    pub fn mark_read(&mut self, feed: &str, id: &str) -> Result<bool, FeedError> {
        if !safe_url(feed) || id.is_empty() || id.len() > MAX_ID || id.chars().any(char::is_control)
        {
            return Err(FeedError::Invalid("read-state key"));
        }
        let key = (feed.to_owned(), id.to_owned());
        if self.read.contains(&key) {
            return Ok(false);
        }
        if self.read.len() >= MAX_READ_ITEMS {
            return Err(FeedError::Limit("read-state entries"));
        }
        // Never accept a state update that cannot subsequently be persisted.
        let bytes = STATE_HEADER.len()
            + self
                .read
                .iter()
                .map(|(f, id)| 2 * (f.len() + id.len()) + 2)
                .sum::<usize>()
            + 2 * (feed.len() + id.len())
            + 2;
        if bytes > MAX_STATE_BYTES {
            return Err(FeedError::Limit("read-state file size"));
        }
        Ok(self.read.insert(key))
    }
    pub fn mark_unread(&mut self, feed: &str, id: &str) -> bool {
        self.read.remove(&(feed.to_owned(), id.to_owned()))
    }
    fn encode(&self) -> Result<Vec<u8>, FeedError> {
        let mut result = Vec::from(STATE_HEADER.as_bytes());
        for (feed, id) in &self.read {
            for (idx, text) in [feed, id].into_iter().enumerate() {
                if idx != 0 {
                    result.push(b'\t');
                }
                for b in text.bytes() {
                    result.extend_from_slice(format!("{b:02x}").as_bytes());
                }
            }
            result.push(b'\n');
            if result.len() > MAX_STATE_BYTES {
                return Err(FeedError::Limit("read-state file size"));
            }
        }
        Ok(result)
    }
    pub fn load(path: &Path) -> Result<Self, FeedError> {
        let file = File::open(path).map_err(io_error)?;
        let mut bytes = Vec::new();
        file.take((MAX_STATE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(FeedError::Limit("read-state file size"));
        }
        let text =
            std::str::from_utf8(&bytes).map_err(|_| FeedError::Invalid("read-state UTF-8"))?;
        let body = text
            .strip_prefix(STATE_HEADER)
            .ok_or(FeedError::Invalid("read-state header"))?;
        if !body.is_empty() && !body.ends_with('\n') {
            return Err(FeedError::Invalid("truncated read-state line"));
        }
        let mut out = Self::new();
        for line in body.lines() {
            let (feed, id) = line
                .split_once('\t')
                .ok_or(FeedError::Invalid("read-state line"))?;
            if !out.mark_read(&unhex(feed)?, &unhex(id)?)? {
                return Err(FeedError::Invalid("duplicate read-state key"));
            }
        }
        Ok(out)
    }
    pub fn save_atomic(&self, path: &Path) -> Result<(), FeedError> {
        let bytes = self.encode()?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or(FeedError::Invalid("read-state parent"))?;
        let filename = path
            .file_name()
            .ok_or(FeedError::Invalid("read-state filename"))?;
        for _ in 0..8 {
            let seq = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let mut name = filename.to_os_string();
            name.push(format!(".tmp.{}.{}", std::process::id(), seq));
            let temp = parent.join(name);
            let mut opts = OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = match opts.open(&temp) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_error(e)),
            };
            let result = (|| {
                f.write_all(&bytes).map_err(io_error)?;
                f.sync_all().map_err(io_error)?;
                drop(f);
                fs::rename(&temp, path).map_err(io_error)?;
                File::open(parent)
                    .and_then(|dir| dir.sync_all())
                    .map_err(io_error)
            })();
            if result.is_err() {
                let _ = fs::remove_file(&temp);
            }
            return result;
        }
        Err(FeedError::Io("temporary filename collisions".into()))
    }
}
fn unhex(hex: &str) -> Result<String, FeedError> {
    if hex.len() % 2 != 0 || hex.len() > MAX_ATTR * 2 {
        return Err(FeedError::Invalid("read-state hex length"));
    }
    let digit = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks_exact(2) {
        out.push(
            (digit(pair[0]).ok_or(FeedError::Invalid("read-state hex"))? << 4)
                | digit(pair[1]).ok_or(FeedError::Invalid("read-state hex"))?,
        );
    }
    String::from_utf8(out).map_err(|_| FeedError::Invalid("read-state decoded UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rss_fixture() {
        let doc = parse_feed(include_bytes!("../tests/fixtures/feeds/rss.xml")).unwrap();
        assert_eq!(doc.format, FeedFormat::Rss);
        assert_eq!(doc.title, "Rust & Tech");
        assert_eq!(doc.entries.len(), 2);
        assert_eq!(doc.entries[0].title, "Hello <world>");
        assert_eq!(doc.entries[0].id, "news-1");
        assert_eq!(doc.entries[0].summary.as_deref(), Some("An & update"));
        assert_eq!(doc.entries[1].url, None);
    }
    #[test]
    fn atom_fixture() {
        let doc = parse_feed(include_bytes!("../tests/fixtures/feeds/atom.xml")).unwrap();
        assert_eq!(doc.format, FeedFormat::Atom);
        assert_eq!(doc.entries.len(), 1);
        assert_eq!(doc.entries[0].id, "urn:entry:one");
        assert_eq!(doc.entries[0].title, "Café & code");
        assert_eq!(
            doc.entries[0].url.as_deref(),
            Some("https://example.org/entry/1")
        );
        assert_eq!(
            doc.entries[0].published.as_deref(),
            Some("2026-11-03T08:00:00Z")
        );
    }
    #[test]
    fn opml_fixture() {
        let subs =
            import_opml(include_bytes!("../tests/fixtures/feeds/subscriptions.opml")).unwrap();
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].title, "Tech & Rust");
        assert_eq!(subs[1].url, "http://example.net/feed.xml");
    }
    #[test]
    fn malformed_xml_and_unsafe_open() {
        for body in [
            b"<!DOCTYPE rss [<!ENTITY e SYSTEM 'file:///etc/passwd'>]><rss/>".as_slice(),
            b"<rss><channel><item><guid>&bad;</guid></item></channel></rss>",
            b"<rss><channel><item><guid>id</guid></channel></item></rss>",
            b"<rss><channel><item><title>missing id</title></item></channel></rss>",
            b"<?xml version='1.0' encoding='ISO-8859-1'?><rss/>",
        ] {
            assert!(parse_feed(body).is_err());
        }
        for url in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://user@host.test/",
            "https://evil.test\\@host.test/",
            "https://host.test/\narg",
            "http://",
            "https://host.test:bad/",
            "https://host.test:65536/",
        ] {
            assert!(OpenInBrowser::new(url).is_err(), "{url}");
        }
        assert_eq!(
            OpenInBrowser::new("https://example.org/a?x=1")
                .unwrap()
                .url(),
            "https://example.org/a?x=1"
        );
    }
    #[test]
    fn caps_and_opml_hardening() {
        assert!(matches!(
            parse_feed(&vec![0; MAX_FEED_BYTES + 1]),
            Err(FeedError::Limit(_))
        ));
        assert!(matches!(
            import_opml(&vec![0; MAX_OPML_BYTES + 1]),
            Err(FeedError::Limit(_))
        ));
        let deep = format!(
            "<opml><body>{}</body></opml>",
            "<outline>".repeat(13) + &"</outline>".repeat(13)
        );
        assert!(matches!(
            import_opml(deep.as_bytes()),
            Err(FeedError::Limit(_))
        ));
        assert!(
            import_opml(b"<opml><body><outline xmlUrl='javascript:alert(1)'/></body></opml>")
                .is_err()
        );
        let many = format!(
            "<rss><channel>{}</channel></rss>",
            "<item><guid>id</guid></item>".repeat(4300)
        );
        assert!(parse_feed(many.as_bytes()).is_err());
    }
    #[test]
    fn state_roundtrip_and_corruption() {
        let root = std::env::temp_dir().join(format!(
            "nuraloumi-feeds-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("reads");
        let mut state = ReadState::new();
        assert!(state.mark_read("https://example.org/rss", "id-1").unwrap());
        assert!(!state.mark_read("https://example.org/rss", "id-1").unwrap());
        state
            .mark_read("https://example.org/rss", "urn:two")
            .unwrap();
        state.save_atomic(&path).unwrap();
        assert_eq!(state, ReadState::load(&path).unwrap());
        assert!(state.mark_unread("https://example.org/rss", "id-1"));
        state.save_atomic(&path).unwrap();
        assert!(!ReadState::load(&path)
            .unwrap()
            .is_read("https://example.org/rss", "id-1"));
        fs::write(&path, b"corrupt header\n").unwrap();
        assert!(ReadState::load(&path).is_err());
        fs::write(&path, vec![b'x'; MAX_STATE_BYTES + 1]).unwrap();
        assert!(matches!(ReadState::load(&path), Err(FeedError::Limit(_))));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn state_entry_cap() {
        let mut state = ReadState::new();
        for i in 0..MAX_READ_ITEMS {
            state
                .mark_read("https://example.org/feed", &i.to_string())
                .unwrap();
        }
        assert!(matches!(
            state.mark_read("https://example.org/feed", "too many"),
            Err(FeedError::Limit(_))
        ));
    }
    #[test]
    fn exact_entry_and_subscription_limits() {
        let rss = |count: usize| {
            let mut xml = String::from("<rss><channel>");
            for i in 0..count {
                xml.push_str(&format!("<item><guid>entry-{i}</guid></item>"));
            }
            xml.push_str("</channel></rss>");
            xml
        };
        assert_eq!(
            parse_feed(rss(MAX_ENTRIES).as_bytes())
                .unwrap()
                .entries
                .len(),
            MAX_ENTRIES
        );
        assert!(matches!(
            parse_feed(rss(MAX_ENTRIES + 1).as_bytes()),
            Err(FeedError::Limit(_))
        ));
        let opml = |count: usize| {
            let mut xml = String::from("<opml><body>");
            for i in 0..count {
                xml.push_str(&format!("<outline xmlUrl='https://example.org/{i}'/>"));
            }
            xml.push_str("</body></opml>");
            xml
        };
        assert_eq!(
            import_opml(opml(MAX_SUBSCRIPTIONS).as_bytes())
                .unwrap()
                .len(),
            MAX_SUBSCRIPTIONS
        );
        assert!(matches!(
            import_opml(opml(MAX_SUBSCRIPTIONS + 1).as_bytes()),
            Err(FeedError::Limit(_))
        ));
    }
    #[test]
    fn read_state_rejects_oversize_before_mutation() {
        let mut state = ReadState::new();
        let feed = format!("https://example.org/{}", "p".repeat(1800));
        let mut rejected = false;
        for i in 0..MAX_READ_ITEMS {
            match state.mark_read(&feed, &format!("guid-{i}")) {
                Ok(true) => {}
                Err(FeedError::Limit("read-state file size")) => {
                    rejected = true;
                    break;
                }
                outcome => panic!("unexpected result: {outcome:?}"),
            }
        }
        assert!(rejected);
        assert!(state.len() < MAX_READ_ITEMS);
        assert!(state.encode().unwrap().len() <= MAX_STATE_BYTES);
    }
    #[test]
    fn strict_xml_and_persistent_key_encoding() {
        for xml in [
            "<rss><channel><item><guid>&#xD800;</guid></item></channel></rss>",
            "<rss><channel><item><guid>ok</guid><title a='1' a='2'>bad</title></item></channel></rss>",
            "<rss/><rss/>",
            "<rss/>",
            "<rss><channel><title>bad\u{0007}title</title></channel></rss>",
            "<feed><entry><id>urn:ok</id><title>before<!-- comment -->after</title></entry></feed>",
        ] {
            if xml.contains("<!--") {
                let doc = parse_feed(xml.as_bytes()).unwrap();
                assert_eq!(doc.entries[0].title, "beforeafter");
            } else {
                assert!(parse_feed(xml.as_bytes()).is_err());
            }
        }
        let mut state = ReadState::new();
        let id = "urn:café:<test>& 'quote'";
        state.mark_read("https://example.org/", id).unwrap();
        let bytes = state.encode().unwrap();
        assert!(!bytes
            .windows(id.len())
            .any(|window| window == id.as_bytes()));
        let mut corrupt = String::from(STATE_HEADER);
        corrupt.push_str("zz\t00\n");
        assert!(unhex("zz").is_err());
        assert!(corrupt.starts_with(STATE_HEADER));
    }
}
