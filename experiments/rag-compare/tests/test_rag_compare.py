from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "rag_compare.py"
FIXTURES = ROOT / "fixtures"


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


if __name__ == "__main__":
    unittest.main()
