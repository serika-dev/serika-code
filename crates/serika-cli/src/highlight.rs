//! Small syntax highlighter for code blocks in the terminal.
//!
//! It knows keywords, strings, comments, numbers, types, and calls for the languages
//! people paste most. Anything it does not recognise passes through unchanged, so a
//! wrong guess only costs colour, never text.

const KEYWORD: &str = "\x1b[38;2;244;114;182m"; // pink
const STRING: &str = "\x1b[38;2;134;239;172m"; // green
const COMMENT: &str = "\x1b[38;2;113;113;122m\x1b[3m"; // grey italic
const NUMBER: &str = "\x1b[38;2;251;146;60m"; // orange
const TYPE: &str = "\x1b[38;2;103;232;249m"; // cyan
const CALL: &str = "\x1b[38;2;147;197;253m"; // blue
const PLAIN: &str = "\x1b[38;2;228;228;231m";
const RESET: &str = "\x1b[0m";

/// Carries an open block comment from one line to the next.
#[derive(Default)]
pub struct State {
    in_block_comment: bool,
}

struct Lang {
    keywords: &'static [&'static str],
    line_comment: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    backtick_strings: bool,
}

const RUST: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while", "Some", "None", "Ok", "Err",
];
const JS: &[&str] = &[
    "async", "await", "break", "case", "catch", "class", "const", "continue", "default", "delete",
    "do", "else", "export", "extends", "false", "finally", "for", "from", "function", "if",
    "import", "in", "instanceof", "interface", "let", "new", "null", "of", "return", "static",
    "super", "switch", "this", "throw", "true", "try", "type", "typeof", "undefined", "var",
    "void", "while", "yield", "enum", "implements", "private", "public", "readonly",
];
const PYTHON: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "False", "finally", "for", "from", "global", "if", "import", "in", "is",
    "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "self", "True", "try",
    "while", "with", "yield",
];
const GO: &[&str] = &[
    "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough", "for",
    "func", "go", "goto", "if", "import", "interface", "map", "package", "range", "return",
    "select", "struct", "switch", "type", "var", "nil", "true", "false",
];
const C_LIKE: &[&str] = &[
    "auto", "break", "case", "catch", "class", "const", "continue", "default", "delete", "do",
    "else", "enum", "extern", "false", "final", "for", "if", "import", "include", "namespace",
    "new", "null", "nullptr", "package", "private", "protected", "public", "return", "static",
    "struct", "switch", "template", "this", "throw", "true", "try", "typedef", "using", "var",
    "virtual", "void", "while", "override", "async", "await",
];
const SHELL: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "case", "esac",
    "function", "return", "export", "local", "echo", "cd", "sudo", "param", "foreach", "try",
    "catch", "throw",
];
const DATA: &[&str] = &["true", "false", "null", "yes", "no"];
const SQL: &[&str] = &[
    "select", "from", "where", "insert", "into", "values", "update", "set", "delete", "create",
    "table", "join", "left", "right", "inner", "on", "group", "by", "order", "limit", "and", "or",
    "not", "null", "as", "primary", "key", "index",
];

fn lang(name: &str) -> Lang {
    let name = name.trim().to_ascii_lowercase();
    match name.as_str() {
        "rust" | "rs" => Lang { keywords: RUST, line_comment: &["//"], block_comment: Some(("/*", "*/")), backtick_strings: false },
        "js" | "jsx" | "ts" | "tsx" | "javascript" | "typescript" | "mjs" | "cjs" | "qml" => {
            Lang { keywords: JS, line_comment: &["//"], block_comment: Some(("/*", "*/")), backtick_strings: true }
        }
        "py" | "python" => Lang { keywords: PYTHON, line_comment: &["#"], block_comment: None, backtick_strings: false },
        "go" | "golang" => Lang { keywords: GO, line_comment: &["//"], block_comment: Some(("/*", "*/")), backtick_strings: true },
        "c" | "h" | "cpp" | "c++" | "hpp" | "cc" | "java" | "kotlin" | "kt" | "cs" | "csharp" | "swift" | "dart" => {
            Lang { keywords: C_LIKE, line_comment: &["//"], block_comment: Some(("/*", "*/")), backtick_strings: false }
        }
        "sh" | "bash" | "zsh" | "shell" | "console" | "ps1" | "powershell" | "pwsh" => {
            Lang { keywords: SHELL, line_comment: &["#"], block_comment: None, backtick_strings: false }
        }
        "json" | "jsonc" | "toml" | "yaml" | "yml" | "ini" => {
            Lang { keywords: DATA, line_comment: &["#", "//"], block_comment: None, backtick_strings: false }
        }
        "sql" => Lang { keywords: SQL, line_comment: &["--"], block_comment: Some(("/*", "*/")), backtick_strings: false },
        _ => Lang { keywords: &[], line_comment: &["//", "#"], block_comment: Some(("/*", "*/")), backtick_strings: false },
    }
}

