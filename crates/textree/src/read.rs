//! Reading one Markdown document: links, wiki links, tags and headings, each with its position.
//!
//! The CommonMark structure (code, links, headings, front matter) comes from `pulldown-cmark`.
//! Wiki links and `#tags` are not CommonMark, so they are found by scanning the source text
//! outside every code span, code block, HTML fragment and link destination.

use std::ops::Range as StdRange;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};
use serde::Serialize;

/// A byte range in the source of a document: `start..end`, half open, UTF-8 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

/// A heading.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub range: Range,
    pub line: usize,
    /// Written as text with an underline (`Title` / `-----`).
    #[serde(skip)]
    pub setext: bool,
    /// Sits inside a block quote or a list item, not at the start of a line.
    #[serde(skip)]
    pub nested: bool,
    /// Where the heading text starts and ends (without the `#` marker, closing hashes or underline).
    #[serde(skip)]
    pub title_start: usize,
    #[serde(skip)]
    pub title_end: usize,
}

/// A footnote reference (`[^x]`) or definition (`[^x]: …`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Footnote {
    pub label: String,
    pub definition: bool,
    pub range: Range,
    pub line: usize,
}

/// One use of a tag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TagUse {
    pub tag: String,
    pub range: Range,
    pub line: usize,
}

/// How a link was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    /// `[[name]]`, `[[name|alias]]`, `[[name#heading]]`
    Wiki,
    /// `![[name]]`
    Embed,
    /// `[text](destination)`, `![alt](destination)`, `[text][label]` and friends
    Path,
}

/// A link as written, before it is resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawLink {
    pub syntax: Syntax,
    /// The link exactly as it appears in the source.
    pub text: String,
    /// The name (wiki links) or destination (path links), without the heading fragment.
    pub target: String,
    pub heading: Option<String>,
    pub alias: Option<String>,
    pub range: Range,
    pub line: usize,
    /// Where the heading fragment (the text after `#`) sits in the source bytes, if there is one.
    pub heading_span: Option<Range>,
    /// Where the name (wiki links) or the path part of the destination (path links, before any
    /// `#heading`) sits in the source bytes. `None` when the link has no destination of its own
    /// (a reference link takes it from its definition, see [`RefDef`]).
    pub target_span: Option<Range>,
    /// The destination was written inside `<` `>`.
    pub angle: bool,
    /// For reference links (`[text][label]`, `[label][]`, `[label]`): the normalized label.
    pub label: Option<String>,
}

/// A reference definition: `[label]: destination "title"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefDef {
    /// The normalized label (lower case, white space collapsed).
    pub label: String,
    /// The destination without its `#heading`.
    pub target: String,
    pub heading: Option<String>,
    /// Where the path part of the destination sits in the source bytes.
    pub target_span: Option<Range>,
    pub heading_span: Option<Range>,
    pub angle: bool,
    pub range: Range,
    pub line: usize,
}

/// Everything read from one document.
#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub headings: Vec<Heading>,
    pub tags: Vec<TagUse>,
    pub links: Vec<RawLink>,
    pub defs: Vec<RefDef>,
    /// Links to a heading of the same document (`[x](#heading)`, `[[#heading]]`).
    pub anchors: Vec<RawLink>,
    pub footnotes: Vec<Footnote>,
    /// The front matter block, if the document starts with one.
    pub front_matter: Option<Range>,
}

