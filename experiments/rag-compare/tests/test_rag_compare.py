from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "rag_compare.py"
FIXTURES = ROOT / "fixtures"
CALIBRATION = ROOT / "calibration" / "questions.jsonl"
PILOT = ROOT / "pilot" / "questions.jsonl"
BLIND_SCRIPT = ROOT / "pilot" / "blind_review.py"
ADJUDICATION = ROOT / "pilot" / "ADJUDICATION.jsonl"


def load_module():
    spec = importlib.util.spec_from_file_location("rag_compare", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load rag_compare.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class RagCompareTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.module = load_module()
        cls.chunks = cls.module.load_chunks(FIXTURES / "snapshot.jsonl")

    def test_flat_document_covers_every_revision(self) -> None:
        documents = self.module.build_flat_documents(self.chunks)
        deployment = next(document for document in documents if document.entry_id == "N9001")
        self.assertEqual(
            deployment.covers,
            ("N9001@r0000", "N9001@r0001", "N9001@r0002"),
        )

    def test_native_context_marks_matched_history_and_head(self) -> None:
        documents = self.module.build_strong_documents(self.chunks)
        matched = next(document for document in documents if document.ref == "N9001@r0001")
        context, refs = self.module.compose_native_context(
            self.chunks,
            [(1.0, matched)],
            "rationale",
            {},
            5,
        )
        self.assertIn("[MATCHED_HISTORICAL N9001@r0001]", context)
        self.assertIn("[SUCCESSOR N9001@r0002]", context)
        self.assertIn("N9001@r0002", refs)

    def test_composer_ablation_levels_add_one_feature_family(self) -> None:
        documents = self.module.build_strong_documents(self.chunks)
        matched = next(document for document in documents if document.ref == "N9001@r0001")
        common = (self.chunks, [(1.0, matched)], "rationale", {"N9001": {"N9002"}}, 5)
        c1_context, c1_refs = self.module.compose_native_context(
            *common, composer_level="c1"
        )
        c2_context, c2_refs = self.module.compose_native_context(
            *common, composer_level="c2"
        )
        c3_context, c3_refs = self.module.compose_native_context(
            *common, composer_level="c3"
        )
        c4_context, c4_refs = self.module.compose_native_context(
            *common, composer_level="c4"
        )
        self.assertEqual(c1_refs, ["N9001@r0001"])
        self.assertEqual(c2_refs, ["N9001@r0001", "N9001@r0002"])
        self.assertIn("N9001@r0000", c3_refs)
        self.assertNotIn("N9002@r0001", c3_refs)
        self.assertIn("N9002@r0001", c4_refs)
        self.assertNotIn("CURRENT_HEAD", c1_context)
        self.assertIn("CURRENT_HEAD", c2_context)
        self.assertIn("BASE_CONTEXT", c3_context)
        self.assertIn("AUTHORED_NEIGHBOR_HEAD", c4_context)

    def test_temporal_expansion_is_limited_to_highest_anchor(self) -> None:
        documents = self.module.build_strong_documents(self.chunks)
        first = next(document for document in documents if document.ref == "N9001@r0001")
        second = next(document for document in documents if document.ref == "N9002@r0001")
        _context, refs = self.module.compose_native_context(
            self.chunks,
            [(1.0, first), (0.9, second)],
            "rationale",
            {},
            5,
            composer_level="c3",
            temporal_anchor_limit=1,
        )
        self.assertIn("N9001@r0000", refs)
        self.assertNotIn("N9002@r0000", refs)

    def test_context_budget_does_not_report_omitted_sources(self) -> None:
        context, refs = self.module.pack_context_sections(
            [
                ("[SOURCE N9001@r0000]\n" + "a" * 20, ("N9001@r0000",)),
                ("[SOURCE N9002@r0000]\n" + "b" * 20, ("N9002@r0000",)),
            ],
            50,
        )
        self.assertIn("N9001@r0000", context)
        self.assertNotIn("N9002@r0000", context)
        self.assertEqual(refs, ["N9001@r0000"])

    def test_route_aware_status_policy(self) -> None:
        stable = self.chunks[0]
        archived = self.module.Chunk(
            entry_id="N9100",
            rev_id="r0000",
            title="archived",
            text="old",
            status="archived",
            created="",
            updated="",
            baseline=None,
            links=(),
            tags=(),
            ordinal=0,
            is_head=True,
        )
        draft = self.module.Chunk(
            entry_id="N9101",
            rev_id="r0000",
            title="draft",
            text="draft",
            status="draft",
            created="",
            updated="",
            baseline=None,
            links=(),
            tags=(),
            ordinal=0,
            is_head=True,
        )
        chunks = [stable, archived, draft]
        current = self.module.eligible_chunks(
            chunks, "current", "route-aware", False
        )
        history = self.module.eligible_chunks(
            chunks, "history", "route-aware", False
        )
        with_draft = self.module.eligible_chunks(
            chunks, "current", "route-aware", True
        )
        self.assertEqual([chunk.entry_id for chunk in current], [stable.entry_id])
        self.assertEqual(
            [chunk.entry_id for chunk in history],
            [stable.entry_id, archived.entry_id],
        )
        self.assertEqual(
            [chunk.entry_id for chunk in with_draft],
            [stable.entry_id, draft.entry_id],
        )

    def test_alternative_gold_sets_and_revision_localization(self) -> None:
        question = {
            "acceptable_ref_sets": [
                ["N9001@r0001"],
                ["N9002@r0001"],
            ]
        }
        ref_sets = self.module.question_ref_sets(question)
        self.assertTrue(
            any(
                self.module.ref_set_satisfied(ref_set, ["N9002@r0001"])
                for ref_set in ref_sets
            )
        )
        self.assertEqual(
            self.module.revision_localization_precision(
                ["N9001@r0002"],
                ["N9001@r0000", "N9001@r0001", "N9001@r0002"],
            ),
            1 / 3,
        )

    def test_citation_parser_supports_long_ids_and_flags_malformed_refs(self) -> None:
        answer = "[N0065@r0001, N10000@r0002] malformed [N0065@r001]"
        self.assertEqual(
            self.module.extract_citations(answer),
            ["N0065@r0001", "N10000@r0002"],
        )
        self.assertEqual(
            self.module.extract_citation_tokens(answer),
            ["N0065@r0001", "N10000@r0002", "N0065@r001"],
        )

    def test_offline_cli_run_and_score(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            run_path = Path(temp_dir) / "runs.jsonl"
            score_path = Path(temp_dir) / "score.json"
            run_command = [
                    sys.executable,
                    str(SCRIPT),
                    "run",
                    "--snapshot",
                    str(FIXTURES / "snapshot.jsonl"),
                    "--questions",
                    str(FIXTURES / "questions.jsonl"),
                    "--output",
                    str(run_path),
                    "--systems",
                    "flat,strong,native",
                    "--retriever",
                    "lexical",
                    "--reader",
                    "none",
                    "--top-k",
                    "2",
                ]
            run = subprocess.run(
                run_command,
                check=False,
                capture_output=True,
                text=True,
                encoding="utf-8",
            )
            self.assertEqual(run.returncode, 0, run.stderr)
            row_count = len(run_path.read_text(encoding="utf-8").splitlines())
            resumed = subprocess.run(
                [*run_command, "--resume"],
                check=False,
                capture_output=True,
                text=True,
                encoding="utf-8",
            )
            self.assertEqual(resumed.returncode, 0, resumed.stderr)
            self.assertEqual(
                len(run_path.read_text(encoding="utf-8").splitlines()),
                row_count,
            )
            run_meta_path = run_path.with_suffix(run_path.suffix + ".meta.json")
            run_meta = json.loads(run_meta_path.read_text(encoding="utf-8"))
            self.assertIn("corpus_prepare_ms", run_meta)
            self.assertEqual(run_meta["status_policy"], "route-aware")
            self.assertEqual(run_meta["status"], "complete")
            score = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "score",
                    "--runs",
                    str(run_path),
                    "--questions",
                    str(FIXTURES / "questions.jsonl"),
                    "--output",
                    str(score_path),
                ],
                check=False,
                capture_output=True,
                text=True,
                encoding="utf-8",
            )
            self.assertEqual(score.returncode, 0, score.stderr)
            report = json.loads(score_path.read_text(encoding="utf-8"))
            self.assertEqual(set(report["summary"]), {"flat", "strong", "native"})
            self.assertGreaterEqual(
                report["summary"]["strong"]["required_source_recall"], 0.65
            )
            self.assertEqual(
                report["summary"]["native"]["context_source_recall"], 1.0
            )

    def test_pilot_is_balanced_preregistered_and_distinct_from_calibration(self) -> None:
        pilot = self.module.read_jsonl(PILOT)
        calibration = self.module.read_jsonl(CALIBRATION)

        expected_classes = {
            "current_decision",
            "decision_rationale",
            "superseded_avoidance",
            "interrupted_work",
            "handoff",
            "absent_evidence",
        }
        self.assertEqual(len(pilot), 30)
        self.assertEqual(len({question["id"] for question in pilot}), 30)
        self.assertEqual(
            Counter(question["class"] for question in pilot),
            Counter({question_class: 5 for question_class in expected_classes}),
        )
        self.assertTrue(all(question.get("human_rubric") for question in pilot))

        calibration_questions = {question["question"] for question in calibration}
        self.assertTrue(
            all(question["question"] not in calibration_questions for question in pilot)
        )
        calibration_signatures = {
            (
                tuple(sorted(question.get("primary_refs", []))),
                tuple(sorted(question.get("required_claims", []))),
            )
            for question in calibration
            if not question.get("expect_abstain", False)
        }
        for question in pilot:
            if question.get("expect_abstain", False):
                self.assertEqual(question["class"], "absent_evidence")
                self.assertEqual(question.get("primary_refs"), [])
                continue
            signature = (
                tuple(sorted(question.get("primary_refs", []))),
                tuple(sorted(question.get("required_claims", []))),
            )
            self.assertNotIn(signature, calibration_signatures, question["id"])

    def test_blind_review_hides_systems_and_requires_complete_verdicts(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            temp = Path(temp_dir)
            runs = temp / "runs.jsonl"
            questions = temp / "questions.jsonl"
            review = temp / "review.jsonl"
            key = temp / "key.json"
            adjudications = temp / "adjudications.jsonl"
            revealed = temp / "revealed.json"

            questions.write_text(
                json.dumps(
                    {
                        "id": "P900",
                        "class": "current_decision",
                        "question": "question",
                        "human_rubric": "rubric",
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            runs.write_text(
                "\n".join(
                    json.dumps(row)
                    for row in (
                        {"question_id": "P900", "system": "strong", "repetition": 0, "answer": "answer one"},
                        {"question_id": "P900", "system": "c3", "repetition": 0, "answer": "answer two"},
                    )
                )
                + "\n",
                encoding="utf-8",
            )
            prepared = subprocess.run(
                [
                    sys.executable,
                    str(BLIND_SCRIPT),
                    "prepare",
                    "--runs",
                    str(runs),
                    "--questions",
                    str(questions),
                    "--review",
                    str(review),
                    "--key",
                    str(key),
                ],
                check=False,
                capture_output=True,
                text=True,
                encoding="utf-8",
            )
            self.assertEqual(prepared.returncode, 0, prepared.stderr)
            review_row = json.loads(review.read_text(encoding="utf-8"))
            self.assertNotIn("system", review_row)
            self.assertEqual({review_row["A"], review_row["B"]}, {"answer one", "answer two"})

            adjudications.write_text(
                json.dumps(
                    {
                        "question_id": "P900",
                        "A": {"pass": True, "reason": None},
                        "B": {"pass": False, "reason": "incomplete"},
                    }
                )
                + "\n",
                encoding="utf-8",
            )
            result = subprocess.run(
                [
                    sys.executable,
                    str(BLIND_SCRIPT),
                    "reveal",
                    "--adjudications",
                    str(adjudications),
                    "--key",
                    str(key),
                    "--questions",
                    str(questions),
                    "--output",
                    str(revealed),
                ],
                check=False,
                capture_output=True,
                text=True,
                encoding="utf-8",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(revealed.read_text(encoding="utf-8"))
            self.assertEqual(report["summary"]["strong"]["total"], 1)
            self.assertEqual(report["summary"]["c3"]["total"], 1)

    def test_committed_pilot_adjudication_is_complete(self) -> None:
        questions = self.module.read_jsonl(PILOT)
        adjudications = self.module.read_jsonl(ADJUDICATION)
        self.assertEqual(
            {row["question_id"] for row in adjudications},
            {row["id"] for row in questions},
        )
        self.assertEqual(len(adjudications), 30)
        valid_reasons = {
            "incorrect",
            "incomplete",
            "stale",
            "unsupported",
            "citation_missing",
            "citation_invalid",
            "failed_to_abstain",
        }
        for row in adjudications:
            for label in ("A", "B"):
                verdict = row[label]
                self.assertIsInstance(verdict["pass"], bool)
                if verdict["pass"]:
                    self.assertIsNone(verdict["reason"])
                else:
                    self.assertIn(verdict["reason"], valid_reasons)


if __name__ == "__main__":
    unittest.main()
