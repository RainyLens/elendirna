---
id: "N0001"
title: "OAuth2 Login Integration"
baseline: null
tags: []
---
# OAuth2 Login Integration

## 배경

기존 세션 기반 인증을 OAuth2로 전환하는 과정에서 정리한 내용.
소셜 로그인(Google, GitHub)과 자체 OAuth2 서버 모두 커버.

## 핵심 흐름

1. Authorization Code Grant 흐름 사용 (Implicit Grant는 deprecated)
2. `state` 파라미터로 CSRF 방지 — UUID v4 생성 후 세션에 저장
3. `redirect_uri`는 서버에 등록된 값과 정확히 일치해야 함 (쿼리스트링 포함)
4. Access Token은 메모리에만 보관, Refresh Token은 HttpOnly 쿠키

## 주의사항

- `code` 교환은 서버사이드에서만 수행 (client_secret 노출 방지)
- Token 응답에 `expires_in` 없으면 3600초로 fallback 처리
- PKCE 적용 시 code_verifier는 43~128자 랜덤 문자열 → see N0004

## 참고

→ see N0002 (보안 가이드라인과 연계)
