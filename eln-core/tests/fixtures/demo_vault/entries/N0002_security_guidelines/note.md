---
id: "N0002"
title: "Security Guidelines"
baseline: null
tags: []
---
# Security Guidelines

## 인증/인가 기본 원칙

1. **최소 권한 원칙**: 토큰 scope는 필요한 것만 요청
2. **전송 암호화**: HTTPS 강제, HSTS 헤더 설정
3. **입력 검증**: redirect_uri whitelisting, state 검증 누락 금지

## 토큰 보안

- Access Token 수명: 최대 1시간
- Refresh Token 수명: 최대 30일, rotation 필수
- JWT 사용 시 alg 필드 검증 필수 (`alg: none` 공격 방어) → see N0003

## 저장 정책

| 토큰 유형 | 저장 위치 | 이유 |
|-----------|-----------|------|
| Access Token | 메모리 (JS 변수) | XSS 노출 최소화 |
| Refresh Token | HttpOnly Cookie | JS 접근 차단 |
| Session ID | HttpOnly Cookie | JS 접근 차단 |

## 로깅 금지 항목

- 토큰 값 전체
- 비밀번호, secret
- 개인식별정보(이름, 이메일 원문)

→ see N0001 (OAuth2 구현과 연계)
