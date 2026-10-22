//! Line diffs of a file changed by a pull request.

use similar::{ChangeTag, TextDiff};

use crate::api::FileContent;

const CONTEXT_LINES: usize = 3;

#[derive(Debug)]
pub enum FileDiff {
    Lines(Vec<DiffLine>),
    /// One side is binary or too large to diff.
    Skipped,
}

impl FileDiff {
    pub fn of(old: FileContent, new: FileContent) -> Self {
        match (old, new) {
            (FileContent::Text(old), FileContent::Text(new)) => Self::Lines(unified(&old, &new)),
            _ => Self::Skipped,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Hunk,
    Context,
    Added,
    Removed,
}

/// Unified hunks with three lines of context, without the file headers.
pub fn unified(old: &str, new: &str) -> Vec<DiffLine> {
    let diff = TextDiff::from_lines(old, new);
    let mut lines = Vec::new();
    for hunk in diff
        .unified_diff()
        .context_radius(CONTEXT_LINES)
        .iter_hunks()
    {
        lines.push(DiffLine {
            kind: DiffLineKind::Hunk,
            text: hunk.header().to_string(),
        });
        for change in hunk.iter_changes() {
            let kind = match change.tag() {
                ChangeTag::Equal => DiffLineKind::Context,
                ChangeTag::Insert => DiffLineKind::Added,
                ChangeTag::Delete => DiffLineKind::Removed,
            };
            lines.push(DiffLine {
                kind,
                text: change
                    .value()
                    .trim_end_matches(['\n', '\r'])
                    .replace('\t', "    "),
            });
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[DiffLine]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                let sign = match line.kind {
                    DiffLineKind::Hunk => "",
                    DiffLineKind::Context => " ",
                    DiffLineKind::Added => "+",
                    DiffLineKind::Removed => "-",
                };
                format!("{sign}{}", line.text)
            })
            .collect()
    }

    #[test]
    fn unified_keeps_three_lines_of_context() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let new = "1\n2\n3\n4\nfive\n6\n7\n8\n9\n";

        assert_eq!(
            text(&unified(old, new)),
            [
                "@@ -2,7 +2,7 @@",
                " 2",
                " 3",
                " 4",
                "-5",
                "+five",
                " 6",
                " 7",
                " 8"
            ]
        );
    }

    #[test]
    fn distant_changes_get_their_own_hunks() {
        let old: String = (1..=20).map(|n| format!("{n}\n")).collect();
        let new: String = (1..=20)
            .map(|n| match n {
                2 => "two\n".to_string(),
                19 => "nineteen\n".to_string(),
                n => format!("{n}\n"),
            })
            .collect();

        let hunks: Vec<_> = text(&unified(&old, &new))
            .into_iter()
            .filter(|line| line.starts_with("@@"))
            .collect();

        assert_eq!(hunks, ["@@ -1,5 +1,5 @@", "@@ -16,5 +16,5 @@"]);
    }

    #[test]
    fn added_file_is_all_additions_and_tabs_are_expanded() {
        let lines = unified("", "fn x() {\r\n\tbody\r\n}");

        assert_eq!(
            text(&lines),
            ["@@ -0,0 +1,3 @@", "+fn x() {", "+    body", "+}"]
        );
    }

    #[test]
    fn identical_files_have_no_hunks() {
        assert!(unified("same\n", "same\n").is_empty());
    }

    #[test]
    fn skipped_side_skips_the_diff() {
        let text = || FileContent::Text("a\n".into());

        assert!(matches!(
            FileDiff::of(text(), FileContent::Skipped),
            FileDiff::Skipped
        ));
        assert!(matches!(
            FileDiff::of(FileContent::Skipped, text()),
            FileDiff::Skipped
        ));
        assert!(matches!(FileDiff::of(text(), text()), FileDiff::Lines(lines) if lines.is_empty()));
    }
}
