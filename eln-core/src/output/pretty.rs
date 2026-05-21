/// 사람용 섹션 구분선
pub fn separator() -> &'static str {
    "──────────────────────────────────────"
}

/// key: value 형식 출력 헬퍼
pub fn field(key: &str, value: &str) {
    println!("  {key:<12} {value}");
}

/// 선택적 field (None이면 생략)
pub fn optional_field(key: &str, value: Option<&str>) {
    if let Some(v) = value {
        field(key, v);
    }
}
