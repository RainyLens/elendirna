# RAG comparison 30-question blind pilot report

작성일: 2026-08-04

## 판정 요약

`strong`과 `c3`의 30문항 blind paired pilot에서 수동 task success는
Strong 21/30(70.0%), C3 22/30(73.3%)였다. paired 결과는 C3 승 3,
Strong 승 2, 동률 25로 C3의 순증가는 한 문항뿐이다.

사전등록한 핵심군(`decision_rationale` + `superseded_avoidance` +
`handoff`)에서 C3는 11/15, Strong은 10/15로 +6.7%p였다. 요구한
+15%p를 충족하지 못했으므로 **revision-aware retrieval의 독립적인 제품
우위 주장은 기각한다(NO-GO)**.

이 결론은 Elendirna 자체의 폐기 결론이 아니다. 현재 증거가 지지하는
범위는 개인 authored vault, context governance layer, 또는 reference
implementation이다. 이 pilot은 C3를 기본 retrieval 제품 경로로 승격할
근거를 만들지 못했다.

## 동결 조건과 무결성

- frozen snapshot: 133 entries, 415 revisions, 548 chunks
- snapshot SHA-256:
  `69908d289e25c1d5add464a0847ab750d3846b0ecfeae7810199c463483d2b6f`
- graph SHA-256:
  `09abe0fd040c7773c1e453e23792670b6256d15d80e0286a02da18026b526804`
- questions SHA-256:
  `2dbe8f717d441a1f5bd0a2622eaf7ef6ef406ee55e70f7261029500ead613f92`
- pre-registration SHA-256:
  `151c5d51e567d61080a9cf255b1c86a2401c1eb1fcbd6417686790fd8fe1b34c`
- raw run SHA-256:
  `6b8e6bcbae82b789f072d8a6267b553cfea4b99bb6c48350c2cf240eb3f5fd65`
- automatic score SHA-256:
  `7f2bdf363bafeba082496841dc4bcc29bae18b59510caf524269df95390ff4b5`
- blind review SHA-256:
  `e91e301a4d9411ec4c3b86f84a965f33ffcbe12f2d8e8705bb9a9692bf760b61`
- blind key SHA-256:
  `6f57acc9ab7e58a4217f5190d184a6006a18e2be707939259998fd301117ec4c`
- frozen adjudication SHA-256:
  `3b7b87ff23588eec1338cfa83edf618408b31a95a5a8c63fdcc504e2446ae250`
- revealed report SHA-256:
  `73df479c90ab082a149fd984c357adad49f676dde51a73586c789e0c89203fae`

기존 5문항 wiring draft는 calibration 문항과 같은 근거·claim을 사용한
근접 중복이라 전부 폐기했다. 30문항은 새로 작성했으며 실행 후 question,
gold, rubric을 변경하거나 추가하지 않았다. 모든 60개 blind verdict를
채운 뒤에만 system key를 공개했다.

## 실행 조건

- systems: Strong, C3
- embedding: `bge-m3`
- reader: `gemma4:12b`, temperature 0, reasoning effort `none`
- top-k 5, context 상한 24,000 chars
- temporal anchor limit 1, graph neighbor limit 1
- output 상한 512 tokens
- question별 system 순서 무작위, repetition 1
- experiment log `N0139` retrieval 제외

60/60행이 414.5초에 완료됐다. timeout, empty final, resume, malformed
citation, truncation은 없었다.

## Blinded human task result

| Class | Strong | C3 | Difference |
|---|---:|---:|---:|
| Current decision | 3/5 | 3/5 | 0%p |
| Decision rationale | 5/5 | 5/5 | 0%p |
| Superseded avoidance | 2/5 | 3/5 | +20%p |
| Interrupted work | 3/5 | 3/5 | 0%p |
| Handoff | 3/5 | 3/5 | 0%p |
| Absent evidence | 5/5 | 5/5 | 0%p |
| **Total** | **21/30** | **22/30** | **+3.3%p** |

사전등록 핵심 15문항(rationale + superseded + handoff)은 Strong 10/15,
C3 11/15로 +6.7%p다.

### Paired differences

- C3만 통과: P013, P015, P025
- Strong만 통과: P014, P021
- 둘 다 같은 판정: 25문항

