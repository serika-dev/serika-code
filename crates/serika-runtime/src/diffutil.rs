#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Context(String),
    Add(String),
    Remove(String),
    Header(String),
}

/// Line diff with a small LCS for modest files, and a coarse fallback past that.
pub fn unified_diff(path: &str, old: &str, new: &str) -> String {
    if old == new {
        return String::new();
    }
    let old_lines: Vec<&str> = lines_of(old);
    let new_lines: Vec<&str> = lines_of(new);
    let body = if old_lines.len() <= 800 && new_lines.len() <= 800 {
        lcs_hunks(&old_lines, &new_lines)
    } else {
        coarse(&old_lines, &new_lines)
    };
    format!("--- a/{path}\n+++ b/{path}\n{body}")
}

pub fn parse_unified(diff: &str) -> Vec<DiffLine> {
    diff.lines()
        .map(|line| {
            if line.starts_with("+++") || line.starts_with("---") || line.starts_with("@@") || line.starts_with("diff ") {
                DiffLine::Header(line.to_string())
            } else if let Some(rest) = line.strip_prefix('+') {
                DiffLine::Add(rest.to_string())
            } else if let Some(rest) = line.strip_prefix('-') {
                DiffLine::Remove(rest.to_string())
            } else if let Some(rest) = line.strip_prefix(' ') {
                DiffLine::Context(rest.to_string())
            } else {
                DiffLine::Context(line.to_string())
            }
        })
        .collect()
}

fn lines_of(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let trimmed = text.strip_suffix('\n').unwrap_or(text);
    trimmed.split('\n').collect()
}

fn coarse(old_lines: &[&str], new_lines: &[&str]) -> String {
    let mut out = format!("@@ -1,{} +1,{} @@\n", old_lines.len(), new_lines.len());
    for line in old_lines {
        out.push('-');
        out.push_str(line);
        out.push('\n');
    }
    for line in new_lines {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn lcs_hunks(old_lines: &[&str], new_lines: &[&str]) -> String {
    let n = old_lines.len();
    let m = new_lines.len();
    let mut dp = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if old_lines[i] == new_lines[j] {
                dp[i + 1][j + 1].saturating_add(1)
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if old_lines[i] == new_lines[j] {
            ops.push((' ', old_lines[i]));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(('-', old_lines[i]));
            i += 1;
        } else {
            ops.push(('+', new_lines[j]));
            j += 1;
        }
    }
    while i < n {
        ops.push(('-', old_lines[i]));
        i += 1;
    }
    while j < m {
        ops.push(('+', new_lines[j]));
        j += 1;
    }

    // One hunk is enough for review. Context is kept so the patch can be read.
    let mut body = format!("@@ -1,{n} +1,{m} @@\n");
    for (tag, line) in ops {
        body.push(tag);
        body.push_str(line);
        body.push('\n');
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_added_and_removed_lines() {
        let diff = unified_diff("a.txt", "one\ntwo\n", "one\nthree\n");
        assert!(diff.contains("-two"));
        assert!(diff.contains("+three"));
        assert!(diff.contains(" a/a.txt"));
        let parsed = parse_unified(&diff);
        assert!(parsed.iter().any(|line| matches!(line, DiffLine::Remove(text) if text == "two")));
        assert!(parsed.iter().any(|line| matches!(line, DiffLine::Add(text) if text == "three")));
    }
}
