//! Reading a server's own shell history, so command suggestions are useful
//! on the first connect instead of starting from nothing. Each parser takes
//! the tail of the file (the start may be cut mid-entry) and returns
//! commands oldest first. Multi-line entries are skipped: a suggestion is
//! one line.

/// Longest command worth suggesting; anything longer is pasted data
pub const MAX_COMMAND_LEN: usize = 1000;

fn keep(cmd: &str) -> Option<String> {
    let cmd = cmd.trim_end();
    // A leading space hides a command from history in bash and zsh; respect it
    if cmd.is_empty() || cmd.starts_with(' ') || cmd.len() > MAX_COMMAND_LEN || cmd.contains(['\n', '\r']) {
        return None;
    }
    Some(cmd.to_string())
}

/// `~/.bash_history`: one command per line, with `#1700000000` timestamp
/// lines when HISTTIMEFORMAT is set.
pub fn parse_bash(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !(l.starts_with('#') && l[1..].chars().all(|c| c.is_ascii_digit()) && l.len() > 1))
        .filter_map(keep)
        .collect()
}

/// `~/.zsh_history`: plain lines, or `: 1700000000:0;command` with
/// EXTENDED_HISTORY. A line ending in `\` continues on the next one.
pub fn parse_zsh(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let mut entry = line.to_string();
        let mut multi = false;
        while entry.ends_with('\\') {
            multi = true;
            match lines.next() {
                Some(next) => {
                    entry.pop();
                    entry.push('\n');
                    entry.push_str(next);
                }
                None => break,
            }
        }
        if multi {
            continue;
        }
        let cmd = match entry.strip_prefix(": ") {
            Some(rest) => match rest.split_once(';') {
                Some((meta, cmd)) if meta.contains(':') => cmd,
                _ => entry.as_str(),
            },
            None => entry.as_str(),
        };
        if let Some(c) = keep(cmd) {
            out.push(c);
        }
    }
    out
}

/// fish's `fish_history`: YAML-like `- cmd: …` entries followed by
/// indented `when:` / `paths:` lines. Inside `cmd`, `\n` is a newline and
/// `\\` a backslash.
pub fn parse_fish(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("- cmd: "))
        .filter(|raw| !has_escaped_newline(raw))
        .filter_map(|raw| keep(&unescape_fish(raw)))
        .collect()
}

fn has_escaped_newline(raw: &str) -> bool {
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if chars.next() == Some('n') {
                return true;
            }
        }
    }
    false
}

fn unescape_fish(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The tail of a file starts wherever the cut fell: drop that first,
/// probably partial, line.
pub fn drop_partial_first_line(text: &str, was_cut: bool) -> &str {
    if !was_cut {
        return text;
    }
    match text.find('\n') {
        Some(i) => &text[i + 1..],
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_skips_timestamps_hidden_and_long_lines() {
        let text = "#1700000000\nls -la\n cd secret\ngit status\n#not a stamp\n";
        assert_eq!(parse_bash(text), ["ls -la", "git status", "#not a stamp"]);
        assert!(parse_bash(&"x".repeat(MAX_COMMAND_LEN + 1)).is_empty());
    }

    #[test]
    fn zsh_reads_extended_entries_and_skips_continued_ones() {
        let text = ": 1700000000:0;git pull\nplain cmd\n: 1700000001:0;for x in a b; do\\\necho $x\\\ndone\n: 1700000002:3;make build\n";
        assert_eq!(parse_zsh(text), ["git pull", "plain cmd", "make build"]);
    }

    #[test]
    fn fish_unescapes_and_skips_multiline() {
        let text = "- cmd: git status\n  when: 1700000000\n- cmd: echo a\\\\b\n  when: 1700000001\n  paths:\n    - a\n- cmd: begin\\n  echo x\\nend\n  when: 1700000002\n- cmd: docker compose up -d\n  when: 1700000003\n";
        assert_eq!(parse_fish(text), ["git status", "echo a\\b", "docker compose up -d"]);
    }

    #[test]
    fn a_cut_file_loses_its_first_line_only_when_cut() {
        assert_eq!(drop_partial_first_line("tatus\nls\n", true), "ls\n");
        assert_eq!(drop_partial_first_line("git status\nls\n", false), "git status\nls\n");
    }
}
