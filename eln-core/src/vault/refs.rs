use regex::Regex;
use std::sync::LazyLock;

// 나열 항목마다 `N0050 (주석)` 형태의 괄호 주석 허용 — 실제 vault 관용구.
static SEE_REF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"→ see\s+(N\d{4,}(?:\s*\([^)]*\))?(?:\s*,\s*N\d{4,}(?:\s*\([^)]*\))?)*)").unwrap()
});
static ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"N\d{4,}").unwrap());
static PAREN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^)]*\)").unwrap());

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
                    // 주석 괄호 안의 ID는 나열 항목이 아니므로 제거 후 추출
                    let span = PAREN_RE.replace_all(&cap[1], "");
                    for id in ID_RE.find_iter(&span) {
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
    fn scan_inline_refs_extracts_annotated_lists_ignoring_ids_inside_annotations() {
        let refs = scan_inline_refs(
            "관련 → see N0050 (비강제 — 반대쪽 반), N0128 (break-safe, discipline 아님), \
             N0072 (형제), N0122 (N0057 감사 렌즈), N0132 (layer-invariant)",
        );

        assert_eq!(refs, vec!["N0050", "N0128", "N0072", "N0122", "N0132"]);
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
