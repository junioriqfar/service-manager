//! Explicit per-app filtering of dirty working-tree entries.
//!
//! Generated artifacts (Vite/npm/Bun builds, Laravel Wayfinder output, and
//! similar) often stay untracked or modified after a build. The updater only
//! ignores an entry when it matches an admin-configured `dirtyIgnore` pattern;
//! every entry that does not match keeps the project `dirty`. There is no
//! hardcoded directory list and `.gitignore` is never modified.

/// Maximum number of patterns accepted per app.
pub const MAX_PATTERNS: usize = 32;
/// Maximum number of characters accepted per pattern.
pub const MAX_PATTERN_LENGTH: usize = 200;

/// Result of filtering `git status --porcelain` output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirtyScan {
    /// Entries that still block the update, as trimmed status lines.
    pub blocking: Vec<String>,
    /// Entries matched by an explicit `dirtyIgnore` pattern.
    pub ignored: Vec<String>,
}

impl DirtyScan {
    /// True when at least one entry is not covered by `dirtyIgnore`.
    pub fn is_blocking(&self) -> bool {
        !self.blocking.is_empty()
    }
}

/// Validate all `dirtyIgnore` patterns with clear, indexed errors.
pub fn validate_patterns(patterns: &[String]) -> Result<(), String> {
    if patterns.len() > MAX_PATTERNS {
        return Err(format!(
            "dirtyIgnore maksimal {MAX_PATTERNS} pola (ditemukan {}).",
            patterns.len()
        ));
    }
    for (index, pattern) in patterns.iter().enumerate() {
        parse_pattern(pattern).map_err(|error| format!("dirtyIgnore[{}]: {error}", index + 1))?;
    }
    Ok(())
}

/// Split a porcelain status output into blocking and explicitly ignored
/// entries. Invalid patterns are ignored defensively so they can never cause
/// an entry to be hidden; configuration validation reports them separately.
pub fn filter_status(output: &str, patterns: &[String]) -> DirtyScan {
    let patterns: Vec<PatternSpec> = patterns
        .iter()
        .filter_map(|pattern| parse_pattern(pattern).ok())
        .collect();

    let mut scan = DirtyScan::default();
    for raw in output.lines() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let entry = line.trim().to_string();
        let paths = entry_paths(line);
        let ignored = !patterns.is_empty()
            && paths
                .iter()
                .any(|path| patterns.iter().any(|pattern| pattern.matches(path)));
        if ignored {
            scan.ignored.push(entry);
        } else {
            scan.blocking.push(entry);
        }
    }
    scan
}

struct PatternSpec {
    segments: Vec<String>,
    literal: bool,
    dir_only: bool,
}

impl PatternSpec {
    fn matches(&self, path: &str) -> bool {
        let Some(path) = normalize_path(path) else {
            return false;
        };
        let path_segments: Vec<&str> = path.split('/').collect();

        if self.literal {
            let literal = self.segments.join("/");
            return path == literal
                || path
                    .strip_prefix(&literal)
                    .is_some_and(|rest| rest.starts_with('/'));
        }

        if self.dir_only {
            let mut segments: Vec<&str> = self.segments.iter().map(String::as_str).collect();
            segments.push("**");
            return match_segments(&segments, &path_segments);
        }

        let segments: Vec<&str> = self.segments.iter().map(String::as_str).collect();
        match_segments(&segments, &path_segments)
    }
}

fn parse_pattern(raw: &str) -> Result<PatternSpec, String> {
    if raw.chars().any(char::is_control) {
        return Err("mengandung karakter kontrol.".to_string());
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("pola kosong.".to_string());
    }
    if trimmed.chars().count() > MAX_PATTERN_LENGTH {
        return Err(format!(
            "terlalu panjang (maksimal {MAX_PATTERN_LENGTH} karakter)."
        ));
    }

    let mut text = trimmed.replace('\\', "/");
    while let Some(rest) = text.strip_prefix("./") {
        text = rest.to_string();
    }
    if text.starts_with('/') {
        return Err("harus relatif (tidak boleh diawali '/' atau '\\').".to_string());
    }
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("tidak boleh memakai drive letter (mis. C:/project).".to_string());
    }

    let dir_only = text.ends_with('/');
    while text.ends_with('/') {
        text.pop();
    }
    if text.is_empty() {
        return Err("pola direktori kosong.".to_string());
    }

    let mut segments = Vec::new();
    for segment in text.split('/') {
        match segment {
            "" => return Err("mengandung segmen kosong ('//'); gunakan satu '/'.".to_string()),
            "." => continue,
            ".." => return Err("tidak boleh mengandung segmen '..'.".to_string()),
            other => segments.push(other.to_lowercase()),
        }
    }
    if segments.is_empty() {
        return Err("pola tidak menunjuk path.".to_string());
    }

    let literal = segments
        .iter()
        .all(|segment| !segment.contains('*') && !segment.contains('?'));
    Ok(PatternSpec {
        segments,
        literal,
        dir_only,
    })
}

