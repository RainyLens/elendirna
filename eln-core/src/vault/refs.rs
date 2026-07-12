use regex::Regex;
use std::sync::LazyLock;

static SEE_REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"→ see\s+(N\d{4,}(?:\s*,\s*N\d{4,})*)").unwrap());
static ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"N\d{4,}").unwrap());

pub fn see_ref_re() -> &'static Regex {
    &SEE_REF_RE
}

/// Collect `→ see N####` references from markdown text while ignoring fenced
/// code blocks, inline code, and block quotes.
pub fn scan_inline_refs(content: &str) -> Vec<String> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};

    let mut refs = Vec::new();
    let mut in_code_block = false;
    let mut in_blockquote = 0u32;

    for event in Parser::new(content) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::Start(Tag::BlockQuote) => in_blockquote += 1,
            Event::End(TagEnd::BlockQuote) => {
                in_blockquote = in_blockquote.saturating_sub(1);
            }
            Event::Code(_) => {}
            Event::Text(text) if !in_code_block && in_blockquote == 0 => {
                for cap in see_ref_re().captures_iter(&text) {
                    for id in ID_RE.find_iter(&cap[1]) {
                        refs.push(id.as_str().to_string());
                    }
                }
            }
            _ => {}
        }
    }
    refs
}

#[cfg(test)]
mod tests {
    use super::scan_inline_refs;

    #[test]
    fn scan_inline_refs_extracts_four_or_more_digit_ids() {
        let refs = scan_inline_refs("plain → see N0001 and rollover → see N10000");

        assert_eq!(refs, vec!["N0001", "N10000"]);
    }

    #[test]
    fn scan_inline_refs_extracts_comma_separated_lists() {
        let refs = scan_inline_refs("연결 → see N0127, N0129. 그리고 → see N0001");

        assert_eq!(refs, vec!["N0127", "N0129", "N0001"]);
    }

    #[test]
    fn scan_inline_refs_ignores_code_and_blockquote_refs() {
        let content = r#"
plain → see N0001

```
→ see N0002
```

inline `→ see N0003`

> quoted → see N0004
"#;

        assert_eq!(scan_inline_refs(content), vec!["N0001"]);
    }
}