/// Reads a document. `text` has no byte order mark; `offset` is added to every range so that
/// ranges point into the original bytes (3 when a byte order mark was stripped, else 0).
pub fn read_document(text: &str, offset: usize) -> Reading {
    let lines = LineIndex::new(text);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;

    let mut excluded: Vec<StdRange<usize>> = Vec::new();
    let mut links: Vec<RawLink> = Vec::new();
    let mut headings: Vec<Heading> = Vec::new();
    let mut tags: Vec<TagUse> = Vec::new();
    let mut anchors: Vec<RawLink> = Vec::new();
    let mut footnotes: Vec<Footnote> = Vec::new();
    let mut front_matter: Option<Range> = None;

    struct LinkFrame {
        end: usize,
        last_child_end: usize,
        link_type: LinkType,
        dest: String,
        id: String,
        start: usize,
    }
    struct HeadingFrame {
        range: StdRange<usize>,
        level: u8,
        text: String,
    }
    let mut stack: Vec<LinkFrame> = Vec::new();
    let mut heading: Option<HeadingFrame> = None;

    let parser = Parser::new_ext(text, options);
    let defs_raw: Vec<(String, StdRange<usize>, String)> = parser
        .reference_definitions()
        .iter()
        .map(|(label, def)| (label.to_string(), def.span.clone(), def.dest.to_string()))
        .collect();
    for (event, range) in parser.into_offset_iter() {
        // Children of a link move the end of its text; the destination starts after that.
        let is_link_end = matches!(event, Event::End(TagEnd::Link) | Event::End(TagEnd::Image));
        if !is_link_end {
            if let Some(top) = stack.last_mut() {
                top.last_child_end = top.last_child_end.max(range.end).min(top.end);
            }
        }
        match event {
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                id,
                ..
            })
            | Event::Start(Tag::Image {
                link_type,
                dest_url,
                id,
                ..
            }) => {
                stack.push(LinkFrame {
                    end: range.end,
                    last_child_end: (range.start + 1).min(range.end),
                    link_type,
                    dest: dest_url.to_string(),
                    id: id.to_string(),
                    start: range.start,
                });
            }
            Event::End(TagEnd::Link) | Event::End(TagEnd::Image) => {
                if let Some(mut frame) = stack.pop() {
                    if let Some(parent) = stack.last_mut() {
                        parent.last_child_end =
                            parent.last_child_end.max(frame.end).min(parent.end);
                    }
                    // A collapsed reference link `[x][]`: the parser ends the link before the `[]`.
                    if frame.link_type == LinkType::Collapsed && text[frame.end..].starts_with("[]")
                    {
                        frame.end += 2;
                    }
                    // The destination part (after the closing bracket of the text) is not prose.
                    let mut close_at = None;
                    if let Some(close) = text[frame.last_child_end..frame.end].find(']') {
                        let at = frame.last_child_end + close;
                        close_at = Some(at);
                        if at + 1 < frame.end {
                            excluded.push(at + 1..frame.end);
                        }
                    }
                    let is_path = matches!(
                        frame.link_type,
                        LinkType::Inline
                            | LinkType::Reference
                            | LinkType::ReferenceUnknown
                            | LinkType::Collapsed
                            | LinkType::CollapsedUnknown
                            | LinkType::Shortcut
                            | LinkType::ShortcutUnknown
                    );
                    if is_path && !frame.dest.trim().is_empty() {
                        let (target, heading) = split_fragment(frame.dest.trim());
                        let mut target_span = None;
                        let mut heading_span = None;
                        let mut angle = false;
                        if frame.link_type == LinkType::Inline {
                            if let Some(at) = close_at {
                                if text[at + 1..frame.end].starts_with('(') {
                                    if let Some((token, a)) = dest_token(text, at + 2, frame.end) {
                                        target_span =
                                            path_part(text, token.clone()).map(|r| Range {
                                                start: r.start + offset,
                                                end: r.end + offset,
                                            });
                                        if target_span.is_none() {
                                            // `(#heading)`: the path part is empty, at the `#`.
                                            target_span = Some(Range {
                                                start: token.start + offset,
                                                end: token.start + offset,
                                            });
                                        }
                                        heading_span = fragment_part(text, token).map(|r| Range {
                                            start: r.start + offset,
                                            end: r.end + offset,
                                        });
                                        angle = a;
                                    }
                                }
                            }
                        }
                        let label = (frame.link_type != LinkType::Inline)
                            .then(|| normalize_label(&frame.id));
                        let same_document = target.is_empty();
                        let link = RawLink {
                            syntax: Syntax::Path,
                            text: text[frame.start..frame.end].to_string(),
                            target,
                            heading,
                            alias: None,
                            range: Range {
                                start: frame.start + offset,
                                end: frame.end + offset,
                            },
                            line: lines.line_of(frame.start),
                            heading_span,
                            target_span,
                            angle,
                            label,
                        };
                        if same_document {
                            anchors.push(link);
                        } else {
                            links.push(link);
                        }
                    }
                }
            }
            Event::Start(Tag::CodeBlock(_))
            | Event::Start(Tag::HtmlBlock)
            | Event::Start(Tag::MetadataBlock(_)) => {
                if let Event::Start(Tag::MetadataBlock(_)) = event {
                    read_front_matter_tags(text, range.clone(), &lines, offset, &mut tags);
                    front_matter = Some(Range {
                        start: range.start + offset,
                        end: range.end + offset,
                    });
                }
                excluded.push(range);
            }
            Event::FootnoteReference(label) => footnotes.push(Footnote {
                label: label.to_string(),
                definition: false,
                range: Range {
                    start: range.start + offset,
                    end: range.end + offset,
                },
                line: lines.line_of(range.start),
            }),
            Event::Start(Tag::FootnoteDefinition(label)) => footnotes.push(Footnote {
                label: label.to_string(),
                definition: true,
                range: Range {
                    start: range.start + offset,
                    end: range.end + offset,
                },
                line: lines.line_of(range.start),
            }),
            Event::Code(_) | Event::Html(_) | Event::InlineHtml(_) => {
                excluded.push(range.clone());
                if let (Event::Code(code), Some(h)) = (&event, heading.as_mut()) {
                    h.text.push_str(code);
                }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                heading = Some(HeadingFrame {
                    range,
                    level: level as u8,
                    text: String::new(),
                });
            }
            Event::Text(t) => {
                if let Some(h) = heading.as_mut() {
                    h.text.push_str(&t);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(h) = heading.take() {
                    let slice = text[h.range.clone()].trim_end();
                    let end = h.range.start + slice.len();
                    let start = h.range.start;
                    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
                    let nested = text[line_start..start]
                        .chars()
                        .any(|c| c != ' ' && c != '\t');
                    let first_line = slice
                        .split('\n')
                        .next()
                        .unwrap_or("")
                        .trim_end_matches('\r');
                    let setext = !first_line.trim_start().starts_with('#');
                    let (title_start, title_end) = if setext {
                        (start, start + first_line.trim_end().len())
                    } else {
                        let after = first_line.trim_start_matches('#');
                        let lead = first_line.len() - after.len();
                        let after_ws = after.trim_start();
                        let ts = start + lead + (after.len() - after_ws.len());
                        let mut te = start + first_line.trim_end().len();
                        let content = &text[ts.min(te)..te];
                        let closed = content.trim_end_matches('#');
                        if closed.is_empty() || closed.ends_with(' ') || closed.ends_with('\t') {
                            te = ts + closed.trim_end().len();
                        }
                        (ts.min(te), te)
                    };
                    headings.push(Heading {
                        setext,
                        nested,
                        title_start: title_start + offset,
                        title_end: title_end + offset,
                        level: h.level,
                        text: h.text.trim().to_string(),
                        range: Range {
                            start: h.range.start + offset,
                            end: end + offset,
                        },
                        line: lines.line_of(h.range.start),
                    });
                }
            }
            _ => {}
        }
    }

    let mut defs: Vec<RefDef> = Vec::new();
    for (label, span, dest) in defs_raw {
        excluded.push(span.clone());
        let Some(colon) = text[span.clone()].find("]:") else {
            continue;
        };
        if dest.trim().is_empty() {
            continue;
        }
        if let Some((token, angle)) = dest_token(text, span.start + colon + 2, span.end) {
            let (target, heading) = split_fragment(dest.trim());
            defs.push(RefDef {
                label: normalize_label(&label),
                target,
                heading,
                target_span: path_part(text, token.clone()).map(|r| Range {
                    start: r.start + offset,
                    end: r.end + offset,
                }),
                heading_span: fragment_part(text, token).map(|r| Range {
                    start: r.start + offset,
                    end: r.end + offset,
                }),
                angle,
                range: Range {
                    start: span.start + offset,
                    end: span.end + offset,
                },
                line: lines.line_of(span.start),
            });
        }
    }
    defs.sort_by_key(|d| d.range.start);

    let excluded = merge(excluded);

    // Wiki links, then tags outside code, wiki links and link destinations.
    let mut consumed = excluded.clone();
    for seg in complement(text.len(), &excluded) {
        scan_wiki(
            text,
            seg,
            &lines,
            offset,
            &mut links,
            &mut anchors,
            &mut consumed,
        );
    }
    let consumed = merge(consumed);
    for seg in complement(text.len(), &consumed) {
        scan_tags(text, seg, &lines, offset, &mut tags);
    }

    links.sort_by_key(|l| l.range.start);
    tags.sort_by_key(|t| t.range.start);
    anchors.sort_by_key(|l| l.range.start);
    Reading {
        headings,
        tags,
        links,
        defs,
        anchors,
        footnotes,
        front_matter,
    }
}

