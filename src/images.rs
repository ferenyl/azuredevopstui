use std::cell::RefCell;
use std::collections::HashMap;

use anyhow::Result;
use image::DynamicImage;
use ratatui::layout::Rect;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use reqwest::Url;

const MARKER_START: &str = "[[image:";
const MARKER_END: &str = "]]";
/// Tallest an image may be drawn, in terminal rows.
const MAX_ROWS: u32 = 20;

/// Placeholder line put where an image appears in text.
pub fn marker(url: &str) -> String {
    format!("{MARKER_START}{url}{MARKER_END}")
}

/// The image URL when `line` is a marker line, ignoring surrounding whitespace.
pub fn image_marker(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix(MARKER_START)?
        .strip_suffix(MARKER_END)
        .filter(|url| !url.is_empty())
}

pub fn image_urls(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(image_marker)
        .map(String::from)
        .collect()
}

/// Short name for a placeholder: the `fileName` query value or the last path segment.
pub fn file_name(url: &str) -> String {
    let Ok(parsed) = Url::parse(url) else {
        return url.into();
    };
    if let Some((_, name)) = parsed.query_pairs().find(|(key, _)| key == "fileName") {
        return name.into_owned();
    }
    parsed
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .map(percent_decode)
        .unwrap_or_else(|| url.into())
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (byte, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Replaces each `<img src="…">` with a paragraph holding an image marker.
pub fn extract_html_images(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = find_ignore_case(rest, "<img") {
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        let tag = &rest[start..start + end + 1];
        out.push_str(&rest[..start]);
        if let Some(src) = attribute(tag, "src") {
            let src = src.replace("&amp;", "&");
            out.push_str(&format!("<p>{}</p>", marker(&src).replace('&', "&amp;")));
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}

fn find_ignore_case(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
}

/// Value of `name` in an HTML tag, quoted or not.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        let before = lower[..at].chars().next_back();
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let value = tag[from..].trim_start().strip_prefix('=')?.trim_start();
        return match value.chars().next()? {
            quote @ ('"' | '\'') => value[1..].split(quote).next(),
            _ => value
                .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .next(),
        };
    }
    None
}

/// Puts each markdown image `![alt](url)` on its own marker line.
pub fn markdown_images(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("![") {
        let after = &rest[start + 2..];
        let Some((url, consumed)) = after.find(']').and_then(|close| {
            after[close + 1..].strip_prefix('(')?;
            let url_start = close + 2;
            let url_end = after[url_start..].find(')')?;
            Some((
                &after[url_start..url_start + url_end],
                url_start + url_end + 1,
            ))
        }) else {
            out.push_str(&rest[..start + 2]);
            rest = after;
            continue;
        };
        let url = url.split_whitespace().next().unwrap_or_default();
        out.push_str(&rest[..start]);
        out.push('\n');
        out.push_str(&marker(url));
        out.push('\n');
        rest = &after[consumed..];
    }
    out.push_str(rest);
    out
}

pub enum ImageState {
    Loading,
    Ready {
        protocol: Box<StatefulProtocol>,
        cols: u16,
        rows: u16,
    },
    Failed,
}

/// Images shown in the detail panel. Disabled when the terminal has no graphics protocol.
pub struct Images {
    picker: Option<Picker>,
    cache: RefCell<HashMap<String, ImageState>>,
    drawn: RefCell<Vec<Rect>>,
    previous: RefCell<Vec<Rect>>,
}

impl Images {
    /// Keeps the picker only for real graphics protocols, not halfblocks.
    pub fn new(picker: Option<Picker>) -> Self {
        Self {
            picker: picker.filter(|picker| picker.protocol_type() != ProtocolType::Halfblocks),
            cache: RefCell::new(HashMap::new()),
            drawn: RefCell::default(),
            previous: RefCell::default(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.picker.is_some()
    }

    /// URLs not loaded or loading yet; they are marked as loading.
    pub fn start_loading(&self, urls: Vec<String>) -> Vec<String> {
        if !self.enabled() {
            return Vec::new();
        }
        let mut cache = self.cache.borrow_mut();
        let mut started = Vec::new();
        for url in urls {
            if !cache.contains_key(&url) {
                cache.insert(url.clone(), ImageState::Loading);
                started.push(url);
            }
        }
        started
    }

    pub fn insert(&self, url: String, image: Result<DynamicImage>) {
        let Some(picker) = &self.picker else {
            return;
        };
        let state = match image {
            Ok(image) => {
                let font = picker.font_size();
                let (cols, rows) = cell_size(
                    image.width(),
                    image.height(),
                    font.width.into(),
                    font.height.into(),
                );
                ImageState::Ready {
                    protocol: Box::new(picker.new_resize_protocol(image)),
                    cols,
                    rows,
                }
            }
            Err(err) => {
                tracing::warn!("failed to load image {url}: {err:#}");
                ImageState::Failed
            }
        };
        self.cache.borrow_mut().insert(url, state);
    }

    pub fn drawn_at(&self, area: Rect) {
        self.drawn.borrow_mut().push(area);
    }

    /// Areas of the last frame's images when they moved or disappeared, leaving stale pixels behind.
    pub fn stale_areas(&self) -> Vec<Rect> {
        let drawn = self.drawn.take();
        let previous = self.previous.replace(drawn);
        if previous == *self.previous.borrow() {
            Vec::new()
        } else {
            previous
        }
    }

    pub fn with<R>(&self, url: &str, f: impl FnOnce(Option<&mut ImageState>) -> R) -> R {
        f(self.cache.borrow_mut().get_mut(url))
    }
}

/// Size in terminal cells for an image of `width`×`height` pixels, at most `MAX_ROWS` tall.
fn cell_size(width: u32, height: u32, font_width: u32, font_height: u32) -> (u16, u16) {
    let cols = width.div_ceil(font_width.max(1)).max(1);
    let rows = height.div_ceil(font_height.max(1)).max(1);
    let (cols, rows) = if rows > MAX_ROWS {
        ((cols * MAX_ROWS).div_ceil(rows).max(1), MAX_ROWS)
    } else {
        (cols, rows)
    };
    (
        u16::try_from(cols).unwrap_or(u16::MAX),
        u16::try_from(rows).unwrap_or(u16::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str =
        "https://dev.azure.com/contoso/p/_apis/wit/attachments/abc?fileName=image.png";

    #[test]
    fn marker_round_trips() {
        assert_eq!(image_marker(&marker(URL)), Some(URL));
        assert_eq!(image_marker(&format!("  {}  ", marker(URL))), Some(URL));
    }

    #[test]
    fn plain_text_is_not_a_marker() {
        assert_eq!(image_marker("[[image:]]"), None);
        assert_eq!(image_marker("see [[image:x]] here"), None);
        assert_eq!(image_marker("text"), None);
    }

    #[test]
    fn image_urls_collect_marker_lines() {
        let text = format!("intro\n{}\nmiddle\n{}\n", marker("a"), marker("b"));

        assert_eq!(image_urls(&text), ["a", "b"]);
    }

    #[test]
    fn file_name_prefers_query_value() {
        assert_eq!(file_name(URL), "image.png");
    }

    #[test]
    fn file_name_falls_back_to_decoded_path() {
        let url = "https://dev.azure.com/o/p/_apis/git/repositories/r/pullRequests/1/attachments/image%20%284%29.png";

        assert_eq!(file_name(url), "image (4).png");
        assert_eq!(file_name("not a url"), "not a url");
    }

    #[test]
    fn html_images_become_marker_paragraphs() {
        let html = r#"<div>Before<img src="https://x/a.png?x=1&amp;fileName=a.png" alt=Image>After<IMG SRC='https://x/b.png'/></div>"#;

        let out = extract_html_images(html);

        assert_eq!(
            out,
            "<div>Before<p>[[image:https://x/a.png?x=1&amp;fileName=a.png]]</p>After<p>[[image:https://x/b.png]]</p></div>"
        );
    }

    #[test]
    fn html_image_without_src_is_dropped() {
        assert_eq!(extract_html_images("a<img alt=x>b"), "ab");
    }

    #[test]
    fn data_src_is_not_mistaken_for_src() {
        assert_eq!(
            extract_html_images(r#"<img data-src="no" src="yes">"#),
            "<p>[[image:yes]]</p>"
        );
    }

    #[test]
    fn markdown_images_get_their_own_line() {
        assert_eq!(
            markdown_images("See ![shot](https://x/image%20%284%29.png) here"),
            "See \n[[image:https://x/image%20%284%29.png]]\n here"
        );
    }

    #[test]
    fn markdown_image_title_is_ignored() {
        assert_eq!(
            markdown_images(r#"![a](https://x/a.png "Title")"#),
            "\n[[image:https://x/a.png]]\n"
        );
    }

    #[test]
    fn markdown_without_images_is_unchanged() {
        let text = "Not ![an image] and [a link](https://x)";

        assert_eq!(markdown_images(text), text);
    }

    #[test]
    fn cell_size_scales_down_tall_images() {
        assert_eq!(cell_size(100, 40, 10, 20), (10, 2));
        assert_eq!(cell_size(100, 800, 10, 20), (5, 20));
        assert_eq!(cell_size(1, 1, 10, 20), (1, 1));
    }

    #[test]
    fn images_are_disabled_without_graphics_protocol() {
        let images = Images::new(Some(Picker::halfblocks()));

        assert!(!images.enabled());
        assert!(images.start_loading(vec!["a".into()]).is_empty());
    }

    #[test]
    fn stale_areas_are_previous_placements_once_moved() {
        let images = Images::new(None);
        let area = Rect::new(0, 0, 4, 2);
        let moved = Rect { y: 1, ..area };

        images.drawn_at(area);
        assert!(images.stale_areas().is_empty());
        images.drawn_at(area);
        assert!(images.stale_areas().is_empty());
        images.drawn_at(moved);
        assert_eq!(images.stale_areas(), [area]);
        assert_eq!(images.stale_areas(), [moved]);
        assert!(images.stale_areas().is_empty());
    }
}
