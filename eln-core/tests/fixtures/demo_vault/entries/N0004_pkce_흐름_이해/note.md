---
id: "N0004"
title: "PKCE 흐름 이해"
baseline: null
tags: []
---
# PKCE 흐름 이해

## PKCE란?

Proof Key for Code Exchange. 모바일/SPA처럼 client_secret을 안전하게 보관할 수 없는 환경에서 Authorization Code 탈취를 방어.

RFC 7636 표준.

## 흐름 요약

```
1. 클라이언트: code_verifier 생성 (43~128자 랜덤 문자열)
2. 클라이언트: code_challenge = BASE64URL(SHA256(code_verifier))
3. 인가 요청: code_challenge + code_challenge_method=S256 포함
4. 인가 서버: code와 함께 code_challenge 저장
5. 토큰 요청: code + code_verifier 전송
6. 인가 서버: SHA256(code_verifier) == code_challenge 검증 후 토큰 발급
```

## 왜 안전한가?

code_challenge는 공개되어도 됨 (단방향 해시). 공격자가 code를 탈취해도 code_verifier 없이는 토큰 교환 불가.

## 구현 시 주의

- `plain` method는 사용 금지 (code_verifier == code_challenge라 의미 없음)
- code_verifier는 매 요청마다 새로 생성 (재사용 금지)
- 서버에서 PKCE 없는 요청을 거부하도록 강제 설정 필요

→ see N0001 (OAuth2 전체 흐름에서 PKCE 위치)
