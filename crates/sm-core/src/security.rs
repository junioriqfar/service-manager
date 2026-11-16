/// Redaksi nilai sensitif sebelum ditulis ke log.
///
/// Tidak dimaksudkan sebagai parser sempurna; fokus pada pola yang sering
/// muncul pada output Git/tool: token provider, Bearer, password=, dan
/// userinfo pada URL.
pub fn redact_secrets(input: &str) -> String {
    let mut text = redact_known_prefixes(input);
    text = redact_bearer(&text);
    text = redact_key_values(&text);
    redact_url_userinfo(&text)
}

fn is_token_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, '_' | '-' | '.')
}

fn redact_known_prefixes(input: &str) -> String {
    const PREFIXES: [&str; 12] = [
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "sk-",
        "AKIA",
        "ASIA",
    ];
    let mut output = String::with_capacity(input.len());
    let mut index = 0usize;

    while index < input.len() {
        let rest = &input[index..];
        let mut matched = None;
        for prefix in PREFIXES {
            if rest.len() >= prefix.len() && rest[..prefix.len()].eq_ignore_ascii_case(prefix) {
                matched = Some(prefix.len());
                break;
            }
        }

        if let Some(prefix_len) = matched {
            let mut end = index + prefix_len;
            let mut token_len = 0usize;
            for character in input[end..].chars() {
                if !is_token_char(character) {
                    break;
                }
                token_len += 1;
                end += character.len_utf8();
            }
            if token_len >= 8 {
                output.push_str(&input[index..index + prefix_len]);
                output.push_str("[REDACTED]");
                index = end;
                continue;
            }
        }

        let character = rest.chars().next().unwrap();
        output.push(character);
        index += character.len_utf8();
    }

    output
}

fn redact_bearer(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut index = 0usize;

    while let Some(relative) = lower[index..].find("bearer ") {
        let start = index + relative;
        output.push_str(&input[index..start]);
        let value_start = start + "bearer ".len();
        let value_end = input[value_start..]
            .find(|character: char| character.is_whitespace())
            .map(|offset| value_start + offset)
            .unwrap_or(input.len());
        output.push_str(&input[start..value_start]);
        output.push_str("[REDACTED]");
        index = value_end;
    }
    output.push_str(&input[index..]);
    output
}

fn redact_key_values(input: &str) -> String {
    const KEYS: [&str; 7] = [
        "password=",
        "passwd=",
        "token=",
        "secret=",
        "api_key=",
        "apikey=",
        "access_token=",
    ];
    let lower = input.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut index = 0usize;

    while index < input.len() {
        let rest = &lower[index..];
        let mut matched = None;
        for key in KEYS {
            if rest.starts_with(key) {
                matched = Some(key);
                break;
            }
        }

        if let Some(key) = matched {
            let value_start = index + key.len();
            let value_end = input[value_start..]
                .find(|character: char| character.is_whitespace() || character == '&')
                .map(|offset| value_start + offset)
                .unwrap_or(input.len());
            output.push_str(&input[index..value_start]);
            if value_end > value_start {
                output.push_str("[REDACTED]");
            }
            index = value_end;
            continue;
        }

        let character = input[index..].chars().next().unwrap();
        output.push(character);
        index += character.len_utf8();
    }

    output
}

fn redact_url_userinfo(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0usize;

    while let Some(relative) = input[index..].find("://") {
        let scheme_end = index + relative + 3;
        output.push_str(&input[index..scheme_end]);

        let rest = &input[scheme_end..];
        let authority_end = rest
            .find(['/', '\\', ' ', '\n', '\r', '"', '\''])
            .map(|offset| scheme_end + offset)
            .unwrap_or(input.len());
        let authority = &input[scheme_end..authority_end];

        if let Some(at) = authority.rfind('@') {
            let userinfo = &authority[..at];
            if let Some(colon) = userinfo.find(':') {
                output.push_str(&userinfo[..colon + 1]);
                output.push_str("[REDACTED]");
                output.push_str(&authority[at..]);
                index = authority_end;
                continue;
            }
        }

        output.push_str(authority);
        index = authority_end;
    }

    output.push_str(&input[index..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_provider_tokens() {
        let text = "remote: https://ghp_abcdefghijklmnop12345@github.com/x";
        let redacted = redact_secrets(text);
        assert!(!redacted.contains("ghp_abcdefghijklmnop12345"));
        assert!(redacted.contains("ghp_[REDACTED]"));
    }

    #[test]
    fn redacts_bearer_tokens() {
        let redacted = redact_secrets("Authorization: Bearer abc.def.ghi");
        assert_eq!(redacted, "Authorization: Bearer [REDACTED]");
    }

    #[test]
    fn redacts_key_value_pairs() {
        let redacted = redact_secrets("token=secretvalue password=hunter2 user=admin");
        assert!(redacted.contains("token=[REDACTED]"));
        assert!(redacted.contains("password=[REDACTED]"));
        assert!(redacted.contains("user=admin"));
        assert!(!redacted.contains("secretvalue"));
        assert!(!redacted.contains("hunter2"));
    }

    #[test]
    fn redacts_url_userinfo() {
        let redacted = redact_secrets("https://oauth2:glpat-abcdefghij@gitlab.com/repo.git");
        assert!(!redacted.contains("glpat-abcdefghij"));
        assert!(redacted.contains("https://oauth2:[REDACTED]@gitlab.com/repo.git"));
    }

    #[test]
    fn leaves_normal_text_unchanged() {
        assert_eq!(
            redact_secrets("Service running; update selesai."),
            "Service running; update selesai."
        );
    }

    #[test]
    fn does_not_redact_short_prefixes() {
        assert_eq!(redact_secrets("ghp_short"), "ghp_short");
    }
}
