# Elendirna vs RAG controlled pilot plan

## Objective

Determine whether Elendirna's lifecycle and provenance structure reduces stale
answers and work resumption errors beyond what a strong revision-aware RAG
baseline can achieve over the same source material.

This is not a benchmark of embedding models or reader LLMs. Retrieval and
reader components remain fixed while only corpus shape and context composition
change.

## Hypotheses

1. `strong` and `native` should perform similarly on reference-style questions.
2. `native` should expose fewer obsolete claims on current-state questions.
3. `native` should recover decision rationale and handoff state more reliably.
4. Any quality gain must survive a fixed context budget and be large enough to
   offset extra context or tool latency.

## Systems

### A — flat

All base and revision text for an entry is concatenated into one retrieval
document. This represents the common "index the note" shape and preserves the
information while removing revision granularity.

### B — strong

The base note and every revision are separate chunks with entry ID, revision
ID, status, timestamp, baseline, and links. This is the competitive ordinary
RAG baseline. It prevents an entry-level chunking weakness from being mistaken
for lifecycle value.

### C — native ablation ladder

Uses the same revision chunks and scorer as B, then applies Elendirna-shaped
operations cumulatively so each marginal effect is visible:

- C1 folds the best revision to one anchor per entry;
- C2 adds the current entry head;
- C3 adds successor/base context with explicit temporal labels;
- C4 adds authored-graph neighbors;
- `current` composition includes base, matched evidence, and entry head with
  explicit temporal labels;
- `history` and `rationale` composition includes the matched revision, its
  successor, and head so that a reader can distinguish a waypoint from the
  current state;
- `handoff` composition prioritizes entry head and related authored entries;
- optional graph expansion reads only authored export edges.

No computed suggestion is persisted and no test result is written into the
snapshot.

### D — GraphRAG (deferred)

Run only after A/B/C show a stable lifecycle effect. Import the same authored
graph export and keep the reader and budget fixed. D answers whether a graph
engine adds value after the source-side lifecycle contract is already present.

## Data preparation

1. Freeze a vault snapshot and graph export with the experiment exporter.
2. Record the snapshot SHA-256 in every round report.
3. Prepare two corpora:
   - a controlled synthetic lifecycle corpus;
   - a frozen real dogfood snapshot.
4. Store all questions and results outside both corpora.
5. Do not reuse a question after its answer or analysis has entered the vault.

## Pilot set

Use 30 fresh questions, five from each class:

1. current decision;
2. decision rationale;
3. superseded approach avoidance;
4. interrupted work restoration;
5. cross-agent handoff;
6. absent evidence / abstention.

For each question define required references, forbidden references, required
claims, forbidden stale claims, expected route, and a short human scoring
rubric. Questions should use natural paraphrases rather than copying vault
phrases.

Calibration precedes the 30-question pilot. It uses eight questions covering:
current state, reference lookup, deep revision, multiple valid evidence sets,
explicit stale state, current/history conflict, handoff, and absent evidence.
The calibration set may diagnose mechanics but must not be counted as pilot
evidence.

## Fixed controls

- identical snapshot and source text;
- identical embedding endpoint/model;
- identical reader endpoint/model and verify-first prompt;
- temperature 0;
- identical top-k and maximum context budget;
- three runs per question when the reader is nondeterministic;
- randomized system order;
- system names hidden during human scoring.

Tune parameters only during a five-question dry run. Freeze them before the
30-question pilot. A tuned run is a new round, not a continuation of an old
round.

## Measurements

### Retrieval

- preferred-source and acceptable-evidence-set recall;
- entry recall and exact revision localization precision;
- acceptable-set mean reciprocal rank;
- forbidden-source exposure at retrieval and final context;
- one-time corpus preparation separated from warm query latency.

### Answer

- current-decision accuracy;
- rationale completeness;
- stale-claim rate;
- citation correctness;
- correct abstention.

### Workflow follow-up

Run a smaller interactive follow-up only if the offline pilot shows a gap:

- time to first correct action;
- user restatement volume;
- user corrections;
- tool calls and context tokens;
- patches discarded because of obsolete assumptions.

## Initial decision gates

Compare `native` against `strong`, not merely against `flat`.

- stale-state errors reduced by at least 50%;
- rationale or handoff success improved by at least 15 percentage points;
- reference-question regression no worse than 5 percentage points;
- time to first correct action or user restatement reduced by at least 25% in
  the interactive follow-up;
- context/tool cost no more than 1.5x unless rework reduction clearly offsets
  it.

Interpretation:

- no meaningful B/C gap: keep Elendirna as personal infrastructure;
- advantage only on history-heavy tasks: position it as a context-governance or
  reference layer;
- advantage across workflow tasks: continue productizing the layer;
- no additional D/C gap: keep general-purpose GraphRAG out of core scope.

## Work sequence

1. Land experiment specification, exporter, runners, scorer, and offline
   fixtures.
2. Verify the harness with the deterministic lexical fixture.
3. Export a real frozen snapshot.
4. Author and review eight calibration questions outside the vault.
5. Run the strong baseline and native composer ablations: entry fold, current
   head, temporal successor/base, and authored graph expansion.
6. Run the fixed reader only after retrieval and context gates pass; correct
   only harness defects.
7. Freeze parameters and author 30 fresh pilot questions.
8. Execute the blind pilot and produce a paired report.
9. Decide whether an interactive workflow round and GraphRAG round are
   justified.