/// Lower case, white space collapsed: how reference labels are compared.
fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The destination token that starts at or after `from` (spaces skipped), inside `limit`.
/// Returns the byte range of the token (without `<` `>`) and whether it was angle-bracketed.
fn dest_token(text: &str, from: usize, limit: usize) -> Option<(StdRange<usize>, bool)> {
    let b = text.as_bytes();
    let limit = limit.min(b.len());
    let mut i = from;
    while i < limit && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    if i >= limit {
        return None;
    }
    if b[i] == b'<' {
        let start = i + 1;
        let mut j = start;
        while j < limit && b[j] != b'>' && b[j] != b'\n' {
            if b[j] == b'\\' {
                j += 1;
            }
            j += 1;
        }
        return (j < limit && b[j] == b'>').then_some((start..j, true));
    }
    let start = i;
    let mut depth = 0i32;
    while i < limit {
        match b[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'(' => depth += 1,
            b')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            b' ' | b'\t' | b'\n' | b'\r' => break,
            _ => {}
        }
        i += 1;
    }
    let end = i.min(limit);
    (end > start).then_some((start..end, false))
}

/// The text after the first `#` of a destination token; `None` when there is none.
fn fragment_part(text: &str, token: StdRange<usize>) -> Option<StdRange<usize>> {
    let raw = &text[token.clone()];
    let hash = raw.find('#')?;
    let start = token.start + hash + 1;
    (start < token.end).then_some(start..token.end)
}

