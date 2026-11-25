pub fn validate_url(input: &str) -> Result<String, &'static str> {
    if input.len() > 8192 {
        return Err("Keep the URL within 8,192 UTF-8 bytes.");
    }
    if input.chars().any(char::is_control) {
        return Err("Remove control characters from the URL.");
    }
    let value = input.trim();
    if value.chars().any(char::is_whitespace) || value.contains('\\') {
        return Err("Remove spaces and backslashes from inside the URL.");
    }
    let parsed = url::Url::parse(value).map_err(|_| "Use an absolute http:// or https:// URL.")?;
    if !matches!(parsed.scheme(), "http" | "https") || !parsed.has_host() {
        return Err("Use an absolute http:// or https:// URL with a host.");
    }
    // The URL parser repairs missing slashes; require an explicit nonempty authority.
    let authority = value
        .split_once("://")
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case(parsed.scheme()))
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
        .filter(|authority| !authority.is_empty())
        .ok_or("Include http:// or https:// followed by a host.")?;
    if authority.contains('@') || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Remove usernames and passwords from the URL.");
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_https_and_preserves_exact_trimmed_text() {
        for input in [
            "https://Example.com/a?b=2&a=1#Hello",
            "http://localhost:8080/",
            "https://[::1]/",
            "https://example.com/%20?q=%0A",
        ] {
            assert_eq!(validate_url(&format!("  {input}  ")).unwrap(), input);
        }
    }

    #[test]
    fn rejects_unsafe_or_ambiguous_urls() {
        for input in [
            "",
            "example.com",
            "//example.com",
            "javascript:alert(1)",
            "file:///C:/secret",
            "data:text/plain,hello",
            "ftp://example.com",
            "https://",
            "http:example.com",
            "http:example.com/?next=https://example.org/",
            "https:/example.com",
            "https:///example.com",
            "https://user:pass@example.com",
            "https://@example.com",
            "https://example.com:99999/",
            "https://example.com/a b",
            "https://example.com/a\nb",
            "\nhttps://example.com",
            "https://example.com\\other",
            "https://example.com/\u{7f}",
        ] {
            assert!(validate_url(input).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn limit_is_utf8_bytes_and_checked_before_trimming() {
        let max = format!("https://example.com/{}", "a".repeat(8192 - 20));
        assert_eq!(max.len(), 8192);
        assert!(validate_url(&max).is_ok());
        assert!(validate_url(&(max + "a")).is_err());
        assert!(validate_url(&format!("https://example.com/{}", "é".repeat(4096))).is_err());
    }
}
