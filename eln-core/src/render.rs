//! 서버사이드 마크다운 렌더 ([[N0106]] r0003).
//!
//! 휴먼 뷰어의 entry note / revision delta를 HTML로 렌더하면서, vault 고유의
//! cross-ref 패턴(`[[N####]]`, `→ see N####`)을 `<a href="#/entry/N####">` 링크로
//! 바꾼다. 링크화는 **코드 블록·인라인 코드 바깥**에서만 일어난다(코드 안의 `[[N####]]`는
//! 리터럴로 보존). `known_ids`에 없는 참조는 dangling으로 표기하고 별도로 수집한다.
//!
//! FE 전용이 아니라 vault 도메인 로직 — CLI/export 표면에서도 재사용 가능하도록
//! `http_backend`가 아닌 crate 본체에 둔다.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// `[[N####]]` 또는 `→ see N####` 형태의 cross-ref를 잡는다.
/// 그룹 1 = `[[ ]]` 형태의 id, 그룹 2 = 화살표 prefix("→ see "), 그룹 3 = 화살표 형태의 id.
static REF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[\[(N\d{4,})\]\]|(→\s*see\s+)(N\d{4,})").expect("ref regex compiles")
});

/// 렌더 결과 — HTML 본문 + 발견된 dangling 참조 id(중복 제거, 등장 순서).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RenderedMd {
    pub html: String,
    pub dangling: Vec<String>,
}

fn parse_options() -> Options {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts
}

/// markdown 본문을 HTML로 렌더하고 cross-ref를 링크화한다.
///
/// `known_ids`: vault에 실재하는 entry id 집합. 여기 없는 참조는 `class="ref dangling"`로
/// 표기되고 `RenderedMd.dangling`에 수집된다.
///
/// 구현: pulldown-cmark가 `[[N####]]`의 대괄호를 link 파싱하며 Text 이벤트를 쪼개므로,
/// event 스트림 위에서 매칭하지 않는다. 대신 (1) offset iterator로 **코드 영역의 byte
/// range**를 모으고, (2) 원본 소스에서 코드 바깥의 ref만 anchor HTML로 치환한 새 소스를
/// 만든 뒤, (3) 그 소스를 파싱해 렌더한다(주입한 `<a>`는 raw inline HTML로 그대로 통과).
pub fn render_markdown(src: &str, known_ids: &HashSet<String>) -> RenderedMd {
    let opts = parse_options();

    // 1) 코드 영역(코드 블록 + 인라인 코드) byte range 수집.
    let mut code_ranges: Vec<std::ops::Range<usize>> = Vec::new();
    let mut cb_start: Option<usize> = None;
    for (event, range) in Parser::new_ext(src, opts).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => cb_start = Some(range.start),
            Event::End(TagEnd::CodeBlock) => {
                let start = cb_start.take().unwrap_or(range.start);
                code_ranges.push(start..range.end);
            }
            Event::Code(_) => code_ranges.push(range),
            _ => {}
        }
    }
    let in_code = |pos: usize| code_ranges.iter().any(|r| r.contains(&pos));

    // 2) 코드 바깥 ref를 anchor HTML로 치환하며 새 소스를 빌드 + dangling 수집.
    let mut dangling: Vec<String> = Vec::new();
    let mut seen_dangling: HashSet<String> = HashSet::new();
    let mut new_src = String::with_capacity(src.len() + 64);
    let mut last = 0usize;
    for caps in REF_RE.captures_iter(src) {
        let m = caps.get(0).unwrap();
        if in_code(m.start()) {
            // 코드 내부 ref는 건드리지 않는다 — last를 진전시키지 않아 다음 복사 때 원문 보존.
            continue;
        }
        new_src.push_str(&src[last..m.start()]);
        if let Some(id) = caps.get(1) {
            // `[[N####]]` → 전체를 anchor로 치환.
            new_src.push_str(&anchor(id.as_str(), known_ids, &mut dangling, &mut seen_dangling));
        } else {
            // `→ see N####` → prefix는 보존, id만 anchor로.
            new_src.push_str(caps.get(2).map(|p| p.as_str()).unwrap_or(""));
            let id = caps.get(3).unwrap().as_str();
            new_src.push_str(&anchor(id, known_ids, &mut dangling, &mut seen_dangling));
        }
        last = m.end();
    }
    new_src.push_str(&src[last..]);

    // 3) 치환된 소스를 렌더 (주입 anchor는 raw inline HTML로 통과).
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(&new_src, opts));
    RenderedMd {
        html: out,
        dangling,
    }
}

fn anchor(
    id: &str,
    known_ids: &HashSet<String>,
    dangling: &mut Vec<String>,
    seen_dangling: &mut HashSet<String>,
) -> String {
    let is_known = known_ids.contains(id);
    if !is_known && seen_dangling.insert(id.to_string()) {
        dangling.push(id.to_string());
    }
    let class = if is_known { "ref" } else { "ref dangling" };
    // id는 `N\d{4,}` 형태라 HTML/속성 escape가 불필요하다.
    format!(r##"<a href="#/entry/{id}" class="{class}">{id}</a>"##)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> HashSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn renders_plain_markdown() {
        let r = render_markdown("# 제목\n\n본문 **굵게**.", &ids(&[]));
        assert!(r.html.contains("<h1>"));
        assert!(r.html.contains("<strong>굵게</strong>"));
        assert!(r.dangling.is_empty());
    }

    #[test]
    fn linkifies_wiki_ref_known() {
        let r = render_markdown("관련 [[N0033]] 참고.", &ids(&["N0033"]));
        assert!(
            r.html.contains(r##"<a href="#/entry/N0033" class="ref">N0033</a>"##),
            "html={}",
            r.html
        );
        assert!(r.dangling.is_empty());
    }

    #[test]
    fn linkifies_arrow_ref_and_keeps_prefix() {
        let r = render_markdown("→ see N0106 에서 이어짐", &ids(&["N0106"]));
        assert!(r.html.contains("→ see "), "prefix 보존: {}", r.html);
        assert!(
            r.html.contains(r##"<a href="#/entry/N0106" class="ref">N0106</a>"##),
            "html={}",
            r.html
        );
    }

    #[test]
    fn flags_dangling_ref() {
        let r = render_markdown("[[N9999]] 는 없음", &ids(&["N0001"]));
        assert!(
            r.html.contains(r#"class="ref dangling""#),
            "html={}",
            r.html
        );
        assert_eq!(r.dangling, vec!["N9999".to_string()]);
    }

    #[test]
    fn does_not_linkify_inside_code_block() {
        let src = "```\n[[N0033]]\n```";
        let r = render_markdown(src, &ids(&["N0033"]));
        assert!(!r.html.contains("href=\"#/entry/N0033\""), "html={}", r.html);
        assert!(r.dangling.is_empty());
    }

    #[test]
    fn dedupes_dangling() {
        let r = render_markdown("[[N9999]] 그리고 다시 [[N9999]]", &ids(&[]));
        assert_eq!(r.dangling, vec!["N9999".to_string()]);
    }
}
