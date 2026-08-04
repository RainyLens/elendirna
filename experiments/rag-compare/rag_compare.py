#!/usr/bin/env python3
"""Controlled flat/strong/native RAG comparison over a frozen Elendirna export.

The script intentionally depends only on the Python standard library. It can
exercise experiment mechanics offline with BM25-like lexical retrieval, or use
OpenAI-compatible embedding and chat endpoints for a real pilot.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import random
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from collections import Counter, defaultdict
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Iterable, Sequence


TOKEN_RE = re.compile(r"[0-9A-Za-z_가-힣]+", re.UNICODE)
VALID_ROUTES = {"current", "history", "rationale", "handoff", "unknown"}
VERIFY_FIRST_PROMPT = """당신은 근거 제한형 지식 복원 reader입니다.
제공된 CONTEXT만 사용하십시오. 먼저 `근거판정: 충분` 또는
`근거판정: 불충분`을 출력하십시오. 충분한 경우 각 핵심 주장 뒤에
`[N####@r####]` 형식의 출처를 붙이십시오. 인접한 주제라는 이유만으로
현재 결정이라고 추정하지 마십시오. CURRENT_HEAD, MATCHED_HISTORICAL,
SUCCESSOR 같은 시제 표지를 존중하십시오. 근거가 직접 뒷받침하지 않으면
답을 만들지 말고 `자료에 없는 내용입니다`라고 기권하십시오.
인용은 각 CONTEXT 섹션의 머리표나 `source:` 줄에 표시된 ref만 사용하고,
본문 안에서 간접 언급된 다른 ref는 직접 읽지 않았으므로 인용하지 마십시오.
질문에 직접 필요한 내용만 8문장 이내로 간결하게 답하십시오."""


@dataclass(frozen=True)
class Chunk:
    entry_id: str
    rev_id: str
    title: str
    text: str
    status: str
    created: str
    updated: str
    baseline: str | None
    links: tuple[str, ...]
    tags: tuple[str, ...]
    ordinal: int
    is_head: bool

    @property
    def ref(self) -> str:
        return f"{self.entry_id}@{self.rev_id}"


@dataclass(frozen=True)
class Document:
    doc_id: str
    ref: str
    entry_id: str
    text: str
    covers: tuple[str, ...]
    chunk_refs: tuple[str, ...]


def json_dumps(value: Any, *, pretty: bool = False) -> str:
    return json.dumps(
        value,
        ensure_ascii=False,
        indent=2 if pretty else None,
        sort_keys=pretty,
    )


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open("r", encoding="utf-8") as handle:
        for line_number, line in enumerate(handle, start=1):
            if not line.strip():
                continue
            try:
                value = json.loads(line)
            except json.JSONDecodeError as exc:
                raise ValueError(f"{path}:{line_number}: invalid JSON: {exc}") from exc
            if not isinstance(value, dict):
                raise ValueError(f"{path}:{line_number}: expected a JSON object")
            rows.append(value)
    return rows


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    with temporary.open("w", encoding="utf-8", newline="\n") as handle:
        for row in rows:
            handle.write(json_dumps(row))
            handle.write("\n")
    temporary.replace(path)


def append_jsonl_row(path: Path, row: dict[str, Any]) -> None:
    """Durably append one completed work item for crash-safe resume."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8", newline="\n") as handle:
        handle.write(json_dumps(row))
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json_dumps(value, pretty=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def run_json(command: Sequence[str]) -> Any:
    completed = subprocess.run(
        list(command),
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"command failed ({completed.returncode}): {' '.join(command)}\n"
            f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(
            f"command returned non-JSON output: {' '.join(command)}\n{completed.stdout}"
        ) from exc


def unwrap_elf(value: Any, command: str) -> Any:
    if isinstance(value, dict) and "ok" in value and "data" in value:
        if not value.get("ok"):
            raise RuntimeError(f"elf {command} failed: {value}")
        return value["data"]
    return value


def export_snapshot(args: argparse.Namespace) -> None:
    vault = str(Path(args.vault).resolve())
    prefix = [args.elf, "--json", "--vault", vault]
    listed = unwrap_elf(run_json([*prefix, "entry", "list"]), "entry list")
    if not isinstance(listed, list):
        raise RuntimeError("elf entry list returned a non-list payload")

    chunks: list[dict[str, Any]] = []
    for summary in sorted(listed, key=lambda row: row["id"]):
        entry_id = summary["id"]
        bundle = unwrap_elf(
            run_json([*prefix, "bundle", entry_id, "--depth", "0"]),
            f"bundle {entry_id}",
        )
        manifest = bundle["manifest"]
        revisions = bundle.get("revisions", [])
        head_ordinal = len(revisions)
        common = {
            "entry_id": entry_id,
            "title": manifest.get("title", ""),
            "status": manifest.get("status", ""),
            "updated": manifest.get("updated", ""),
            "baseline": manifest.get("baseline"),
            "links": manifest.get("links", []),
            "tags": manifest.get("tags", []),
        }
        chunks.append(
            {
                **common,
                "rev_id": "r0000",
                "kind": "base",
                "text": bundle.get("note", ""),
                "created": manifest.get("created", ""),
                "ordinal": 0,
                "is_head": head_ordinal == 0,
            }
        )
        for ordinal, revision in enumerate(revisions, start=1):
            chunks.append(
                {
                    **common,
                    "rev_id": revision["rev_id"],
                    "kind": "revision",
                    "text": revision.get("delta", ""),
                    "created": revision.get("created", ""),
                    "revision_baseline": revision.get("baseline"),
                    "ordinal": ordinal,
                    "is_head": ordinal == head_ordinal,
                }
            )

    output = Path(args.output)
    write_jsonl(output, chunks)

    graph_output: Path | None = None
    if args.graph_output:
        graph_output = Path(args.graph_output)
        graph = unwrap_elf(
            run_json([*prefix, "graph", "--format", "json"]),
            "graph",
        )
        write_json(graph_output, graph)

    entry_count = len({row["entry_id"] for row in chunks})
    meta = {
        "created_unix": int(time.time()),
        "vault": vault,
        "snapshot": str(output.resolve()),
        "snapshot_sha256": file_sha256(output),
        "entry_count": entry_count,
        "chunk_count": len(chunks),
        "revision_count": len(chunks) - entry_count,
        "graph": str(graph_output.resolve()) if graph_output else None,
        "graph_sha256": file_sha256(graph_output) if graph_output else None,
    }
    meta_path = output.with_suffix(output.suffix + ".meta.json")
    write_json(meta_path, meta)
    print(json_dumps(meta, pretty=True))


def load_chunks(path: Path) -> list[Chunk]:
    chunks: list[Chunk] = []
    for row in read_jsonl(path):
        chunk = Chunk(
            entry_id=str(row["entry_id"]),
            rev_id=str(row["rev_id"]),
            title=str(row.get("title", "")),
            text=str(row.get("text", "")),
            status=str(row.get("status", "")),
            created=str(row.get("created", "")),
            updated=str(row.get("updated", "")),
            baseline=row.get("baseline"),
            links=tuple(str(item) for item in row.get("links", [])),
            tags=tuple(str(item) for item in row.get("tags", [])),
            ordinal=int(row.get("ordinal", 0)),
            is_head=bool(row.get("is_head", False)),
        )
        chunks.append(chunk)
    if not chunks:
        raise ValueError(f"snapshot is empty: {path}")
    refs = [chunk.ref for chunk in chunks]
    if len(refs) != len(set(refs)):
        raise ValueError(f"snapshot contains duplicate entry/revision refs: {path}")
    return chunks


def validate_questions(rows: list[dict[str, Any]], path: Path) -> None:
    seen: set[str] = set()
    for row in rows:
        question_id = str(row.get("id", ""))
        if not question_id:
            raise ValueError(f"question without id in {path}")
        if question_id in seen:
            raise ValueError(f"duplicate question id {question_id} in {path}")
        seen.add(question_id)
        if not row.get("question"):
            raise ValueError(f"question {question_id} has no text")
        route = str(row.get("route", "unknown"))
        if route not in VALID_ROUTES:
            raise ValueError(
                f"question {question_id} has invalid route {route!r}; "
                f"expected one of {sorted(VALID_ROUTES)}"
            )
        for field in (
            "required_refs",
            "primary_refs",
            "forbidden_refs",
            "required_claims",
            "forbidden_claims",
        ):
            value = row.get(field, [])
            if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
                raise ValueError(f"question {question_id} field {field} must be a string list")
        acceptable_ref_sets = row.get("acceptable_ref_sets", [])
        if not isinstance(acceptable_ref_sets, list) or not all(
            isinstance(ref_set, list)
            and bool(ref_set)
            and all(isinstance(item, str) for item in ref_set)
            for ref_set in acceptable_ref_sets
        ):
            raise ValueError(
                f"question {question_id} field acceptable_ref_sets must be "
                "a list of non-empty string lists"
            )


def question_ref_sets(question: dict[str, Any]) -> list[list[str]]:
    """Return complete alternative source sets accepted for a question."""
    alternatives = question.get("acceptable_ref_sets", [])
    if alternatives:
        return [[str(ref) for ref in ref_set] for ref_set in alternatives]
    required = [str(ref) for ref in question.get("required_refs", [])]
    return [required] if required else []


def question_primary_refs(question: dict[str, Any]) -> list[str]:
    primary = [str(ref) for ref in question.get("primary_refs", [])]
    if primary:
        return primary
    required = [str(ref) for ref in question.get("required_refs", [])]
    if required:
        return required
    alternatives = question_ref_sets(question)
    return alternatives[0] if alternatives else []


def eligible_chunks(
    chunks: Sequence[Chunk],
    route: str,
    status_policy: str,
    include_draft: bool,
) -> list[Chunk]:
    """Apply the experiment's explicit status visibility policy."""
    if status_policy == "all":
        return list(chunks)
    allow_archived = route in {"history", "rationale"}
    selected: list[Chunk] = []
    for chunk in chunks:
        status = chunk.status.casefold()
        if status == "archived" and not allow_archived:
            continue
        if status == "draft" and not include_draft:
            continue
        selected.append(chunk)
    return selected


def chunk_document_text(chunk: Chunk) -> str:
    position = "CURRENT_HEAD" if chunk.is_head else "HISTORICAL_REVISION"
    return (
        f"title: {chunk.title}\n"
        f"source: {chunk.ref}\n"
        f"status: {chunk.status}\n"
        f"temporal_position: {position}\n"
        f"created: {chunk.created}\n"
        f"text:\n{chunk.text}"
    )


def build_strong_documents(chunks: Sequence[Chunk]) -> list[Document]:
    return [
        Document(
            doc_id=chunk.ref,
            ref=chunk.ref,
            entry_id=chunk.entry_id,
            text=chunk_document_text(chunk),
            covers=(chunk.ref,),
            chunk_refs=(chunk.ref,),
        )
        for chunk in chunks
    ]


def build_flat_documents(chunks: Sequence[Chunk]) -> list[Document]:
    grouped: dict[str, list[Chunk]] = defaultdict(list)
    for chunk in chunks:
        grouped[chunk.entry_id].append(chunk)
    documents: list[Document] = []
    for entry_id, records in sorted(grouped.items()):
        ordered = sorted(records, key=lambda chunk: chunk.ordinal)
        sections = [
            f"[{chunk.ref}]\n{chunk_document_text(chunk)}" for chunk in ordered
        ]
        documents.append(
            Document(
                doc_id=f"{entry_id}@flat",
                ref=f"{entry_id}@flat",
                entry_id=entry_id,
                text="\n\n".join(sections),
                covers=tuple(chunk.ref for chunk in ordered),
                chunk_refs=tuple(chunk.ref for chunk in ordered),
            )
        )
    return documents


def tokenize(text: str) -> list[str]:
    tokens: list[str] = []
    for raw in TOKEN_RE.findall(text):
        token = raw.casefold()
        tokens.append(token)
        # The offline scorer is a mechanics check, but a whitespace-only Korean
        # token would still make 조사/어미 variants look unrelated. Character
        # bigrams provide a deterministic dependency-free fallback. A real
        # pilot uses the configured multilingual embedding model instead.
        if len(token) >= 2 and any("가" <= char <= "힣" for char in token):
            tokens.extend(token[index : index + 2] for index in range(len(token) - 1))
    return tokens


class LexicalRanker:
    def __init__(self) -> None:
        self._cache: dict[tuple[str, ...], tuple[list[list[str]], Counter[str], float]] = {}

    def prepare(
        self,
        document_sets: Sequence[Sequence[Document]],
        queries: Sequence[str],
    ) -> None:
        del queries
        for documents in document_sets:
            self.rank(documents, "", 0)

    def rank(
        self, documents: Sequence[Document], query: str, limit: int
    ) -> list[tuple[float, Document]]:
        key = tuple(document.doc_id for document in documents)
        cached = self._cache.get(key)
        if cached is None:
            tokenized = [tokenize(document.text) for document in documents]
            document_frequency: Counter[str] = Counter()
            for tokens in tokenized:
                document_frequency.update(set(tokens))
            average_length = (
                sum(len(tokens) for tokens in tokenized) / len(tokenized)
                if tokenized
                else 0.0
            )
            cached = (tokenized, document_frequency, average_length)
            self._cache[key] = cached
        tokenized, document_frequency, average_length = cached
        query_terms = Counter(tokenize(query))
        count = max(len(documents), 1)
        k1 = 1.5
        b = 0.75
        ranked: list[tuple[float, Document]] = []
        for document, tokens in zip(documents, tokenized):
            frequencies = Counter(tokens)
            score = 0.0
            for term, query_frequency in query_terms.items():
                frequency = frequencies.get(term, 0)
                if not frequency:
                    continue
                df = document_frequency.get(term, 0)
                inverse_document_frequency = math.log(
                    1.0 + (count - df + 0.5) / (df + 0.5)
                )
                length_norm = 1.0 - b
                if average_length:
                    length_norm += b * len(tokens) / average_length
                score += (
                    query_frequency
                    * inverse_document_frequency
                    * frequency
                    * (k1 + 1.0)
                    / (frequency + k1 * length_norm)
                )
            ranked.append((score, document))
        ranked.sort(key=lambda pair: (-pair[0], pair[1].doc_id))
        return ranked[:limit]


class OpenAICompatibleClient:
    def __init__(self, base_url: str, api_key: str, timeout: float) -> None:
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.timeout = timeout

    def post(self, path: str, payload: dict[str, Any]) -> dict[str, Any]:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        headers = {"Content-Type": "application/json"}
        if self.api_key:
            headers["Authorization"] = f"Bearer {self.api_key}"
        request = urllib.request.Request(
            f"{self.base_url}{path}", data=body, headers=headers, method="POST"
        )
        last_error: Exception | None = None
        for attempt in range(3):
            try:
                with urllib.request.urlopen(request, timeout=self.timeout) as response:
                    value = json.loads(response.read().decode("utf-8"))
                    if not isinstance(value, dict):
                        raise RuntimeError("endpoint returned a non-object JSON response")
                    return value
            except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
                last_error = exc
                if attempt < 2:
                    time.sleep(0.5 * (attempt + 1))
        raise RuntimeError(f"request failed after 3 attempts: {last_error}")


class EmbeddingRanker:
    def __init__(
        self,
        client: OpenAICompatibleClient,
        model: str,
        cache_path: Path | None,
    ) -> None:
        self.client = client
        self.model = model
        self.cache_path = cache_path
        self.cache: dict[str, list[float]] = {}
        if cache_path and cache_path.exists():
            loaded = json.loads(cache_path.read_text(encoding="utf-8"))
            if isinstance(loaded, dict):
                self.cache = {
                    str(key): [float(value) for value in values]
                    for key, values in loaded.items()
                    if isinstance(values, list)
                }

    def _key(self, text: str) -> str:
        return hashlib.sha256(f"{self.model}\0{text}".encode("utf-8")).hexdigest()

    def embed(self, texts: Sequence[str]) -> list[list[float]]:
        missing: list[str] = []
        seen_missing: set[str] = set()
        for text in texts:
            key = self._key(text)
            if key not in self.cache and key not in seen_missing:
                missing.append(text)
                seen_missing.add(key)
        for start in range(0, len(missing), 64):
            batch = missing[start : start + 64]
            response = self.client.post(
                "/embeddings", {"model": self.model, "input": batch}
            )
            data = response.get("data", [])
            if len(data) != len(batch):
                raise RuntimeError(
                    f"embedding endpoint returned {len(data)} vectors for {len(batch)} inputs"
                )
            ordered = sorted(data, key=lambda item: int(item.get("index", 0)))
            for text, item in zip(batch, ordered):
                vector = item.get("embedding")
                if not isinstance(vector, list):
                    raise RuntimeError("embedding response is missing an embedding list")
                self.cache[self._key(text)] = [float(value) for value in vector]
        if missing and self.cache_path:
            write_json(self.cache_path, self.cache)
        return [self.cache[self._key(text)] for text in texts]

    def prepare(
        self,
        document_sets: Sequence[Sequence[Document]],
        queries: Sequence[str],
    ) -> None:
        texts: list[str] = []
        seen: set[str] = set()
        for documents in document_sets:
            for document in documents:
                key = self._key(document.text)
                if key not in seen:
                    seen.add(key)
                    texts.append(document.text)
        for query in queries:
            key = self._key(query)
            if key not in seen:
                seen.add(key)
                texts.append(query)
        self.embed(texts)

    def rank(
        self, documents: Sequence[Document], query: str, limit: int
    ) -> list[tuple[float, Document]]:
        vectors = self.embed([document.text for document in documents])
        query_vector = self.embed([query])[0]
        ranked = [
            (cosine_similarity(query_vector, vector), document)
            for document, vector in zip(documents, vectors)
        ]
        ranked.sort(key=lambda pair: (-pair[0], pair[1].doc_id))
        return ranked[:limit]


def cosine_similarity(left: Sequence[float], right: Sequence[float]) -> float:
    if len(left) != len(right):
        raise ValueError(f"vector dimensions differ: {len(left)} != {len(right)}")
    numerator = sum(a * b for a, b in zip(left, right))
    left_norm = math.sqrt(sum(value * value for value in left))
    right_norm = math.sqrt(sum(value * value for value in right))
    if not left_norm or not right_norm:
        return 0.0
    return numerator / (left_norm * right_norm)


def normalize_entry_ref(ref: str) -> str:
    return ref.split("@", 1)[0]


def load_graph(path: Path | None) -> dict[str, set[str]]:
    adjacency: dict[str, set[str]] = defaultdict(set)
    if path is None:
        return adjacency
    value = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(value, dict) and "data" in value and "ok" in value:
        value = value["data"]
    for edge in value.get("edges", []):
        source = normalize_entry_ref(str(edge.get("from", "")))
        target = normalize_entry_ref(str(edge.get("to", "")))
        if not source or not target or source == target:
            continue
        adjacency[source].add(target)
        adjacency[target].add(source)
    return adjacency


def choose_native_anchors(
    ranked: Sequence[tuple[float, Document]], top_k: int
) -> list[tuple[float, Document]]:
    anchors: list[tuple[float, Document]] = []
    seen_entries: set[str] = set()
    for score, document in ranked:
        if document.entry_id in seen_entries:
            continue
        seen_entries.add(document.entry_id)
        anchors.append((score, document))
        if len(anchors) >= top_k:
            break
    return anchors


def temporal_label(chunk: Chunk, matched_ref: str, route: str) -> str:
    if chunk.is_head:
        return "CURRENT_HEAD"
    if chunk.ref == matched_ref:
        return "MATCHED_HISTORICAL"
    if route in {"history", "rationale"}:
        return "HISTORICAL_CONTEXT"
    return "BASE_CONTEXT" if chunk.rev_id == "r0000" else "HISTORICAL_CONTEXT"


def compose_native_context(
    chunks: Sequence[Chunk],
    anchors: Sequence[tuple[float, Document]],
    route: str,
    adjacency: dict[str, set[str]],
    top_k: int,
    max_chars: int = 1_000_000_000,
    composer_level: str = "c4",
    temporal_anchor_limit: int = 1,
    graph_neighbor_limit: int = 1,
) -> tuple[str, list[str]]:
    if composer_level not in {"c1", "c2", "c3", "c4"}:
        raise ValueError(f"unknown composer level: {composer_level}")
    include_head = composer_level in {"c2", "c3", "c4"}
    include_temporal = composer_level in {"c3", "c4"}
    include_graph = composer_level == "c4"
    grouped: dict[str, list[Chunk]] = defaultdict(list)
    by_ref: dict[str, Chunk] = {}
    for chunk in chunks:
        grouped[chunk.entry_id].append(chunk)
        by_ref[chunk.ref] = chunk
    for records in grouped.values():
        records.sort(key=lambda chunk: chunk.ordinal)

    selected: list[tuple[str, Chunk]] = []
    selected_refs: set[str] = set()

    def add(label: str, chunk: Chunk) -> None:
        if chunk.ref not in selected_refs:
            selected_refs.add(chunk.ref)
            selected.append((label, chunk))

    for anchor_index, (_score, anchor) in enumerate(anchors):
        records = grouped.get(anchor.entry_id, [])
        if not records:
            continue
        matched = by_ref.get(anchor.ref, records[-1])
        if route == "unknown":
            add("RETRIEVED", matched)
            continue
        if route == "handoff" and include_head:
            add("CURRENT_HEAD", records[-1])
            add(temporal_label(matched, anchor.ref, route), matched)
        else:
            add(temporal_label(matched, anchor.ref, route), matched)
        if (
            include_temporal
            and anchor_index < temporal_anchor_limit
            and route in {"history", "rationale"}
            and not matched.is_head
        ):
            successor_index = min(matched.ordinal + 1, len(records) - 1)
            successor = records[successor_index]
            if successor.ref != matched.ref:
                add("SUCCESSOR", successor)
        if include_head:
            add("CURRENT_HEAD", records[-1])
        if include_temporal and anchor_index < temporal_anchor_limit:
            add("BASE_CONTEXT", records[0])

    graph_budget = graph_neighbor_limit if adjacency and include_graph else 0
    anchor_entries = {anchor.entry_id for _, anchor in anchors}
    for anchor_entry in sorted(anchor_entries):
        for neighbor in sorted(adjacency.get(anchor_entry, set())):
            if graph_budget <= 0:
                break
            if neighbor in anchor_entries or not grouped.get(neighbor):
                continue
            add("AUTHORED_NEIGHBOR_HEAD", grouped[neighbor][-1])
            graph_budget -= 1

    sections = [
        (
            f"[{label} {chunk.ref}]\n{chunk_document_text(chunk)}",
            (chunk.ref,),
        )
        for label, chunk in selected
    ]
    return pack_context_sections(sections, max_chars)


def pack_context_sections(
    sections: Sequence[tuple[str, Sequence[str]]], max_chars: int
) -> tuple[str, list[str]]:
    """Pack whole ranked sections and report only refs that reach the reader."""
    packed: list[str] = []
    refs: list[str] = []
    used = 0
    separator = "\n\n"
    suffix = "\n\n[CONTEXT_TRUNCATED]"
    for section, section_refs in sections:
        separator_cost = len(separator) if packed else 0
        if used + separator_cost + len(section) <= max_chars:
            packed.append(section)
            refs.extend(str(ref) for ref in section_refs)
            used += separator_cost + len(section)
            continue
        if not packed:
            available = max(0, max_chars - len(suffix))
            packed.append(section[:available] + suffix)
            refs.extend(str(ref) for ref in section_refs)
        elif used + len(suffix) <= max_chars:
            packed.append("[CONTEXT_TRUNCATED]")
        break
    return separator.join(packed), refs


def answer_with_reader(
    client: OpenAICompatibleClient,
    model: str,
    question: str,
    context: str,
    temperature: float,
    max_tokens: int,
    reasoning_effort: str,
) -> tuple[str, dict[str, Any]]:
    response = client.post(
        "/chat/completions",
        {
            "model": model,
            "temperature": temperature,
            "max_tokens": max_tokens,
            "reasoning_effort": reasoning_effort,
            "messages": [
                {"role": "system", "content": VERIFY_FIRST_PROMPT},
                {
                    "role": "user",
                    "content": f"QUESTION:\n{question}\n\nCONTEXT:\n{context}",
                },
            ],
        },
    )
    choices = response.get("choices", [])
    if not choices:
        raise RuntimeError("chat endpoint returned no choices")
    message = choices[0].get("message", {})
    answer = str(message.get("content", ""))
    finish_reason = str(choices[0].get("finish_reason", ""))
    if not answer.strip():
        reasoning_chars = len(str(message.get("reasoning", "")))
        raise RuntimeError(
            "reader returned empty final content "
            f"(finish_reason={finish_reason!r}, reasoning_chars={reasoning_chars})"
        )
    raw_usage = response.get("usage", {})
    usage = dict(raw_usage) if isinstance(raw_usage, dict) else {}
    usage["finish_reason"] = finish_reason
    usage["reasoning_chars"] = len(str(message.get("reasoning", "")))
    return answer, usage


def run_comparison(args: argparse.Namespace) -> None:
    total_setup_started = time.perf_counter()
    snapshot_path = Path(args.snapshot)
    chunks = load_chunks(snapshot_path)
    excluded_entry_ids = {str(entry_id) for entry_id in args.exclude_entry}
    if excluded_entry_ids:
        chunks = [
            chunk for chunk in chunks if chunk.entry_id not in excluded_entry_ids
        ]
        if not chunks:
            raise ValueError("entry exclusions removed every snapshot chunk")
    questions = read_jsonl(Path(args.questions))
    validate_questions(questions, Path(args.questions))
    systems = [item.strip() for item in args.systems.split(",") if item.strip()]
    native_systems = {"c1", "c2", "c3", "c4", "native"}
    invalid_systems = set(systems) - ({"flat", "strong"} | native_systems)
    if invalid_systems:
        raise ValueError(f"unknown systems: {sorted(invalid_systems)}")
    if not systems:
        raise ValueError("at least one system is required")

    api_key = os.environ.get(args.api_key_env, "")
    ranker_init_started = time.perf_counter()
    if args.retriever == "openai":
        if not args.embedding_endpoint or not args.embedding_model:
            raise ValueError("openai retriever requires embedding endpoint and model")
        embedding_client = OpenAICompatibleClient(
            args.embedding_endpoint, api_key, args.timeout
        )
        ranker: Any = EmbeddingRanker(
            embedding_client,
            args.embedding_model,
            Path(args.embedding_cache) if args.embedding_cache else None,
        )
    else:
        ranker = LexicalRanker()
    ranker_init_ms = round(
        (time.perf_counter() - ranker_init_started) * 1000.0,
        3,
    )

    reader_client: OpenAICompatibleClient | None = None
    if args.reader == "openai":
        if not args.chat_endpoint or not args.chat_model:
            raise ValueError("openai reader requires chat endpoint and model")
        reader_client = OpenAICompatibleClient(args.chat_endpoint, api_key, args.timeout)

    adjacency = load_graph(Path(args.graph) if args.graph else None)
    rng = random.Random(args.seed)
    output_rows: list[dict[str, Any]] = []
    snapshot_hash = file_sha256(snapshot_path)

    routes = sorted({str(question.get("route", "unknown")) for question in questions})
    chunks_by_route = {
        route: eligible_chunks(
            chunks,
            route,
            args.status_policy,
            args.include_draft,
        )
        for route in routes
    }
    documents_by_route: dict[str, dict[str, list[Document]]] = {}
    for route, route_chunks in chunks_by_route.items():
        documents_by_route[route] = {
            "flat": build_flat_documents(route_chunks),
            "strong": build_strong_documents(route_chunks),
        }

    document_sets: list[Sequence[Document]] = []
    seen_document_sets: set[tuple[str, ...]] = set()
    required_document_kinds: set[str] = set()
    if "flat" in systems:
        required_document_kinds.add("flat")
    if any(system != "flat" for system in systems):
        required_document_kinds.add("strong")
    for route_documents in documents_by_route.values():
        for document_kind in sorted(required_document_kinds):
            documents = route_documents[document_kind]
            key = tuple(document.doc_id for document in documents)
            if key not in seen_document_sets:
                seen_document_sets.add(key)
                document_sets.append(documents)
    preparation_started = time.perf_counter()
    ranker.prepare(
        document_sets,
        [str(question["question"]) for question in questions],
    )
    corpus_prepare_ms = round(
        (time.perf_counter() - preparation_started) * 1000.0,
        3,
    )
    total_prepare_ms = round(
        (time.perf_counter() - total_setup_started) * 1000.0,
        3,
    )

    work_items: list[tuple[int, dict[str, Any], str]] = []
    for repetition in range(args.repeat):
        for question in questions:
            shuffled = systems.copy()
            rng.shuffle(shuffled)
            for system in shuffled:
                work_items.append((repetition, question, system))

    output_path = Path(args.output)
    planned_keys = {
        (str(question["id"]), system, repetition)
        for repetition, question, system in work_items
    }
    if args.resume and output_path.exists():
        output_rows = read_jsonl(output_path)
    else:
        output_rows = []
        write_jsonl(output_path, output_rows)
    completed_keys: set[tuple[str, str, int]] = set()
    for row in output_rows:
        if str(row.get("snapshot_sha256", "")) != snapshot_hash:
            raise ValueError("resume output snapshot hash does not match this run")
        key = (
            str(row.get("question_id", "")),
            str(row.get("system", "")),
            int(row.get("repetition", 0)),
        )
        if key not in planned_keys:
            raise ValueError(f"resume output contains an unexpected work item: {key}")
        if key in completed_keys:
            raise ValueError(f"resume output contains a duplicate work item: {key}")
        completed_keys.add(key)
    pending_work_items = [
        item
        for item in work_items
        if (str(item[1]["id"]), item[2], item[0]) not in completed_keys
    ]
    progress_meta_path = output_path.with_suffix(output_path.suffix + ".meta.json")
    progress_meta = {
        "status": "running",
        "output": str(output_path.resolve()),
        "snapshot_sha256": snapshot_hash,
        "planned_rows": len(work_items),
        "completed_rows": len(output_rows),
        "pending_rows": len(pending_work_items),
        "resume": args.resume,
    }
    write_json(progress_meta_path, progress_meta)

    for repetition, question, system in pending_work_items:
        route = str(question.get("route", "unknown"))
        route_documents = documents_by_route[route]
        documents = route_documents["flat" if system == "flat" else "strong"]
        retrieval_limit = args.top_k * 4 if system in native_systems else args.top_k
        retrieval_started = time.perf_counter()
        ranked = ranker.rank(documents, str(question["question"]), retrieval_limit)
        if system in native_systems:
            ranked = choose_native_anchors(ranked, args.top_k)
        else:
            ranked = ranked[: args.top_k]
        retrieval_ms = round((time.perf_counter() - retrieval_started) * 1000.0, 3)

        retrieved = [
            {
                "rank": index,
                "ref": document.ref,
                "entry_id": document.entry_id,
                "score": score,
                "covers": list(document.covers),
            }
            for index, (score, document) in enumerate(ranked, start=1)
        ]

        if system in native_systems:
            composer_level = "c4" if system == "native" else system
            context, context_refs = compose_native_context(
                chunks_by_route[route],
                ranked,
                route,
                adjacency,
                args.top_k,
                args.max_context_chars,
                composer_level,
                args.temporal_anchor_limit,
                args.graph_neighbor_limit,
            )
        else:
            sections = [
                (
                    f"[SOURCE {document.ref}]\n{document.text}",
                    document.covers,
                )
                for _score, document in ranked
            ]
            context, context_refs = pack_context_sections(
                sections, args.max_context_chars
            )

        answer = ""
        usage: dict[str, Any] = {}
        reader_ms = 0.0
        if reader_client is not None:
            reader_started = time.perf_counter()
            answer, usage = answer_with_reader(
                reader_client,
                args.chat_model,
                str(question["question"]),
                context,
                args.temperature,
                args.reader_max_tokens,
                args.reasoning_effort,
            )
            reader_ms = round((time.perf_counter() - reader_started) * 1000.0, 3)

        output_row = {
                "snapshot_sha256": snapshot_hash,
                "question_id": question["id"],
                "question_class": question.get("class", ""),
                "route": route,
                "system": system,
                "repetition": repetition,
                "retriever": args.retriever,
                "reader": args.reader,
                "status_policy": args.status_policy,
                "include_draft": args.include_draft,
                "top_k": args.top_k,
                "max_context_chars": args.max_context_chars,
                "reader_max_tokens": args.reader_max_tokens,
                "reasoning_effort": args.reasoning_effort,
                "temporal_anchor_limit": args.temporal_anchor_limit,
                "graph_neighbor_limit": args.graph_neighbor_limit,
                "retrieved": retrieved,
                "context_refs": context_refs,
                "context": context,
                "answer": answer,
                "usage": usage,
                "retrieval_ms": retrieval_ms,
                "reader_ms": reader_ms,
                "corpus_prepare_ms": corpus_prepare_ms,
                "ranker_init_ms": ranker_init_ms,
                "total_prepare_ms": total_prepare_ms,
            }
        output_rows.append(output_row)
        append_jsonl_row(output_path, output_row)
        progress_meta["completed_rows"] = len(output_rows)
        progress_meta["pending_rows"] = len(work_items) - len(output_rows)
        write_json(progress_meta_path, progress_meta)
        print(
            f"[{system}] {question['id']} rep={repetition} "
            f"retrieved={len(retrieved)} context={len(context)} chars",
            file=sys.stderr,
        )

    write_jsonl(output_path, output_rows)
    graph_path = Path(args.graph) if args.graph else None
    run_meta = {
        "status": "complete",
        "created_unix": int(time.time()),
        "output": str(output_path.resolve()),
        "snapshot": str(snapshot_path.resolve()),
        "snapshot_sha256": snapshot_hash,
        "graph": str(graph_path.resolve()) if graph_path else None,
        "graph_sha256": file_sha256(graph_path) if graph_path else None,
        "question_count": len(questions),
        "row_count": len(output_rows),
        "planned_rows": len(work_items),
        "completed_rows": len(output_rows),
        "pending_rows": 0,
        "resumed": args.resume,
        "systems": systems,
        "retriever": args.retriever,
        "embedding_model": args.embedding_model,
        "reader": args.reader,
        "chat_model": args.chat_model,
        "status_policy": args.status_policy,
        "include_draft": args.include_draft,
        "excluded_entry_ids": sorted(excluded_entry_ids),
        "top_k": args.top_k,
        "max_context_chars": args.max_context_chars,
        "reader_max_tokens": args.reader_max_tokens,
        "reasoning_effort": args.reasoning_effort,
        "temporal_anchor_limit": args.temporal_anchor_limit,
        "graph_neighbor_limit": args.graph_neighbor_limit,
        "repeat": args.repeat,
        "seed": args.seed,
        "corpus_prepare_ms": corpus_prepare_ms,
        "ranker_init_ms": ranker_init_ms,
        "total_prepare_ms": total_prepare_ms,
        "eligible_chunk_counts": {
            route: len(route_chunks)
            for route, route_chunks in sorted(chunks_by_route.items())
        },
    }
    write_json(progress_meta_path, run_meta)
    print(
        json_dumps(
            {
                "output": str(output_path.resolve()),
                "rows": len(output_rows),
                "questions": len(questions),
                "systems": systems,
                "snapshot_sha256": snapshot_hash,
                "corpus_prepare_ms": corpus_prepare_ms,
                "ranker_init_ms": ranker_init_ms,
                "total_prepare_ms": total_prepare_ms,
            },
            pretty=True,
        )
    )


def reference_matches(required: str, covered: str) -> bool:
    if "@" in required:
        return required == covered
    return normalize_entry_ref(covered) == required


def ref_set_satisfied(required: Sequence[str], covered: Sequence[str]) -> bool:
    return all(
        any(reference_matches(ref, candidate) for candidate in covered)
        for ref in required
    )


def best_ref_set_recall(
    ref_sets: Sequence[Sequence[str]], covered: Sequence[str]
) -> float | None:
    if not ref_sets:
        return None
    return max(
        sum(
            any(reference_matches(ref, candidate) for candidate in covered)
            for ref in ref_set
        )
        / len(ref_set)
        for ref_set in ref_sets
    )


def best_ref_set_reciprocal_rank(
    ref_sets: Sequence[Sequence[str]], covered_by_rank: Sequence[tuple[int, str]]
) -> float | None:
    if not ref_sets:
        return None
    values: list[float] = []
    for ref_set in ref_sets:
        matched_ranks: list[int] = []
        for ref in ref_set:
            ranks = [
                rank
                for rank, covered in covered_by_rank
                if reference_matches(ref, covered)
            ]
            if not ranks:
                matched_ranks = []
                break
            matched_ranks.append(min(ranks))
        values.append(1.0 / max(matched_ranks) if matched_ranks else 0.0)
    return max(values)


def revision_localization_precision(
    gold_refs: Sequence[str], covered: Sequence[str]
) -> float | None:
    exact_gold = {ref for ref in gold_refs if "@" in ref}
    if not exact_gold:
        return None
    gold_entries = {normalize_entry_ref(ref) for ref in exact_gold}
    candidates = {
        ref
        for ref in covered
        if "@" in ref and normalize_entry_ref(ref) in gold_entries
    }
    if not candidates:
        return 0.0
    return len(candidates & exact_gold) / len(candidates)


CANONICAL_CITATION_RE = re.compile(r"N\d{4,}@r\d{4}")
CITATION_LIKE_RE = re.compile(r"N\d+@r\d+")
BRACKET_GROUP_RE = re.compile(r"\[([^\]]+)\]")


def extract_citations(answer: str) -> list[str]:
    return [
        token
        for token in extract_citation_tokens(answer)
        if CANONICAL_CITATION_RE.fullmatch(token)
    ]


def extract_citation_tokens(answer: str) -> list[str]:
    return [
        token
        for group in BRACKET_GROUP_RE.findall(answer)
        for token in CITATION_LIKE_RE.findall(group)
    ]


def claim_has_citation(answer: str, claim: str) -> bool:
    for line in answer.splitlines():
        if normalized_contains(line, claim) and extract_citations(line):
            return True
    return False


def normalized_contains(text: str, claim: str) -> bool:
    normalized_text = " ".join(text.casefold().split())
    normalized_claim = " ".join(claim.casefold().split())
    return normalized_claim in normalized_text


def answer_abstained(answer: str) -> bool:
    markers = (
        "근거판정: 불충분",
        "자료에 없는 내용입니다",
        "근거가 부족",
        "insufficient evidence",
        "cannot determine",
        "not enough information",
    )
    return any(marker.casefold() in answer.casefold() for marker in markers)


def score_runs(args: argparse.Namespace) -> None:
    questions = read_jsonl(Path(args.questions))
    validate_questions(questions, Path(args.questions))
    by_id = {str(question["id"]): question for question in questions}
    runs = read_jsonl(Path(args.runs))
    details: list[dict[str, Any]] = []

    for run in runs:
        question_id = str(run.get("question_id", ""))
        if question_id not in by_id:
            raise ValueError(f"run references unknown question {question_id}")
        question = by_id[question_id]
        retrieved = run.get("retrieved", [])
        covered_by_rank: list[tuple[int, str]] = []
        for hit in retrieved:
            rank = int(hit.get("rank", 0))
            for covered in hit.get("covers", []):
                covered_by_rank.append((rank, str(covered)))

        primary_refs = question_primary_refs(question)
        acceptable_ref_sets = question_ref_sets(question)
        if not acceptable_ref_sets and primary_refs:
            acceptable_ref_sets = [primary_refs]
        all_acceptable_refs = sorted(
            {
                ref
                for ref_set in acceptable_ref_sets
                for ref in ref_set
            }
            | set(primary_refs)
        )
        covered_refs = [covered for _, covered in covered_by_rank]
        forbidden_refs = [str(ref) for ref in question.get("forbidden_refs", [])]
        primary_hits = [
            any(reference_matches(required, covered) for _, covered in covered_by_rank)
            for required in primary_refs
        ]
        reciprocal_ranks: list[float] = []
        for required in primary_refs:
            ranks = [
                rank
                for rank, covered in covered_by_rank
                if reference_matches(required, covered)
            ]
            reciprocal_ranks.append(1.0 / min(ranks) if ranks else 0.0)
        acceptable_source_success = (
            any(
                ref_set_satisfied(ref_set, covered_refs)
                for ref_set in acceptable_ref_sets
            )
            if acceptable_ref_sets
            else None
        )
        acceptable_source_recall = best_ref_set_recall(
            acceptable_ref_sets,
            covered_refs,
        )
        acceptable_mrr = best_ref_set_reciprocal_rank(
            acceptable_ref_sets,
            covered_by_rank,
        )
        entry_ref_sets = [
            sorted({normalize_entry_ref(ref) for ref in ref_set})
            for ref_set in acceptable_ref_sets
        ]
        covered_entries = sorted({normalize_entry_ref(ref) for ref in covered_refs})
        entry_recall = best_ref_set_recall(entry_ref_sets, covered_entries)
        forbidden_exposed = any(
            reference_matches(forbidden, covered)
            for forbidden in forbidden_refs
            for _, covered in covered_by_rank
        )
        context_refs = [str(ref) for ref in run.get("context_refs", [])]
        context_primary_hits = [
            any(reference_matches(required, covered) for covered in context_refs)
            for required in primary_refs
        ]
        acceptable_context_success = (
            any(
                ref_set_satisfied(ref_set, context_refs)
                for ref_set in acceptable_ref_sets
            )
            if acceptable_ref_sets
            else None
        )
        acceptable_context_recall = best_ref_set_recall(
            acceptable_ref_sets,
            context_refs,
        )
        forbidden_context_exposed = any(
            reference_matches(forbidden, covered)
            for forbidden in forbidden_refs
            for covered in context_refs
        )

        answer = str(run.get("answer", ""))
        required_claims = [str(claim) for claim in question.get("required_claims", [])]
        forbidden_claims = [str(claim) for claim in question.get("forbidden_claims", [])]
        required_claim_hits = [
            normalized_contains(answer, claim) for claim in required_claims
        ]
        stale_claim = any(normalized_contains(answer, claim) for claim in forbidden_claims)
        abstained = answer_abstained(answer) if answer else None
        expect_abstain = bool(question.get("expect_abstain", False))
        citation_tokens = extract_citation_tokens(answer)
        citations = extract_citations(answer)
        malformed_citations = [
            token
            for token in citation_tokens
            if not CANONICAL_CITATION_RE.fullmatch(token)
        ]
        valid_citations = [
            citation
            for citation in citations
            if any(reference_matches(citation, ref) for ref in context_refs)
        ]
        claim_citation_hits = [
            claim_has_citation(answer, claim) for claim in required_claims
        ]

        details.append(
            {
                "question_id": question_id,
                "system": run.get("system"),
                "repetition": run.get("repetition", 0),
                "required_source_recall": (
                    sum(primary_hits) / len(primary_hits) if primary_hits else None
                ),
                "exact_required_source_success": (
                    all(primary_hits) if primary_hits else None
                ),
                "context_source_recall": (
                    sum(context_primary_hits) / len(context_primary_hits)
                    if context_primary_hits
                    else None
                ),
                "exact_context_source_success": (
                    all(context_primary_hits) if context_primary_hits else None
                ),
                "acceptable_source_recall": acceptable_source_recall,
                "acceptable_source_success": acceptable_source_success,
                "acceptable_context_recall": acceptable_context_recall,
                "acceptable_context_success": acceptable_context_success,
                "entry_recall": entry_recall,
                "revision_localization_precision": revision_localization_precision(
                    all_acceptable_refs,
                    covered_refs,
                ),
                "context_revision_localization_precision": (
                    revision_localization_precision(
                        all_acceptable_refs,
                        context_refs,
                    )
                ),
                "mrr": (
                    sum(reciprocal_ranks) / len(reciprocal_ranks)
                    if reciprocal_ranks
                    else None
                ),
                "acceptable_mrr": acceptable_mrr,
                "forbidden_source_exposed": forbidden_exposed,
                "forbidden_context_exposed": forbidden_context_exposed,
                "required_claim_recall": (
                    sum(required_claim_hits) / len(required_claim_hits)
                    if answer and required_claim_hits
                    else None
                ),
                "required_claim_citation_recall": (
                    sum(claim_citation_hits) / len(claim_citation_hits)
                    if answer and claim_citation_hits
                    else None
                ),
                "citation_count": len(citation_tokens) if answer else None,
                "citation_present": (
                    bool(citation_tokens)
                    if answer and abstained is False
                    else None
                ),
                "malformed_citation_count": (
                    len(malformed_citations) if answer else None
                ),
                "valid_citation_precision": (
                    len(valid_citations) / len(citation_tokens)
                    if answer and citation_tokens
                    else None
                ),
                "stale_claim": stale_claim if answer else None,
                "abstained": abstained,
                "abstention_correct": (
                    abstained == expect_abstain if abstained is not None else None
                ),
                "retrieval_ms": run.get("retrieval_ms", 0.0),
                "reader_ms": run.get("reader_ms", 0.0),
                "reader_truncated": (
                    run.get("usage", {}).get("finish_reason") == "length"
                    if answer and isinstance(run.get("usage", {}), dict)
                    else None
                ),
                "corpus_prepare_ms": run.get("corpus_prepare_ms"),
                "ranker_init_ms": run.get("ranker_init_ms"),
                "total_prepare_ms": run.get("total_prepare_ms"),
            }
        )

    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for detail in details:
        grouped[str(detail["system"])].append(detail)

    def mean_present(rows: Sequence[dict[str, Any]], field: str) -> float | None:
        values = [row[field] for row in rows if row.get(field) is not None]
        if not values:
            return None
        return sum(float(value) for value in values) / len(values)

    summary: dict[str, Any] = {}
    for system, rows in sorted(grouped.items()):
        summary[system] = {
            "run_count": len(rows),
            "required_source_recall": mean_present(rows, "required_source_recall"),
            "exact_required_source_success": mean_present(
                rows, "exact_required_source_success"
            ),
            "context_source_recall": mean_present(rows, "context_source_recall"),
            "exact_context_source_success": mean_present(
                rows, "exact_context_source_success"
            ),
            "acceptable_source_recall": mean_present(
                rows, "acceptable_source_recall"
            ),
            "acceptable_source_success": mean_present(
                rows, "acceptable_source_success"
            ),
            "acceptable_context_recall": mean_present(
                rows, "acceptable_context_recall"
            ),
            "acceptable_context_success": mean_present(
                rows, "acceptable_context_success"
            ),
            "entry_recall": mean_present(rows, "entry_recall"),
            "revision_localization_precision": mean_present(
                rows, "revision_localization_precision"
            ),
            "context_revision_localization_precision": mean_present(
                rows, "context_revision_localization_precision"
            ),
            "mrr": mean_present(rows, "mrr"),
            "acceptable_mrr": mean_present(rows, "acceptable_mrr"),
            "forbidden_source_exposure_rate": mean_present(
                rows, "forbidden_source_exposed"
            ),
            "forbidden_context_exposure_rate": mean_present(
                rows, "forbidden_context_exposed"
            ),
            "required_claim_recall": mean_present(rows, "required_claim_recall"),
            "required_claim_citation_recall": mean_present(
                rows, "required_claim_citation_recall"
            ),
            "valid_citation_precision": mean_present(
                rows, "valid_citation_precision"
            ),
            "citation_presence_rate": mean_present(rows, "citation_present"),
            "mean_malformed_citation_count": mean_present(
                rows, "malformed_citation_count"
            ),
            "stale_claim_rate": mean_present(rows, "stale_claim"),
            "abstention_accuracy": mean_present(rows, "abstention_correct"),
            "reader_truncation_rate": mean_present(rows, "reader_truncated"),
            "corpus_prepare_ms": mean_present(rows, "corpus_prepare_ms"),
            "ranker_init_ms": mean_present(rows, "ranker_init_ms"),
            "total_prepare_ms": mean_present(rows, "total_prepare_ms"),
            "mean_retrieval_ms": mean_present(rows, "retrieval_ms"),
            "mean_reader_ms": mean_present(rows, "reader_ms"),
        }

    report = {
        "runs": str(Path(args.runs).resolve()),
        "questions": str(Path(args.questions).resolve()),
        "summary": summary,
        "details": details,
    }
    write_json(Path(args.output), report)
    print(json_dumps(summary, pretty=True))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    export_parser = subparsers.add_parser(
        "export", help="export a read-only frozen vault snapshot"
    )
    export_parser.add_argument("--vault", required=True)
    export_parser.add_argument("--output", required=True)
    export_parser.add_argument("--graph-output")
    export_parser.add_argument("--elf", default="elf")
    export_parser.set_defaults(func=export_snapshot)

    run_parser = subparsers.add_parser("run", help="run retrieval/reader systems")
    run_parser.add_argument("--snapshot", required=True)
    run_parser.add_argument("--graph")
    run_parser.add_argument("--questions", required=True)
    run_parser.add_argument("--output", required=True)
    run_parser.add_argument("--systems", default="flat,strong,c1,c2,c3,c4")
    run_parser.add_argument("--retriever", choices=("lexical", "openai"), default="lexical")
    run_parser.add_argument("--reader", choices=("none", "openai"), default="none")
    run_parser.add_argument("--embedding-endpoint")
    run_parser.add_argument("--embedding-model")
    run_parser.add_argument("--embedding-cache")
    run_parser.add_argument("--chat-endpoint")
    run_parser.add_argument("--chat-model")
    run_parser.add_argument("--api-key-env", default="RAG_COMPARE_API_KEY")
    run_parser.add_argument("--timeout", type=float, default=120.0)
    run_parser.add_argument("--temperature", type=float, default=0.0)
    run_parser.add_argument("--reader-max-tokens", type=int, default=512)
    run_parser.add_argument(
        "--reasoning-effort",
        choices=("none", "low", "medium", "high"),
        default="none",
        help="OpenAI-compatible reasoning control; none preserves answer-token budget",
    )
    run_parser.add_argument("--top-k", type=int, default=5)
    run_parser.add_argument("--max-context-chars", type=int, default=24000)
    run_parser.add_argument(
        "--temporal-anchor-limit",
        type=int,
        default=1,
        help="number of highest-ranked entry anchors receiving successor/base context",
    )
    run_parser.add_argument(
        "--graph-neighbor-limit",
        type=int,
        default=1,
        help="maximum authored graph-neighbor heads added to native context",
    )
    run_parser.add_argument(
        "--status-policy",
        choices=("route-aware", "all"),
        default="route-aware",
        help="route-aware excludes archived except for history/rationale routes",
    )
    run_parser.add_argument(
        "--include-draft",
        action="store_true",
        help="include draft entries when using the route-aware status policy",
    )
    run_parser.add_argument(
        "--exclude-entry",
        action="append",
        default=[],
        help="entry ID to exclude from the run; repeat for multiple entries",
    )
    run_parser.add_argument("--repeat", type=int, default=1)
    run_parser.add_argument("--seed", type=int, default=0)
    run_parser.add_argument(
        "--resume",
        action="store_true",
        help="skip completed question/system/repetition rows in an existing output",
    )
    run_parser.set_defaults(func=run_comparison)

    score_parser = subparsers.add_parser("score", help="score run JSONL")
    score_parser.add_argument("--runs", required=True)
    score_parser.add_argument("--questions", required=True)
    score_parser.add_argument("--output", required=True)
    score_parser.set_defaults(func=score_runs)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        if getattr(args, "top_k", 1) <= 0:
            raise ValueError("top-k must be positive")
        if getattr(args, "repeat", 1) <= 0:
            raise ValueError("repeat must be positive")
        if getattr(args, "max_context_chars", 1) <= 0:
            raise ValueError("max-context-chars must be positive")
        if getattr(args, "reader_max_tokens", 1) <= 0:
            raise ValueError("reader-max-tokens must be positive")
        if getattr(args, "temporal_anchor_limit", 0) < 0:
            raise ValueError("temporal-anchor-limit must be non-negative")
        if getattr(args, "graph_neighbor_limit", 0) < 0:
            raise ValueError("graph-neighbor-limit must be non-negative")
        args.func(args)
        return 0
    except (ValueError, RuntimeError, OSError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