P013과 P015에서 C3가 후속 상태를 복원한 것은 calibration의 가능성을
재현한다. 그러나 P014에서는 반대로 Strong이 직접 결정적인
`N0135@r0003`을 검색해 통과했고, C3는 `r0001`을 anchor로 잡은 뒤
immediate successor `r0002`와 head `r0005`를 붙이면서 실제 완료 근거인
중간 `r0003`을 건너뛰었다. C3의 현재 규칙인 “한 anchor의 immediate
successor + head”는 임의 길이의 revision history에서 결정적인 중간
상태를 안정적으로 복원하지 못한다.

P021에서는 C3 context에 필요한 revision이 있었어도 reader가 기각안
주입 조건을 빼먹어 실패했다. P003에서는 C3 context에 최신 r0005가
있었지만 답변은 r0004를 인용했다. composer 확장은 evidence availability와
reader의 올바른 선택을 동일하게 보장하지 않는다.

## Retrieval and reader guardrails

| Metric | Strong | C3 | C3 / Strong |
|---|---:|---:|---:|
| Acceptable source success | 18/25 | 12/25 | — |
| Acceptable context success | 18/25 | 17/25 | — |
| Mean context chars | 11,082.0 | 15,834.4 | 1.429x |
| Mean prompt tokens | 5,307.9 | 7,479.7 | 1.409x |
| Mean reader latency | 6.261s | 7.308s | 1.167x |
| p95 reader latency | 9.162s | 10.138s | 1.107x |
| Citation presence | 100% | 100% | — |
| Context-valid citation precision | 100% | 93.9% | — |
| Truncation | 0/30 | 0/30 | — |
| Correct absent-evidence abstention | 5/5 | 5/5 | — |

C3는 unique-entry anchor를 고르는 entry-fold 때문에 source success가
Strong보다 낮아졌다. context expansion이 일부를 복구했지만 최종 exact
context success도 Strong보다 한 문항 낮았다. 특히 superseded 5문항의
acceptable context success는 Strong 3/5, C3 0/5였다. 여러 revision을
요구하는 질문에서 단일 temporal anchor 제한과 immediate/head 선택이
gold set을 완성하지 못했다.

C3의 context 비용 1.429x는 1.5x gate 안이지만 prompt token은 40.9%,
reader latency는 16.7% 늘었다. 인용 정밀도도 6.1%p 낮았다. 낮아진 인용
정밀도는 P003과 P024의 실패 답변에 집중되었다.

## Pre-registered gate 판정

| Gate | Requirement | Result | Verdict |
|---|---|---|---|
| Native task gain | 핵심 15문항 +15%p 이상 | +6.7%p | **Fail** |
| Current reference regression | 5%p 이하 | 0%p | Pass |
| Stale temporal failure | 50% 이상 감소 | Strong 1, C3 1 | **Fail** |
| Context cost | 1.5x 이하 | 1.429x | Pass |
| Citation/abstention guardrail | material regression 없음 | citation precision -6.1%p, abstention 동일 | **Fail** |

blind adjudication에서 P003(C3)과 P015(Strong)를 각각 stale 주실패로
분류했다. 표본이 시스템당 한 건뿐이라 stale gate는 통계적으로도 매우
불안정하지만, 사전등록 규칙상 감소가 없으므로 fail이다.

## 결론과 조치

1. **Strong을 기본 비교 기준으로 유지한다.** 현재 C3는 평균 정확도
   +3.3%p를 위해 context 42.9%, prompt token 40.9%, latency 16.7%를 더
   쓴다. 독립 가치로 정당화되지 않는다.
2. **C3를 제품 기능으로 승격하지 않는다.** 별도 interactive follow-up은
   offline pilot에서 충분한 gap이 나타날 때만 하기로 했으므로 이번에는
   진행하지 않는다.
3. **실험 harness와 frozen 자료는 보존한다.** 향후 retrieval 연구가 다시
   필요할 때 동일한 blind/pre-registration 절차를 재사용한다.
4. **재방문 조건은 알고리즘 변화다.** 단순 context 확대가 아니라 질문에
   필요한 timeline revision을 rerank하거나, intermediate revisions를
   선택하는 명시적 temporal retrieval이 구현된 경우에만 새 calibration과
   새 30문항으로 다시 시작한다. 현재 결과에 맞춰 gold나 gate를 조정하지
   않는다.
5. **Elendirna의 포지셔닝은 좁힌다.** authored provenance와 writeback을
   보존하는 개인 vault/reference/governance layer로 설명하되, 일반 RAG보다
   우월한 retrieval 제품이라는 주장은 하지 않는다.
