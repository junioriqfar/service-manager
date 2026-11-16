//! `${NAME}` expansion for admin-configured commands.
//!
//! Expansion is intentionally small and deterministic: only variables from
//! [`crate::registry::UpdaterSettings::variables`] are substituted, unknown
//! placeholders stay untouched, and nothing is read from the environment or
//! from project files.

use std::collections::BTreeMap;

/// Maximum recursive expansion depth. Keeps self-referencing variables from
/// recursing forever.
pub const MAX_EXPANSION_DEPTH: usize = 8;

/// Replace known `${NAME}` placeholders recursively.
///
/// - Names are case-sensitive.
/// - Unknown `${NAME}` placeholders are preserved verbatim.
/// - Values are expanded up to [`MAX_EXPANSION_DEPTH`] levels.
/// - Environment variables are never consulted.
pub fn expand_variables(value: &str, variables: &BTreeMap<String, String>) -> String {
    expand_inner(value, variables, 0)
}

fn expand_inner(value: &str, variables: &BTreeMap<String, String>, depth: usize) -> String {
    if depth >= MAX_EXPANSION_DEPTH || value.is_empty() {
        return value.to_string();
    }

    let mut output = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'$' && bytes.get(index + 1) == Some(&b'{') {
            if let Some(relative_end) = value[index + 2..].find('}') {
                let end = index + 2 + relative_end;
                let name = &value[index + 2..end];
                if let Some(replacement) = variables.get(name) {
                    output.push_str(&expand_inner(replacement, variables, depth + 1));
                } else {
                    output.push_str(&value[index..=end]);
                }
                index = end + 1;
                continue;
            }
        }

        let character = value[index..]
            .chars()
            .next()
            .expect("index always points at a valid character boundary");
        output.push(character);
        index += character.len_utf8();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variables(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn expands_php_and_php_home() {
        let vars = variables(&[
            ("PHP", r"C:\laragon\bin\php\php-8.3.16\php.exe"),
            ("PHP_HOME", r"C:\laragon\bin\php\php-8.3.16"),
        ]);
        assert_eq!(
            expand_variables(r#""${PHP}" artisan"#, &vars),
            r#""C:\laragon\bin\php\php-8.3.16\php.exe" artisan"#
        );
        assert_eq!(
            expand_variables(r#"${PHP_HOME}\composer.phar"#, &vars),
            r#"C:\laragon\bin\php\php-8.3.16\composer.phar"#
        );
    }

    #[test]
    fn expands_recursively_with_depth_limit() {
        let vars = variables(&[
            ("PHP", r"C:\php\php.exe"),
            ("PHP_HOME", r"C:\php"),
            ("TOOL", "${PHP}"),
            ("NESTED", "${TOOL} -v"),
            ("LOOP", "${LOOP}"),
        ]);
        assert_eq!(expand_variables("${NESTED}", &vars), r"C:\php\php.exe -v");
        assert_eq!(
            expand_variables("${LOOP}", &vars),
            "${LOOP}",
            "self reference stops at the depth limit"
        );
    }

    #[test]
    fn leaves_unknown_placeholders_and_environment_untouched() {
        let vars = variables(&[("PHP", r"C:\php\php.exe")]);
        assert_eq!(
            expand_variables(r#"${MISSING} %PATH% $PHP ${PHP"#, &vars),
            r#"${MISSING} %PATH% $PHP ${PHP"#
        );
    }

    #[test]
    fn expands_unicode_text_without_corrupting_it() {
        let vars = variables(&[("PHP", r"C:\php\php.exe")]);
        assert_eq!(
            expand_variables("harga → ${PHP} ✓", &vars),
            r"harga → C:\php\php.exe ✓"
        );
    }
}
