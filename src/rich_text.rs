use html2text::render::{RichAnnotation, TaggedLine};
use pulldown_cmark::{Event, Options, Parser};

use crate::images::{extract_html_images, image_urls};

/// Wide enough that html2text never wraps; the UI wraps to the panel width.
const RENDER_WIDTH: usize = 10_000;

#[cfg(test)]
thread_local! {
    /// Calls to `RichText::from_markdown` on this thread, to prove rendering does not parse.
    pub static MARKDOWN_RENDERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Formatting of a piece of text, independent of theme colours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
    pub link: bool,
    pub strike: bool,
    pub heading: bool,
    pub muted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub marks: Marks,
    /// Index into `RichText::links` when this text is part of a selectable link.
    pub link: Option<usize>,
}

/// Formatted text from HTML or markdown, one entry per line. Images are marker lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RichText {
    pub lines: Vec<Vec<Segment>>,
    /// Targets of the links in the text, in reading order (mentions and `mailto:` excluded).
    pub links: Vec<String>,
}

impl RichText {
    pub fn from_markdown(markdown: &str) -> Self {
        #[cfg(test)]
        MARKDOWN_RENDERS.with(|count| count.set(count.get() + 1));
        let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
        let events = Parser::new_ext(markdown, options).map(|event| match event {
            Event::SoftBreak => Event::HardBreak,
            event => event,
        });
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, events);
        Self::from_html(&html)
    }

    pub fn from_html(html: &str) -> Self {
        let html = extract_html_images(html);
        let mut links = Vec::new();
        let mut carried = None;
        let lines = match html2text::config::rich()
            .link_footnotes(false)
            .lines_from_read(html.as_bytes(), RENDER_WIDTH)
        {
            Ok(lines) => lines
                .iter()
                .map(|line| segments(line, &mut links, &mut carried))
                .collect(),
            Err(_) => html.lines().map(|line| vec![plain(line)]).collect(),
        };
        let mut text = Self { lines, links };
        text.trim();
        text
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn plain(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn image_urls(&self) -> Vec<String> {
        image_urls(&self.plain())
    }

    fn trim(&mut self) {
        let blank = |line: &Vec<Segment>| line.iter().all(|s| s.text.trim().is_empty());
        while self.lines.last().is_some_and(blank) {
            self.lines.pop();
        }
        let leading = self.lines.iter().take_while(|line| blank(line)).count();
        self.lines.drain(..leading);
    }
}

fn plain(text: &str) -> Segment {
    Segment {
        text: text.to_string(),
        marks: Marks::default(),
        link: None,
    }
}

/// The link a line ended in: its target and index, so a wrapped link stays one entry.
type OpenLink = Option<(String, Option<usize>)>;

fn segments(
    line: &TaggedLine<Vec<RichAnnotation>>,
    links: &mut Vec<String>,
    carried: &mut OpenLink,
) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut link: Option<(String, String)> = None;
    let mut index = None;
    let mut continued = carried.take();
    for piece in line.tagged_strings() {
        let mut marks = Marks::default();
        let mut url = None;
        for annotation in &piece.tag {
            match annotation {
                RichAnnotation::Strong => marks.strong = true,
                RichAnnotation::Emphasis => marks.emphasis = true,
                RichAnnotation::Code | RichAnnotation::Preformat(_) => marks.code = true,
                RichAnnotation::Strikeout => marks.strike = true,
                RichAnnotation::Link(target) => {
                    marks.link = true;
                    url = Some(target.clone());
                }
                _ => {}
            }
        }
        if link.as_ref().map(|(target, _)| target) != url.as_ref() {
            push_link_target(&mut out, link.take());
            link = url.map(|target| (target, String::new()));
            index = match (&link, continued.take()) {
                (Some((target, _)), Some((previous, index))) if *target == previous => index,
                (Some((target, _)), _) => {
                    let mention = piece.s.trim().starts_with(['#', '@']);
                    (!mention && !target.starts_with("mailto:")).then(|| {
                        links.push(target.clone());
                        links.len() - 1
                    })
                }
                (None, _) => None,
            };
        }
        continued = None;
        if let Some((_, text)) = &mut link {
            text.push_str(&piece.s);
        }
        out.push(Segment {
            text: piece.s.clone(),
            marks,
            link: if marks.link { index } else { None },
        });
    }
    *carried = link.as_ref().map(|(target, _)| (target.clone(), index));
    push_link_target(&mut out, link);
    decorate_prefix(&mut out);
    out
}

/// Shows the target after a link unless the text already says it or is a mention.
fn push_link_target(out: &mut Vec<Segment>, link: Option<(String, String)>) {
    let Some((target, text)) = link else {
        return;
    };
    let text = text.trim();
    if text == target || text.starts_with(['#', '@']) || target.starts_with("mailto:") {
        return;
    }
    out.push(Segment {
        text: format!(" ({target})"),
        marks: Marks {
            muted: true,
            ..Marks::default()
        },
        link: None,
    });
}

/// Turns html2text's heading, list and quote prefixes into nicer markers.
fn decorate_prefix(line: &mut [Segment]) {
    let Some(first) = line.first_mut() else {
        return;
    };
    let indent = first.text.len() - first.text.trim_start().len();
    let rest = &first.text[indent..];
    let hashes = rest.chars().take_while(|&c| c == '#').count();
    if hashes > 0 && rest[hashes..].starts_with(' ') {
        first.text = rest[hashes + 1..].to_string();
        for segment in line.iter_mut() {
            segment.marks.heading = true;
        }
        return;
    }
    let mut rest = rest.to_string();
    let mut prefix = first.text[..indent].to_string();
    while let Some(after) = rest.strip_prefix("> ") {
        prefix.push_str("│ ");
        rest = after.to_string();
    }
    if let Some(after) = rest.strip_prefix("* ") {
        prefix.push_str("• ");
        rest = after.to_string();
        for (from, to) in [("[ ] ", "☐ "), ("[x] ", "☑ "), ("[X] ", "☑ ")] {
            if let Some(after) = rest.strip_prefix(from) {
                prefix.push_str(to);
                rest = after.to_string();
            }
        }
    }
    first.text = prefix + &rest;
}

#[cfg(test)]
mod tests {
    use super::RichText;

    #[test]
    fn links_are_collected_in_reading_order_and_tag_their_segments() {
        let text = RichText::from_markdown("[a](https://a.test) and\n\n[b](https://b.test)");

        assert_eq!(text.links, ["https://a.test", "https://b.test"]);
        let tagged: Vec<_> = text
            .lines
            .iter()
            .flatten()
            .filter_map(|s| s.link.map(|i| (s.text.as_str(), i)))
            .collect();
        assert_eq!(tagged, [("a", 0), ("b", 1)]);
    }

    #[test]
    fn a_link_spanning_lines_is_collected_once() {
        let text = RichText::from_html(
            r#"<a href="https://a.test">one<br>two</a> <a href="https://b.test">b</a>"#,
        );

        assert_eq!(text.links, ["https://a.test", "https://b.test"]);
        let tagged: Vec<_> = text.lines.iter().flatten().filter_map(|s| s.link).collect();
        assert_eq!(tagged, [0, 0, 1]);
    }

    #[test]
    fn mentions_and_mailto_links_are_not_selectable() {
        let text = RichText::from_html(
            r##"<a href="mailto:x@y.test">mail</a> <a href="https://u.test">@user</a> <a href="https://w.test">#12</a>"##,
        );

        assert!(text.links.is_empty());
        assert!(text.lines.iter().flatten().all(|s| s.link.is_none()));
    }
}
