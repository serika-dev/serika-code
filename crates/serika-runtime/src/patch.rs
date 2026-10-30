use crate::error::{Error, Result};

/// Apply a unified diff. Each file section is applied independently.
/// A hunk that does not match the current file is rejected, not forced.
pub fn apply_unified_diff(original: &str, patch: &str) -> Result<String> {
    let hunks = parse_hunks(patch)?;
    if hunks.is_empty() {
        return Err(Error::msg("the patch did not contain any hunks"));
    }
    let mut lines = to_lines(original);
    // Apply from the bottom so earlier line numbers stay valid when possible.
    // We still search, because models often get the line numbers slightly wrong.
    for hunk in hunks.into_iter().rev() {
        apply_hunk(&mut lines, &hunk)?;
    }
    Ok(from_lines(&lines, original.ends_with('\n') || original.is_empty()))
}

struct Hunk {
    old: Vec<String>,
    new: Vec<String>,
    hint: usize,
}

fn parse_hunks(patch: &str) -> Result<Vec<Hunk>> {
    let mut hunks = Vec::new();
    let mut current: Option<Hunk> = None;
    // The final newline of the patch is a terminator, not a blank context line.
    let patch = patch.trim_end_matches(['\n', '\r']);
    for raw in patch.split('\n') {
        let line = raw.trim_end_matches('\r');
        if line.starts_with("@@") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            current = Some(Hunk {
                old: Vec::new(),
                new: Vec::new(),
                hint: hint_line(line),
            });
            continue;
        }
        if line.starts_with("---") || line.starts_with("+++") || line.starts_with("diff ") || line.starts_with("index ") {
            continue;
        }
        let Some(hunk) = current.as_mut() else {
            continue;
        };
        if let Some(text) = line.strip_prefix('-') {
            hunk.old.push(text.to_string());
        } else if let Some(text) = line.strip_prefix('+') {
            hunk.new.push(text.to_string());
        } else if let Some(text) = line.strip_prefix(' ') {
            hunk.old.push(text.to_string());
            hunk.new.push(text.to_string());
        } else if line.is_empty() && !hunk.old.is_empty() {
            // A blank context line is represented as " ".
            hunk.old.push(String::new());
            hunk.new.push(String::new());
        }
    }
    if let Some(hunk) = current {
        hunks.push(hunk);
    }
    Ok(hunks)
}

fn hint_line(header: &str) -> usize {
    // @@ -12,3 +12,4 @@
    let Some(start) = header.find('-') else {
        return 0;
    };
    let rest = &header[start + 1..];
    let number: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    number.parse::<usize>().unwrap_or(1).saturating_sub(1)
}

fn apply_hunk(lines: &mut Vec<String>, hunk: &Hunk) -> Result<()> {
    if hunk.old.is_empty() && hunk.new.is_empty() {
        return Err(Error::msg("empty hunk"));
    }
    if hunk.old.is_empty() {
        let at = hunk.hint.min(lines.len());
        for (offset, line) in hunk.new.iter().enumerate() {
            lines.insert(at + offset, line.clone());
        }
        return Ok(());
    }
    let at = find_span(lines, &hunk.old, hunk.hint).ok_or_else(|| {
        Error::msg("patch context did not match the file. Read the file and retry with the current text.")
    })?;
    lines.splice(at..at + hunk.old.len(), hunk.new.iter().cloned());
    Ok(())
}

fn find_span(lines: &[String], needle: &[String], hint: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(hint.min(lines.len()));
    }
    if needle.len() > lines.len() {
        return None;
    }
    // Search outward from the line number in the header, so a context block that
    // appears more than once resolves to the occurrence the patch was written for.
    let last = lines.len() - needle.len();
    let hint = hint.min(last);
    let matches = |start: usize| lines[start..start + needle.len()] == needle[..];
    for distance in 0..=last {
        let below = hint + distance;
        if below <= last && matches(below) {
            return Some(below);
        }
        if distance > 0 && distance <= hint && matches(hint - distance) {
            return Some(hint - distance);
        }
        if below > last && distance > hint {
            break;
        }
    }
    None
}

fn to_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let trimmed = text.strip_suffix('\n').unwrap_or(text);
    trimmed.split('\n').map(str::to_string).collect()
}

fn from_lines(lines: &[String], trailing_newline: bool) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut text = lines.join("\n");
    if trailing_newline {
        text.push('\n');
    }
    text
}

/// Exact search/replace. `all` is required when the search text appears more than once.
pub fn replace_exact(original: &str, search: &str, replace: &str, all: bool) -> Result<String> {
    if search.is_empty() {
        return Err(Error::msg("search text is empty. Use write_file to create a file."));
    }
    let count = original.matches(search).count();
    if count == 0 {
        return Err(Error::msg(
            "search text was not found. Read the file and copy the text exactly.",
        ));
    }
    if count > 1 && !all {
        return Err(Error::msg(format!(
            "search text matched {count} places. Include more context, or set all to true."
        )));
    }
    Ok(if all {
        original.replace(search, replace)
    } else {
        original.replacen(search, replace, 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_unique_text_and_rejects_ambiguity() {
        let updated = replace_exact("alpha beta alpha", "beta", "gamma", false).unwrap();
        assert_eq!(updated, "alpha gamma alpha");
        assert!(replace_exact("alpha alpha", "alpha", "b", false).is_err());
        assert_eq!(replace_exact("alpha alpha", "alpha", "b", true).unwrap(), "b b");
    }

    #[test]
    fn applies_a_unified_hunk_with_a_wrong_line_number() {
        let original = "one\ntwo\nthree\n";
        let patch = "--- a/f\n+++ b/f\n@@ -40,1 +40,1 @@\n-two\n+TWO\n";
        let updated = apply_unified_diff(original, patch).unwrap();
        assert_eq!(updated, "one\nTWO\nthree\n");
    }

    #[test]
    fn rejects_a_hunk_that_does_not_match() {
        let original = "one\ntwo\n";
        let patch = "@@ -1,1 +1,1 @@\n-missing\n+nope\n";
        assert!(apply_unified_diff(original, patch).is_err());
    }

    #[test]
    fn repeated_context_resolves_near_the_header_line() {
        let original = "x\nsame\ny\nx\nsame\ny\n";
        let patch = "@@ -5,1 +5,1 @@\n-same\n+changed\n";
        let updated = apply_unified_diff(original, patch).unwrap();
        assert_eq!(updated, "x\nsame\ny\nx\nchanged\ny\n");
    }

    #[test]
    fn applies_two_hunks_and_keeps_blank_context() {
        let original = "a\n\nb\nc\nd\ne\n";
        let patch = "--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n a\n\n-b\n+B\n@@ -5,2 +5,2 @@\n d\n-e\n+E\n";
        let updated = apply_unified_diff(original, patch).unwrap();
        assert_eq!(updated, "a\n\nB\nc\nd\nE\n");
    }
}