/// The part of a destination token before any `#heading`; `None` when that part is empty.
fn path_part(text: &str, token: StdRange<usize>) -> Option<StdRange<usize>> {
    let raw = &text[token.clone()];
    let end = raw.find('#').map_or(token.end, |h| token.start + h);
    (end > token.start).then_some(token.start..end)
}

fn split_fragment(dest: &str) -> (String, Option<String>) {
    match dest.find('#') {
        Some(hash) => {
            let frag = dest[hash + 1..].trim();
            let frag = crate::path::percent_decode(frag);
            (
                dest[..hash].to_string(),
                if frag.is_empty() { None } else { Some(frag) },
            )
        }
        None => (dest.to_string(), None),
    }
}

/// Byte offsets of line starts, to turn a byte position into a 1-based line number.
struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        LineIndex { starts }
    }

    fn line_of(&self, pos: usize) -> usize {
        match self.starts.binary_search(&pos) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
    }
}

fn merge(mut ranges: Vec<StdRange<usize>>) -> Vec<StdRange<usize>> {
    ranges.retain(|r| r.start < r.end);
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut out: Vec<StdRange<usize>> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

/// The parts of `0..len` that are not covered by the (sorted, merged) ranges.
fn complement(len: usize, covered: &[StdRange<usize>]) -> Vec<StdRange<usize>> {
    let mut out = Vec::new();
    let mut pos = 0;
    for r in covered {
        if r.start > pos {
            out.push(pos..r.start);
        }
        pos = pos.max(r.end);
    }
    if pos < len {
        out.push(pos..len);
    }
    out
}

fn scan_wiki(
    text: &str,
    seg: StdRange<usize>,
    lines: &LineIndex,
    offset: usize,
    links: &mut Vec<RawLink>,
    anchors: &mut Vec<RawLink>,
    consumed: &mut Vec<StdRange<usize>>,
) {
    let bytes = text.as_bytes();
    let mut i = seg.start;
    while i + 1 < seg.end {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let start = i + 2;
            let mut j = start;
            let mut closed = false;
            while j < seg.end {
                match bytes[j] {
                    b'\n' | b'[' => break,
                    b']' => {
                        closed = j + 1 < seg.end && bytes[j + 1] == b']';
                        break;
                    }
                    _ => j += 1,
                }
            }
            if closed && !text[start..j].trim().is_empty() {
                let embed = i > seg.start && bytes[i - 1] == b'!';
                let link_start = if embed { i - 1 } else { i };
                let link_end = j + 2;
                consumed.push(link_start..link_end);
                if let Some(w) = parse_wiki(&text[start..j]) {
                    let link = RawLink {
                        syntax: if embed { Syntax::Embed } else { Syntax::Wiki },
                        text: text[link_start..link_end].to_string(),
                        target: w.name.clone(),
                        heading: w.heading.clone(),
                        alias: w.alias.clone(),
                        range: Range {
                            start: link_start + offset,
                            end: link_end + offset,
                        },
                        line: lines.line_of(link_start),
                        heading_span: w.heading_range.as_ref().map(|r| Range {
                            start: start + r.start + offset,
                            end: start + r.end + offset,
                        }),
                        target_span: Some(Range {
                            start: start + w.name_range.start + offset,
                            end: start + w.name_range.end + offset,
                        }),
                        angle: false,
                        label: None,
                    };
                    // `[[#heading]]` points at a heading of its own document.
                    if w.name.is_empty() {
                        anchors.push(link);
                    } else {
                        links.push(link);
                    }
                }
                i = link_end;
                continue;
            }
        }
        i += 1;
    }
}

