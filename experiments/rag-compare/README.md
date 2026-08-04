# RAG comparison experiment

This directory contains an experiment-only harness for comparing three retrieval
shapes and four cumulative native ablations over the same frozen Elendirna
vault snapshot:

- `flat`: one flattened document per entry.
- `strong`: base notes and revisions are independent retrieval chunks.
- `c1`: strong retrieval plus one anchor per entry.
- `c2`: C1 plus the current entry head.
- `c3`: C2 plus successor/base temporal context.
- `c4` (alias `native`): C3 plus authored-graph expansion.

The harness does not write to a vault or change product code. Evaluation
questions and run outputs stay outside the indexed vault so that an experiment
cannot teach later runs their own answers.

## Quick dry run

From the repository root:

```powershell
python experiments/rag-compare/rag_compare.py run `
  --snapshot experiments/rag-compare/fixtures/snapshot.jsonl `
  --questions experiments/rag-compare/fixtures/questions.jsonl `
  --output experiments/rag-compare/fixtures/run.jsonl `
  --systems flat,strong,c1,c2,c3,c4 `
  --retriever lexical `
  --reader none

python experiments/rag-compare/rag_compare.py score `
  --runs experiments/rag-compare/fixtures/run.jsonl `
  --questions experiments/rag-compare/fixtures/questions.jsonl `
  --output experiments/rag-compare/fixtures/score.json
```

The fixture is deliberately small and only verifies experiment mechanics. It
is not evidence for or against Elendirna.

## Export a frozen vault snapshot

Export before writing any experiment questions or results back to the vault:

```powershell
python experiments/rag-compare/rag_compare.py export `
  --vault D:\Work\elen-labs `
  --output C:\tmp\elendirna-rag-pilot\snapshot.jsonl `
  --graph-output C:\tmp\elendirna-rag-pilot\graph.json
```

The exporter uses only `elf --json entry list`, `elf --json bundle <id>
--depth 0`, and `elf graph --format json`. Alongside the JSONL snapshot it
writes a `.meta.json` file containing the snapshot hash and entry/revision
counts.

Keep the question set, run logs, and reports outside the vault until the whole
round has been scored. If a question or answer is recorded as an entry or
revision first, it is no longer a fresh evaluation item.

## Run a real retrieval comparison

The offline `lexical` retriever is useful for development. A real comparison
should use the same OpenAI-compatible embedding endpoint and model for all
systems:

```powershell
$env:RAG_COMPARE_API_KEY = "unused-for-local-ollama"

python experiments/rag-compare/rag_compare.py run `
  --snapshot C:\tmp\elendirna-rag-pilot\snapshot.jsonl `
  --graph C:\tmp\elendirna-rag-pilot\graph.json `
  --questions C:\tmp\elendirna-rag-pilot\questions.jsonl `
  --output C:\tmp\elendirna-rag-pilot\runs.jsonl `
  --systems flat,strong,c1,c2,c3,c4 `
  --retriever openai `
  --embedding-endpoint http://localhost:11434/v1 `
  --embedding-model bge-m3 `
  --embedding-cache C:\tmp\elendirna-rag-pilot\embeddings.json `
  --reader openai `
  --chat-endpoint http://localhost:11434/v1 `
  --chat-model gemma4:12b `
  --reasoning-effort none `
  --reader-max-tokens 512 `
  --status-policy route-aware `
  --exclude-entry N0139 `
  --top-k 5 `
  --temporal-anchor-limit 1 `
  --graph-neighbor-limit 1 `
  --max-context-chars 24000
```

Use the same embedding model, reader, top-k, context budget, and prompt for all
systems. The system shape must be the only independent variable.

Thinking-capable OpenAI-compatible readers must use an explicit reasoning
setting. The calibrated local reader uses `--reasoning-effort none`; otherwise
the model can spend the whole completion budget in a separate reasoning field
and return empty final content. Empty final content is treated as a run error.

Temporal successor/base expansion is limited to the highest-ranked entry
anchor by default, and C4 adds at most one authored neighbor. These limits keep
the lifecycle composer within the 1.5x context-cost gate established in the
calibration plan. Changing either limit starts a new calibration round.

## Question record

Each line of `questions.jsonl` is one JSON object:

```json
{
  "id": "Q001",
  "class": "current_decision",
  "route": "current",
  "question": "What deployment strategy is currently active?",
  "primary_refs": ["N9001@r0002"],
  "acceptable_ref_sets": [
    ["N9001@r0002"],
    ["N9001@r0001", "N9002@r0001"]
  ],
  "forbidden_refs": [],
  "required_claims": ["canary"],
  "forbidden_claims": ["blue is current", "green is current"],
  "expect_abstain": false
}
```

`route` is one of `current`, `history`, `rationale`, `handoff`, or `unknown`.
`acceptable_ref_sets` lists complete alternative evidence sets: satisfying any
one inner list is a success. `primary_refs` identifies the preferred evidence
for recall diagnostics. Legacy `required_refs` remains supported and is treated
as both the primary and sole acceptable set. Claims are simple deterministic
smoke checks; final pilot answers still require blinded human review.

The default `--status-policy route-aware` excludes archived entries from
`current`, `handoff`, and `unknown` routes, while allowing them for `history`
and `rationale`. Drafts are always excluded unless `--include-draft` is set.
Use `--status-policy all` only for an explicit ablation.

## Outputs

Every run row records:

- system and question identifiers;
- ranked retrieval hits and the exact revisions each hit covers;
- the composed context with temporal labels;
- answer text and reported API usage;
- retrieval and reader latency after preparation.

The adjacent `runs.jsonl.meta.json` records snapshot/graph hashes, eligible
chunk counts by route, parameters, and one-time `corpus_prepare_ms`. Embedding
and lexical index preparation happens before timed queries, so first-system
cache population is not misreported as retrieval latency.

Each completed question/system row is flushed to disk immediately. If a reader
request fails, rerun the same command with `--resume`; completed
`(question_id, system, repetition)` tuples are validated and skipped. The meta
sidecar exposes `status`, `completed_rows`, and `pending_rows` while a run is in
progress.

The scorer reports primary and acceptable-set recall, entry recall, exact
revision localization precision, final-context recall, forbidden-source
exposure, claim/citation checks, stale-claim rate, and abstention accuracy by
system. Revision localization precision exposes flat documents that retrieve
the right entry while indiscriminately covering many wrong revisions. Keeping
anchor and final-context metrics separate shows when lifecycle composition
recovers evidence that the first semantic hit missed. Workflow metrics
such as user corrections and time to first correct action require a later
interactive session log; they are intentionally not invented by this harness.

See [PLAN.md](PLAN.md) for the controlled pilot procedure and decision gates.
The latest calibration interpretation is in
[calibration/REPORT.md](calibration/REPORT.md).