fn normalize_path(raw: &str) -> Option<String> {
    let mut text = raw.trim().replace('\\', "/");
    while let Some(rest) = text.strip_prefix("./") {
        text = rest.to_string();
    }
    if text.starts_with('/') {
        return None;
    }
    let text = text.trim_end_matches('/');
    if text.is_empty() {
        return None;
    }
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return None;
    }

    let mut segments = Vec::new();
    for segment in text.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return None,
            other => segments.push(other.to_lowercase()),
        }
    }
    if segments.is_empty() {
        None
    } else {
        Some(segments.join("/"))
    }
}

fn entry_paths(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    if bytes.len() < 4 || bytes[2] != b' ' {
        return Vec::new();
    }
    let raw = &line[3..];
    match raw.split_once(" -> ") {
        Some((old, new)) => vec![unquote(old), unquote(new)],
        None => vec![unquote(raw)],
    }
    .into_iter()
    .filter(|path| !path.trim().is_empty())
    .collect()
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return value.to_string();
    }
    let inner = &value[1..value.len() - 1];
    let mut output = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => output.push('\n'),
            Some('t') => output.push('\t'),
            Some('r') => output.push('\r'),
            Some('"') => output.push('"'),
            Some('\\') => output.push('\\'),
            Some(other) => output.push(other),
            None => output.push('\\'),
        }
    }
    output
}

fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    let mut previous = vec![false; path.len() + 1];
    previous[0] = true;

    for segment in pattern {
        let mut current = vec![false; path.len() + 1];
        if *segment == "**" {
            current[0] = previous[0];
            for position in 1..=path.len() {
                current[position] = previous[position] || current[position - 1];
            }
        } else {
            for position in 1..=path.len() {
                current[position] =
                    previous[position - 1] && match_segment(segment, path[position - 1]);
            }
        }
        previous = current;
    }

    previous[path.len()]
}

