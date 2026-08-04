from __future__ import annotations

import argparse
import hashlib
import json
import secrets
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any


FAILURE_REASONS = {
    "incorrect",
    "incomplete",
    "stale",
    "unsupported",
    "citation_missing",
    "citation_invalid",
    "failed_to_abstain",
}


def read_jsonl(path: Path) -> list[dict[str, Any]]:
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "".join(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n" for row in rows),
        encoding="utf-8",
    )


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def prepare_blind_review(
    runs_path: Path,
    questions_path: Path,
    review_path: Path,
    key_path: Path,
) -> dict[str, Any]:
    if review_path.exists() or key_path.exists():
        raise ValueError("review or key output already exists")

    questions = read_jsonl(questions_path)
    runs = read_jsonl(runs_path)
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in runs:
        grouped[str(row["question_id"])].append(row)

    review_rows: list[dict[str, Any]] = []
    mapping: dict[str, dict[str, str]] = {}
    for question in questions:
        question_id = str(question["id"])
        candidates = grouped.get(question_id, [])
        systems = {str(row["system"]) for row in candidates}
        if len(candidates) != 2 or systems != {"strong", "c3"}:
            raise ValueError(f"{question_id} does not have exactly one strong/c3 pair")
        if {int(row.get("repetition", 0)) for row in candidates} != {0}:
            raise ValueError(f"{question_id} contains an unexpected repetition")

        ordered = candidates if secrets.randbits(1) == 0 else list(reversed(candidates))
        mapping[question_id] = {
            "A": str(ordered[0]["system"]),
            "B": str(ordered[1]["system"]),
        }
        review_rows.append(
            {
                "question_id": question_id,
                "class": question["class"],
                "question": question["question"],
                "human_rubric": question["human_rubric"],
                "A": ordered[0]["answer"],
                "B": ordered[1]["answer"],
            }
        )

    write_jsonl(review_path, review_rows)
    key = {
        "run_sha256": file_sha256(runs_path),
        "questions_sha256": file_sha256(questions_path),
        "mapping": mapping,
    }
    key_path.parent.mkdir(parents=True, exist_ok=True)
    key_path.write_text(json.dumps(key, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return {
        "rows": len(review_rows),
        "run_sha256": key["run_sha256"],
        "questions_sha256": key["questions_sha256"],
        "review_sha256": file_sha256(review_path),
        "key_sha256": file_sha256(key_path),
    }


def reveal_adjudication(
    adjudications_path: Path,
    key_path: Path,
    questions_path: Path,
    output_path: Path,
) -> dict[str, Any]:
    questions = {str(row["id"]): row for row in read_jsonl(questions_path)}
    adjudications = read_jsonl(adjudications_path)
    key = json.loads(key_path.read_text(encoding="utf-8"))
    mapping = key["mapping"]

    if file_sha256(questions_path) != key["questions_sha256"]:
        raise ValueError("question file hash no longer matches the blind key")
    if len(adjudications) != len(questions):
        raise ValueError("every question must be adjudicated before reveal")
    if len({str(row["question_id"]) for row in adjudications}) != len(questions):
        raise ValueError("adjudication question IDs are incomplete or duplicated")

    items: list[dict[str, Any]] = []
    summary: dict[str, dict[str, Any]] = {}
    for row in adjudications:
        question_id = str(row["question_id"])
        if question_id not in questions or question_id not in mapping:
            raise ValueError(f"unexpected adjudication question: {question_id}")
        question_class = str(questions[question_id]["class"])
        for label in ("A", "B"):
            verdict = row.get(label)
            if not isinstance(verdict, dict) or not isinstance(verdict.get("pass"), bool):
                raise ValueError(f"{question_id}/{label} has no boolean pass verdict")
            reason = verdict.get("reason")
            if verdict["pass"] and reason is not None:
                raise ValueError(f"{question_id}/{label} passed but has a failure reason")
            if not verdict["pass"] and reason not in FAILURE_REASONS:
                raise ValueError(f"{question_id}/{label} has an invalid failure reason")
            system = str(mapping[question_id][label])
            items.append(
                {
                    "question_id": question_id,
                    "class": question_class,
                    "system": system,
                    "pass": verdict["pass"],
                    "reason": reason,
                    "note": verdict.get("note", ""),
                }
            )

    for system in ("strong", "c3"):
        system_items = [item for item in items if item["system"] == system]
        class_counts: dict[str, dict[str, int]] = {}
        for question_class in sorted({item["class"] for item in system_items}):
            class_items = [item for item in system_items if item["class"] == question_class]
            class_counts[question_class] = {
                "passed": sum(bool(item["pass"]) for item in class_items),
                "total": len(class_items),
            }
        failures = Counter(
            str(item["reason"]) for item in system_items if not item["pass"]
        )
        summary[system] = {
            "passed": sum(bool(item["pass"]) for item in system_items),
            "total": len(system_items),
            "classes": class_counts,
            "failures_by_reason": dict(sorted(failures.items())),
        }

    report = {
        "run_sha256": key["run_sha256"],
        "questions_sha256": key["questions_sha256"],
        "adjudications_sha256": file_sha256(adjudications_path),
        "summary": summary,
        "items": items,
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    return report


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Prepare and reveal a blinded Strong/C3 review")
    subparsers = parser.add_subparsers(dest="command", required=True)

    prepare = subparsers.add_parser("prepare")
    prepare.add_argument("--runs", required=True, type=Path)
    prepare.add_argument("--questions", required=True, type=Path)
    prepare.add_argument("--review", required=True, type=Path)
    prepare.add_argument("--key", required=True, type=Path)

    reveal = subparsers.add_parser("reveal")
    reveal.add_argument("--adjudications", required=True, type=Path)
    reveal.add_argument("--key", required=True, type=Path)
    reveal.add_argument("--questions", required=True, type=Path)
    reveal.add_argument("--output", required=True, type=Path)
    return parser


def main() -> None:
    args = build_parser().parse_args()
    if args.command == "prepare":
        result = prepare_blind_review(args.runs, args.questions, args.review, args.key)
    else:
        result = reveal_adjudication(args.adjudications, args.key, args.questions, args.output)
    printable = result if args.command == "prepare" else {"summary": result["summary"]}
    print(json.dumps(printable, ensure_ascii=False, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