struct WikiParts {
    name: String,
    name_range: StdRange<usize>,
    heading: Option<String>,
    heading_range: Option<StdRange<usize>>,
    alias: Option<String>,
}

/// Splits the inside of `[[ ]]` into name, heading and alias (with where the name and the
/// heading sit). A link with neither a name nor a heading is not a link.
fn parse_wiki(inner: &str) -> Option<WikiParts> {
    let (target_part, alias) = match inner.find('|') {
        Some(bar) => {
            let alias = inner[bar + 1..].trim();
            (
                &inner[..bar],
                if alias.is_empty() {
                    None
                } else {
                    Some(alias.to_string())
                },
            )
        }
        None => (inner, None),
    };
    let target_part = target_part.trim_end_matches('\\');
    let hash = target_part.find('#');
    let (name_str, heading_raw) = match hash {
        Some(h) => (&target_part[..h], Some(&target_part[h + 1..])),
        None => (target_part, None),
    };
    let name = name_str.trim();
    let heading_info = heading_raw.and_then(|raw| {
        let t = raw.trim();
        if t.is_empty() {
            None
        } else {
            let start = hash.unwrap_or(0) + 1 + (raw.len() - raw.trim_start().len());
            Some((start..start + t.len(), t.to_string()))
        }
    });
    if name.is_empty() && heading_info.is_none() {
        return None;
    }
    let name_range = if name.is_empty() {
        let at = hash.unwrap_or(0);
        at..at
    } else {
        let lead = name_str.len() - name_str.trim_start().len();
        lead..lead + name.len()
    };
    Some(WikiParts {
        name: name.to_string(),
        name_range,
        heading: heading_info.as_ref().map(|(_, t)| t.clone()),
        heading_range: heading_info.map(|(r, _)| r),
        alias,
    })
}