fn match_segment(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut pattern_index, mut text_index) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;

    while text_index < text.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == text[text_index])
        {
            pattern_index += 1;
            text_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star = Some((pattern_index, text_index));
            pattern_index += 1;
        } else if let Some((star_pattern, star_text)) = star {
            pattern_index = star_pattern + 1;
            text_index = star_text + 1;
            star = Some((star_pattern, star_text + 1));
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn without_patterns_every_entry_blocks() {
        let scan = filter_status(" M src/app.ts\n?? notes.txt\n", &[]);
        assert_eq!(scan.blocking, ["M src/app.ts", "?? notes.txt"]);
        assert!(scan.ignored.is_empty());
        assert!(scan.is_blocking());
    }

    #[test]
    fn all_entries_can_be_ignored() {
        let scan = filter_status(
            "?? public/build/app.js\n?? resources/js/actions/App.ts\n M resources/js/routes/index.ts\n",
            &patterns(&[
                "public/build/**",
                "resources/js/actions/**",
                "resources/js/routes/**",
            ]),
        );
        assert!(scan.blocking.is_empty());
        assert_eq!(scan.ignored.len(), 3);
        assert!(!scan.is_blocking());
    }

    #[test]
    fn mixed_entries_keep_only_unmatched_blocking() {
        let scan = filter_status(
            "?? public/build/app.js\n M src/app.ts\n",
            &patterns(&["public/build/**"]),
        );
        assert_eq!(scan.blocking, ["M src/app.ts"]);
        assert_eq!(scan.ignored, ["?? public/build/app.js"]);
    }

    #[test]
    fn literal_pattern_matches_exact_path_and_descendants() {
        assert!(
            filter_status("?? public/build", &patterns(&["public/build"]))
                .ignored
                .len()
                == 1
        );
        assert!(
            filter_status(
                "?? public/build/nested/app.js",
                &patterns(&["public/build"])
            )
            .ignored
            .len()
                == 1
        );
        let scan = filter_status("?? public/buildx/app.js", &patterns(&["public/build"]));
        assert!(scan.ignored.is_empty());
        assert_eq!(scan.blocking, ["?? public/buildx/app.js"]);
    }

    #[test]
    fn directory_suffix_matches_contents_of_wildcard_directory() {
        let scan = filter_status(
            "?? public/build/\n?? public/nested/app.js\n",
            &patterns(&["public/*/"]),
        );
        assert_eq!(
            scan.ignored,
            ["?? public/build/", "?? public/nested/app.js"]
        );
        assert!(scan.blocking.is_empty());
    }

    #[test]
    fn double_star_matches_across_segments() {
        let scan = filter_status(
            "?? public/app.js\n?? public/build/nested/app.js\n M src/app.js\n",
            &patterns(&["public/**/*.js"]),
        );
        assert_eq!(
            scan.ignored,
            ["?? public/app.js", "?? public/build/nested/app.js"]
        );
        assert_eq!(scan.blocking, ["M src/app.js"]);
    }

    #[test]
    fn single_star_and_question_mark_stay_within_one_segment() {
        let scan = filter_status(
            "?? public/app.js\n?? public/nested/app.js\n M src/a.ts\n M src/ab.ts\n",
            &patterns(&["public/*.js", "src/?.ts"]),
        );
        assert_eq!(scan.ignored, ["?? public/app.js", "M src/a.ts"]);
        assert_eq!(scan.blocking, ["?? public/nested/app.js", "M src/ab.ts"]);
    }

    #[test]
    fn rename_matches_either_side() {
        for pattern in ["src/old.ts", "src/new.ts"] {
            let scan = filter_status("R  src/old.ts -> src/new.ts\n", &patterns(&[pattern]));
            assert!(scan.blocking.is_empty(), "pattern {pattern}");
            assert_eq!(scan.ignored, ["R  src/old.ts -> src/new.ts"]);
        }
    }

    #[test]
    fn rename_with_unmatched_other_side_is_fully_ignored() {
        let scan = filter_status("R  src/old.ts -> docs/new.ts\n", &patterns(&["src/old.ts"]));
        assert!(scan.blocking.is_empty());
        assert_eq!(scan.ignored.len(), 1);
    }

    #[test]
    fn quoted_paths_with_spaces_are_unquoted() {
        let scan = filter_status(
            "?? \"public/build/with space/app.js\"\n M src/with space/app.ts\n",
            &patterns(&["public/build/**", "src/with space/app.ts"]),
        );
        assert!(scan.blocking.is_empty());
        assert_eq!(scan.ignored.len(), 2);
    }

    #[test]
    fn backslashes_and_case_are_normalized() {
        let scan = filter_status(
            "?? public/build/app.js\n M SRC/App.TS\n",
            &patterns(&["PUBLIC\\Build\\**", "src\\app.ts"]),
        );
        assert!(scan.blocking.is_empty());
        assert_eq!(scan.ignored.len(), 2);
    }

    #[test]
    fn invalid_patterns_never_hide_entries() {
        for pattern in [
            "",
            "src/\u{7}/app.ts",
            "/src/app.ts",
            "C:\\src\\app.ts",
            "../src/app.ts",
        ] {
            let scan = filter_status(" M src/app.ts\n", &[pattern.to_string()]);
            assert_eq!(scan.blocking, ["M src/app.ts"], "pattern {pattern:?}");
            assert!(scan.ignored.is_empty(), "pattern {pattern:?}");
        }
    }

    #[test]
    fn validate_patterns_reports_indexed_errors() {
        assert!(validate_patterns(&[]).is_ok());
        assert!(validate_patterns(&patterns(&["public/build/**"])).is_ok());

        let error = validate_patterns(&patterns(&["public/build/**", "/absolute"])).unwrap_err();
        assert!(error.contains("dirtyIgnore[2]"), "{error}");
        assert!(error.contains("relatif"), "{error}");

        let error = validate_patterns(&patterns(&["C:\\project\\**"])).unwrap_err();
        assert!(error.contains("drive letter"), "{error}");

        let error = validate_patterns(&patterns(&["../outside"])).unwrap_err();
        assert!(error.contains(".."), "{error}");

        let error = validate_patterns(&patterns(&[""])).unwrap_err();
        assert!(error.contains("kosong"), "{error}");

        let error = validate_patterns(&patterns(&["bad\npattern"])).unwrap_err();
        assert!(error.contains("kontrol"), "{error}");
    }

    #[test]
    fn validate_patterns_enforces_count_and_length_caps() {
        let too_many: Vec<String> = (0..MAX_PATTERNS + 1)
            .map(|index| format!("build/{index}/**"))
            .collect();
        let error = validate_patterns(&too_many).unwrap_err();
        assert!(error.contains("maksimal"), "{error}");

        let too_long = "a".repeat(MAX_PATTERN_LENGTH + 1);
        let error = validate_patterns(&[too_long]).unwrap_err();
        assert!(error.contains("terlalu panjang"), "{error}");
    }

    #[test]
    fn malformed_lines_and_empty_output_are_safe() {
        assert!(filter_status(" \n\r\n", &[]).blocking.is_empty());
        assert_eq!(
            filter_status("garbage\n", &[]).blocking,
            ["garbage".to_string()]
        );
    }
}