/// Colour one line of `language` code. `state` must be reused for the lines of one block.
pub fn line(text: &str, language: &str, state: &mut State) -> String {
    let lang = lang(language);
    let case_insensitive = language.eq_ignore_ascii_case("sql");
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() * 2);
    let mut i = 0;
    let mut current: &str = "";
    let paint = |out: &mut String, style: &'static str, piece: &str, current: &mut &'static str| {
        if *current != style {
            out.push_str(RESET);
            out.push_str(style);
            *current = style;
        }
        out.push_str(piece);
    };
    let starts = |at: usize, needle: &str| -> bool {
        let n: Vec<char> = needle.chars().collect();
        chars.len() >= at + n.len() && chars[at..at + n.len()] == n[..]
    };

    while i < chars.len() {
        if state.in_block_comment {
            let (_, close) = lang.block_comment.unwrap_or(("/*", "*/"));
            let mut end = i;
            while end < chars.len() && !starts(end, close) {
                end += 1;
            }
            let stop = (end + close.chars().count()).min(chars.len());
            if end < chars.len() {
                state.in_block_comment = false;
            }
            paint(&mut out, COMMENT, &chars[i..stop].iter().collect::<String>(), &mut current);
            i = stop;
            continue;
        }
        if let Some((open, _)) = lang.block_comment {
            if starts(i, open) {
                state.in_block_comment = true;
                paint(&mut out, COMMENT, open, &mut current);
                i += open.chars().count();
                continue;
            }
        }
        if lang.line_comment.iter().any(|c| starts(i, c)) {
            // `#` only starts a comment at a word boundary (keeps `#[derive]` and `a#b` intact).
            let hash = chars[i] == '#';
            let attribute = hash && chars.get(i + 1).map(|c| *c == '[' || *c == '!').unwrap_or(false);
            if !attribute && (!hash || i == 0 || chars[i - 1].is_whitespace()) {
                paint(&mut out, COMMENT, &chars[i..].iter().collect::<String>(), &mut current);
                break;
            }
        }
        let c = chars[i];
        if c == '"' || c == '\'' || (c == '`' && lang.backtick_strings) {
            // A lone apostrophe in Rust is a lifetime, not a string.
            if c == '\'' && language.eq_ignore_ascii_case("rust") {
                let closes = chars.get(i + 2) == Some(&'\'') || (chars.get(i + 1) == Some(&'\\') && chars.get(i + 3) == Some(&'\''));
                if !closes {
                    paint(&mut out, KEYWORD, "'", &mut current);
                    i += 1;
                    continue;
                }
            }
            let mut end = i + 1;
            while end < chars.len() && chars[end] != c {
                if chars[end] == '\\' {
                    end += 1;
                }
                end += 1;
            }
            let stop = (end + 1).min(chars.len());
            paint(&mut out, STRING, &chars[i..stop].iter().collect::<String>(), &mut current);
            i = stop;
            continue;
        }
        if c.is_ascii_digit() && (i == 0 || !is_word(chars[i - 1])) {
            let mut end = i;
            while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '.' || chars[end] == '_') {
                end += 1;
            }
            paint(&mut out, NUMBER, &chars[i..end].iter().collect::<String>(), &mut current);
            i = end;
            continue;
        }
        if is_word(c) {
            let mut end = i;
            while end < chars.len() && is_word(chars[end]) {
                end += 1;
            }
            let word: String = chars[i..end].iter().collect();
            let next = chars[end..].iter().find(|c| !c.is_whitespace()).copied();
            let keyword = if case_insensitive {
                lang.keywords.iter().any(|k| k.eq_ignore_ascii_case(&word))
            } else {
                lang.keywords.contains(&word.as_str())
            };
            let style = if keyword {
                KEYWORD
            } else if next == Some('(') || next == Some('!') && language.eq_ignore_ascii_case("rust") {
                CALL
            } else if word.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false) {
                TYPE
            } else {
                PLAIN
            };
            paint(&mut out, style, &word, &mut current);
            i = end;
            continue;
        }
        let piece = c.to_string();
        paint(&mut out, PLAIN, &piece, &mut current);
        i += 1;
    }
    out.push_str(RESET);
    out
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn keeps_the_text_intact() {
        let mut state = State::default();
        for (code, lang) in [
            ("fn main() { println!(\"hi {}\", 1); } // done", "rust"),
            ("const x = `a ${b}` + 'c'; /* note */", "ts"),
            ("def f(x): return x # comment", "python"),
            ("#[derive(Debug)] struct A<'a>(&'a str);", "rust"),
        ] {
            assert_eq!(strip(&line(code, lang, &mut state)), code);
        }
    }

    #[test]
    fn colours_keywords_strings_and_comments() {
        let mut state = State::default();
        let out = line("let s = \"x\"; // c", "rust", &mut state);
        assert!(out.contains(&format!("{KEYWORD}let")));
        assert!(out.contains(&format!("{STRING}\"x\"")));
        assert!(out.contains(&format!("{COMMENT}// c")));
    }

    #[test]
    fn block_comments_span_lines() {
        let mut state = State::default();
        line("/* start", "c", &mut state);
        assert!(state.in_block_comment);
        let out = line("still */ int x;", "c", &mut state);
        assert!(!state.in_block_comment);
        assert!(out.starts_with(&format!("{RESET}{COMMENT}still */")));
    }
}