fn is_boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | '[' | '{' | '"' | '\'' | ',' | ';' | '>')
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}

fn scan_tags(
    text: &str,
    seg: StdRange<usize>,
    lines: &LineIndex,
    offset: usize,
    tags: &mut Vec<TagUse>,
) {
    let iter = text[seg.clone()].char_indices();
    for (rel, c) in iter {
        if c != '#' {
            continue;
        }
        let pos = seg.start + rel;
        let before_ok = text[..pos].chars().next_back().map_or(true, is_boundary);
        if !before_ok {
            continue;
        }
        let rest = &text[pos + 1..seg.end];
        let mut len = 0;
        for ch in rest.chars() {
            if is_tag_char(ch) {
                len += ch.len_utf8();
            } else {
                break;
            }
        }
        let name = rest[..len].trim_end_matches(['-', '/']);
        let first_ok = name
            .chars()
            .next()
            .is_some_and(|ch| ch.is_alphabetic() || ch == '_');
        if first_ok {
            tags.push(TagUse {
                tag: name.to_string(),
                range: Range {
                    start: pos + offset,
                    end: pos + 1 + name.len() + offset,
                },
                line: lines.line_of(pos),
            });
        }
    }
}

/// Reads `tags:` (or `tag:`) from a front matter block: `[a, b]`, `a, b` or a `- a` list.
fn read_front_matter_tags(
    text: &str,
    block: StdRange<usize>,
    lines: &LineIndex,
    offset: usize,
    tags: &mut Vec<TagUse>,
) {
    let mut in_list = false;
    let mut pos = block.start;
    for line in text[block.clone()].split_inclusive('\n') {
        let line_start = pos;
        pos += line.len();
        let content = line.trim_end_matches(['\n', '\r']);
        if content.trim() == "---" || content.trim() == "..." {
            in_list = false;
            continue;
        }
        let trimmed = content.trim_start();
        let key_len = if trimmed.starts_with("tags:") {
            Some(5)
        } else if trimmed.starts_with("tag:") {
            Some(4)
        } else {
            None
        };
        if let Some(key_len) = key_len {
            let value_at = line_start + (content.len() - trimmed.len()) + key_len;
            let value = &text[value_at..line_start + content.len()];
            let inner = value.trim().trim_start_matches('[').trim_end_matches(']');
            if inner.trim().is_empty() {
                in_list = true;
            } else {
                in_list = false;
                push_front_matter_items(text, value_at, value, lines, offset, tags, |c| {
                    c == ',' || c == '[' || c == ']' || c.is_whitespace()
                });
            }
            continue;
        }
        if in_list {
            if let Some(item) = trimmed.strip_prefix('-') {
                let item_at = line_start + (content.len() - trimmed.len()) + 1;
                push_front_matter_items(text, item_at, item, lines, offset, tags, |c| {
                    c.is_whitespace()
                });
                continue;
            }
            in_list = false;
        }
    }
}

fn push_front_matter_items(
    text: &str,
    value_at: usize,
    value: &str,
    lines: &LineIndex,
    offset: usize,
    tags: &mut Vec<TagUse>,
    is_separator: impl Fn(char) -> bool,
) {
    let mut cursor = 0;
    for piece in value.split(is_separator) {
        let piece_at = value[cursor..]
            .find(piece)
            .map(|p| cursor + p)
            .unwrap_or(cursor);
        cursor = piece_at + piece.len();
        let name = piece.trim_matches(|c| c == '"' || c == '\'' || c == '#');
        if name.is_empty() {
            continue;
        }
        let name_at = piece_at + piece.find(name).unwrap_or(0);
        let start = value_at + name_at;
        tags.push(TagUse {
            tag: name.to_string(),
            range: Range {
                start: start + offset,
                end: start + name.len() + offset,
            },
            line: lines.line_of(start),
        });
        let _ = text;
    }
}
