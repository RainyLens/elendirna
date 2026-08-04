# 30-question blind paired pilot pre-registration

Frozen before any pilot retrieval or reader run.

## Dataset

- 30 questions, with five questions in each class:
  `current_decision`, `decision_rationale`, `superseded_avoidance`,
  `interrupted_work`, `handoff`, and `absent_evidence`.
- All 30 questions use a different fact axis from the eight calibration
  questions. The former five-question wiring draft was retired because its
  evidence and claims were near-duplicates of calibration items.
- Gold references, acceptable evidence sets, required claims, stale claims,
  expected route, and a human rubric are frozen in `questions.jsonl`.
- No gold reference may be added after the first run. A genuine annotation
  defect invalidates the affected item rather than changing its gold.

## Fixed systems and controls

- Systems: `strong` and `c3` only.
- Snapshot SHA-256:
  `69908d289e25c1d5add464a0847ab750d3846b0ecfeae7810199c463483d2b6f`.
- Graph SHA-256:
  `09abe0fd040c7773c1e453e23792670b6256d15d80e0286a02da18026b526804`.
- Exclude experiment log entry `N0139` from retrieval.
- Embedding: `bge-m3`, OpenAI-compatible local endpoint.
- Reader: `gemma4:12b`, temperature 0, reasoning effort `none`, 512 output
  tokens.
- Retrieval: top-k 5, 24,000 maximum context characters.
- Composer: temporal anchor limit 1 and graph neighbor limit 1. The graph
  neighbor setting is inert for the two selected systems.
- One repetition is used because the reader is deterministic at temperature
  zero. System order is randomized per question with seed 20260804.

## Blind review

- Each question produces a pair whose systems are hidden behind independently
  randomized labels.
- The reviewer sees the question, rubric, answers, and citations, but not the
  system key or retrieval/context internals.
- Each answer is marked `pass` or `fail`. A fail also receives one primary
  reason: `incorrect`, `incomplete`, `stale`, `unsupported`,
  `citation_missing`, `citation_invalid`, or `failed_to_abstain`.
- The system key is revealed only after every item has an adjudication.
- Automatic claim matching is a smoke check, not the primary answer metric.

## Pre-registered decision gates

The native-value claim is accepted only if all applicable gates pass:

1. On the 15 temporal/rationale/handoff questions (`decision_rationale`,
   `superseded_avoidance`, `handoff`), C3 task accuracy improves by at least
   15 percentage points over Strong.
2. On the five `current_decision` reference questions, C3 does not regress by
   more than 5 percentage points.
3. C3 reduces stale temporal failures by at least 50 percent. If Strong has no
   stale failures, this gate is non-applicable and cannot support a native
   advantage claim.
4. C3 mean context size remains at or below 1.5 times Strong.
5. Citation presence, context-valid citation precision, and correct abstention
   are reported as guardrails. Any material regression blocks a product-value
   claim even if the accuracy gates pass.

If the gates fail, Elendirna remains justified as a personal authored vault or
reference implementation, but this pilot does not support a distinct
revision-aware retrieval advantage.
