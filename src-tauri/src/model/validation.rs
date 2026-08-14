//! Lightweight format checks (port of legacy `utils/validation.py`).
#![allow(dead_code)]

/// Simple e-mail check (not RFC-complete). Legacy: `^[^\s@]+@[^\s@]+\.[^\s@]+$`.
pub fn is_valid_email(value: &str) -> bool {
    let candidate = value.trim();
    if candidate.is_empty() || candidate.contains(' ') {
        return false;
    }
    let Some((local, rest)) = candidate.split_once('@') else {
        return false;
    };
    if local.is_empty() || rest.contains('@') {
        return false;
    }
    if local.chars().any(char::is_whitespace) || rest.chars().any(char::is_whitespace) {
        return false;
    }
    let Some(dot) = rest.find('.') else {
        return false;
    };
    let domain = &rest[..dot];
    let after_dot = &rest[dot + 1..];
    !domain.is_empty() && !after_dot.is_empty()
}

/// True if the string looks like an HTTP(S) URL (prefix only, case-sensitive).
pub fn is_valid_share_link(value: &str) -> bool {
    let candidate = value.trim();
    candidate.starts_with("http://") || candidate.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_accepts_simple_addresses() {
        assert!(is_valid_email("user@example.com"));
        assert!(is_valid_email("a@b.co"));
        assert!(is_valid_email("  max@example.de  "));
        assert!(is_valid_email("gsr.andy@hotmail.de"));
    }

    #[test]
    fn email_rejects_invalid() {
        assert!(!is_valid_email(""));
        assert!(!is_valid_email("invalid"));
        assert!(!is_valid_email("a@b"));
        assert!(!is_valid_email("a @b.de"));
        assert!(!is_valid_email("a@b@c.de"));
        assert!(!is_valid_email("@b.de"));
        assert!(!is_valid_email("a@.de"));
        assert!(!is_valid_email("a@b."));
    }

    #[test]
    fn share_link_requires_http_or_https() {
        assert!(is_valid_share_link("https://example.com/x"));
        assert!(is_valid_share_link("http://example.com"));
        assert!(is_valid_share_link("  https://dropbox.com/s/abc  "));
        assert!(!is_valid_share_link("ftp://example.com"));
        assert!(!is_valid_share_link(""));
        assert!(!is_valid_share_link("HTTPS://example.com"));
        assert!(!is_valid_share_link("dropbox.com/s/abc"));
    }
}
