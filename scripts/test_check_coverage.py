"""Regression tests for the coverage gate, including real Git diffs."""

import contextlib
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

import check_coverage as gate


class SourceSelectionTests(unittest.TestCase):
    def test_runtime_sources_are_included(self):
        for name in ["crates/news-core/src/lib.rs", "crates/worker/src/adapter.rs", "crates/news-server/src/main.rs", "frontend/src/main.ts", "frontend/src/App.vue", "sites/src/worker.ts"]:
            with self.subTest(name=name):
                self.assertTrue(gate.production_source(name))

    def test_nonproduction_files_are_excluded(self):
        for name in ["README.md", "scripts/check_coverage.py", "frontend/vite.config.ts", "frontend/src/env.d.ts", "frontend/src/App.test.ts", "sites/src/worker.spec.ts", "frontend/src/__tests__/helpers.ts", "crates/news-core/src/tests.rs", "crates/news-core/tests/integration.rs", "frontend/node_modules/package/index.js"]:
            with self.subTest(name=name):
                self.assertFalse(gate.production_source(name))


class LcovTests(unittest.TestCase):
    def setUp(self):
        self.root = Path("/tmp/coverage-repo")

    def parse(self, content, source_root=None):
        return gate.parse_lcov(content, source_root or self.root, self.root)

    def test_absolute_and_relative_paths(self):
        self.assertEqual(self.parse("SF:/tmp/coverage-repo/crates/core/src/lib.rs\nDA:2,1\nend_of_record\n"), {"crates/core/src/lib.rs": {2: 1}})
        self.assertEqual(self.parse("SF:src/main.ts\nDA:2,0\nend_of_record\n", self.root / "frontend"), {"frontend/src/main.ts": {2: 0}})

    def test_duplicate_records_merge_without_inflating_denominator(self):
        result = self.parse("TN:first\nSF:frontend/src/main.ts\nDA:2,0\nLF:1\nLH:0\nend_of_record\nTN:second\nSF:frontend/src/main.ts\nDA:2,3\nDA:3,0,checksum\nLF:2\nLH:1\nend_of_record\n")
        self.assertEqual(result, {"frontend/src/main.ts": {2: 3, 3: 0}})
        self.assertEqual(gate.merge_reports([result, result]), result)

    def test_llvm_function_summary_exceeds_physical_da_lines(self):
        # Regression: real LLVM exports had 243 DA lines but LF/LH of 257/257;
        # the native server had 271 covered DA lines but LF/LH of 354/326.
        report = self.parse("SF:crates/core/src/lib.rs\nDA:1,1\nDA:2,1\nLF:4\nLH:3\nend_of_record\n")
        result = gate.evaluate(set(report), {"crates/core/src/lib.rs": {1, 2}}, report)
        self.assertEqual((result.total_covered, result.total_lines), (3, 4))
        self.assertEqual((result.changed_covered, result.changed_lines), (2, 2))
        self.assertEqual(len(result.failures(95, 95)), 1)

    def test_duplicate_da_and_reports_do_not_inflate_summary(self):
        report = self.parse("SF:crates/core/src/lib.rs\nDA:1,0\nDA:1,2\nDA:2,0\nLF:3\nLH:1\nend_of_record\n")
        merged = gate.merge_reports([report, report])
        result = gate.evaluate(set(merged), {}, merged)
        self.assertEqual((result.total_covered, result.total_lines), (1, 3))
        self.assertEqual(merged["crates/core/src/lib.rs"], {1: 2, 2: 0})

    def test_summary_retains_uncovered_overlapping_function_line(self):
        report = self.parse("SF:crates/core/src/lib.rs\nDA:1,2\nDA:2,1\nLF:2\nLH:1\nend_of_record\n")
        self.assertEqual(report["crates/core/src/lib.rs"].totals(), (1, 2))

    def test_complementary_records_merge_only_proven_physical_hits(self):
        first = self.parse("SF:crates/core/src/lib.rs\nDA:1,2\nDA:2,0\nLF:4\nLH:2\nend_of_record\n")
        second = self.parse("SF:crates/core/src/lib.rs\nDA:1,0\nDA:2,1\nLF:4\nLH:2\nend_of_record\n")
        merged = gate.merge_reports([first, second])
        self.assertEqual(merged["crates/core/src/lib.rs"].totals(), (3, 4))

    def test_llvm_summary_only_without_da_is_rejected(self):
        with self.assertRaises(gate.CoverageError):
            self.parse("SF:crates/core/src/lib.rs\nLF:257\nLH:257\nend_of_record\n")

    def test_impossible_summary_is_rejected(self):
        for content in [
            "SF:a\nDA:1,1\nDA:2,1\nLF:1\nLH:1\nend_of_record\n",
            "SF:a\nDA:1,1\nLF:1\nLH:2\nend_of_record\n",
            "SF:a\nDA:1,0\nLF:1\nLH:1\nend_of_record\n",
        ]:
            with self.subTest(content=content), self.assertRaises(gate.CoverageError):
                self.parse(content)

    def test_explicit_zero_executable_record_is_preserved(self):
        self.assertEqual(self.parse("SF:frontend/src/types.ts\nLF:0\nLH:0\nend_of_record\n"), {"frontend/src/types.ts": {}})

    def test_malformed_or_incomplete_reports_fail_closed(self):
        invalid = ["SF:frontend/src/worker.ts\nend_of_record\n", "", "TN:empty\n", "SF:\nend_of_record\n", "SF:../escape.ts\nend_of_record\n", "DA:1,1\n", "end_of_record\n", "SF:a\nDA:1,no\nend_of_record\n", "SF:a\nDA:0,1\nend_of_record\n", "SF:a\nDA:1,-1\nend_of_record\n", "SF:a\nDA:1\nend_of_record\n", "SF:a\n", "SF:a\nSF:b\n", "LF:2\n", "SF:a\nLF:-1\nend_of_record\n", "SF:a\nLF:x\nend_of_record\n", "SF:a\nDA:1,1\nLF:2\nend_of_record\n", "SF:a\nDA:1,0\nLH:1\nend_of_record\n"]
        for content in invalid:
            with self.subTest(content=content), self.assertRaises(gate.CoverageError):
                self.parse(content)


class EvaluationTests(unittest.TestCase):
    def test_exact_thresholds_pass(self):
        result = gate.CoverageResult(95, 100, 95, 100, (), {})
        self.assertEqual(result.failures(95, 95), [])

    def test_rounding_cannot_bypass_threshold(self):
        result = gate.CoverageResult(94999, 100000, 94999, 100000, (), {})
        self.assertEqual(len(result.failures(95, 95)), 2)

    def test_total_and_changed_95_percent_gates_are_independent(self):
        self.assertEqual(len(gate.CoverageResult(94, 100, 95, 100, (), {}).failures(95, 95)), 1)
        self.assertEqual(len(gate.CoverageResult(95, 100, 94, 100, (), {}).failures(95, 95)), 1)

    def test_missing_unchanged_and_changed_sources_fail(self):
        result = gate.evaluate({"a", "b", "c"}, {"a": {1}, "c": {1}}, {"a": {1: 1}})
        self.assertEqual(result.missing_files, ("b", "c"))
        self.assertEqual(len(result.failures(95, 95)), 1)

    def test_no_executable_changed_lines_is_allowed_but_empty_total_is_not(self):
        self.assertEqual(gate.CoverageResult(1, 1, 0, 0, (), {}).failures(95, 95), [])
        self.assertTrue(gate.CoverageResult(0, 0, 0, 0, (), {}).failures(95, 95))
        self.assertIn("n/a", gate.percentage(0, 0))

    def test_changed_denominator_uses_instrumented_lines(self):
        result = gate.evaluate({"a"}, {"a": {1, 2, 3, 4}}, {"a": {2: 1, 4: 0, 6: 1}, "deleted": {1: 0}})
        self.assertEqual((result.total_covered, result.total_lines), (2, 3))
        self.assertEqual((result.changed_covered, result.changed_lines), (1, 2))
        self.assertEqual(result.uncovered_changed, {"a": [4]})


class GitTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git("init", "-q")
        self.git("config", "user.email", "ci-test@example.invalid")
        self.git("config", "user.name", "Coverage Tests")
        self.write("README.md", "initial\n")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").strip()

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.root), *args], check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True).stdout

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def commit(self):
        self.git("add", "--all")
        self.git("commit", "-qm", "test")

    def changes(self, working_tree=False):
        return gate.changed_lines(self.root, self.base, None if working_tree else "HEAD")

    def test_added_source_counts_all_lines(self):
        self.write("frontend/src/main.ts", "export const first = 1\nexport const second = 2\n")
        self.commit()
        self.assertEqual(self.changes(), {"frontend/src/main.ts": {1, 2}})
        self.assertEqual(gate.source_inventory(self.root, "HEAD"), {"frontend/src/main.ts"})

    def test_modified_deleted_and_renamed_lines(self):
        self.write("frontend/src/old.ts", "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n")
        self.write("frontend/src/deleted.ts", "remove\n")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").strip()
        self.git("mv", "frontend/src/old.ts", "frontend/src/renamed.ts")
        self.write("frontend/src/renamed.ts", "a\nb\nc\nchanged\ne\nf\ng\nh\ni\nj\n")
        self.git("rm", "frontend/src/deleted.ts")
        self.commit()
        self.assertEqual(self.changes(), {"frontend/src/renamed.ts": {4}})

    def test_unchanged_rename_has_no_changed_lines(self):
        self.write("frontend/src/old.ts", "export const a = 1\n")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").strip()
        self.git("mv", "frontend/src/old.ts", "frontend/src/new.ts")
        self.commit()
        self.assertEqual(self.changes(), {"frontend/src/new.ts": set()})

    def test_deletion_only_has_no_added_lines(self):
        self.write("frontend/src/main.ts", "a\nb\nc\n")
        self.commit()
        self.base = self.git("rev-parse", "HEAD").strip()
        self.write("frontend/src/main.ts", "a\nc\n")
        self.commit()
        self.assertEqual(self.changes(), {"frontend/src/main.ts": set()})

    def test_working_tree_includes_staged_unstaged_and_untracked_but_not_tests(self):
        self.write("frontend/src/staged.ts", "one\n")
        self.git("add", "frontend/src/staged.ts")
        self.write("frontend/src/staged.ts", "one\ntwo\n")
        self.write("sites/src/worker.ts", "worker\n")
        self.write("sites/src/worker.test.ts", "test\n")
        self.assertEqual(self.changes(True), {"frontend/src/staged.ts": {1, 2}, "sites/src/worker.ts": {1}})
        self.assertEqual(self.changes(), {})
        self.assertEqual(gate.source_inventory(self.root, None), {"frontend/src/staged.ts", "sites/src/worker.ts"})

    def test_spaces_tabs_unicode_and_pathspec_characters(self):
        name = "frontend/src/space\tü [*].ts"
        self.write(name, "one\ntwo\n")
        self.commit()
        self.assertEqual(self.changes(), {name: {1, 2}})

    def test_binary_source_fails(self):
        self.write("frontend/src/binary.ts", "binary\0data\n")
        with self.assertRaises(gate.CoverageError):
            self.changes(True)
        self.commit()
        with self.assertRaises(gate.CoverageError):
            self.changes()

    def test_invalid_base_fails(self):
        with self.assertRaises(gate.CoverageError):
            gate.revision(self.root, "nonexistent")

    def test_empty_tree_can_be_initial_base(self):
        empty = subprocess.run(["git", "-C", str(self.root), "hash-object", "-t", "tree", "--stdin"], input="", text=True, check=True, stdout=subprocess.PIPE).stdout.strip()
        self.assertEqual(gate.revision(self.root, empty, tree=True), empty)

    def test_cli_defaults_to_95_percent_for_both_gates(self):
        name = "frontend/src/main.ts"
        self.write(name, "export const value = 1\n" * 20)
        self.commit()
        args = ["--repo", str(self.root), "--base", self.base, "--report", "coverage.lcov", ".", "--json-output", str(self.root / "summary.json")]
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            for covered, expected_exit in [(19, 0), (18, 1)]:
                records = "".join(f"DA:{line},{int(line <= covered)}\n" for line in range(1, 21))
                self.write("coverage.lcov", f"SF:{name}\n{records}LF:20\nLH:{covered}\nend_of_record\n")
                self.assertEqual(gate.main(args), expected_exit)
                summary = json.loads((self.root / "summary.json").read_text())
                self.assertEqual(summary["total"]["minimum"], 95)
                self.assertEqual(summary["changed"]["minimum"], 95)

    def test_cli_pass_fail_json_and_missing_report(self):
        name = "frontend/src/main.ts"
        self.write(name, "export const one = 1\nexport const two = 2\n")
        self.commit()
        self.write("coverage.lcov", f"SF:{name}\nDA:1,1\nDA:2,1\nend_of_record\n")
        args = ["--repo", str(self.root), "--base", self.base, "--report", "coverage.lcov", ".", "--json-output", str(self.root / "summary.json")]
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(gate.main(args), 0)
            self.assertTrue(json.loads((self.root / "summary.json").read_text())["passed"])
            self.write("coverage.lcov", f"SF:{name}\nDA:1,1\nDA:2,0\nend_of_record\n")
            self.assertEqual(gate.main(args), 1)
            self.assertFalse(json.loads((self.root / "summary.json").read_text())["passed"])
            self.write("coverage.lcov", "")
            self.assertEqual(gate.main(args), 2)
            (self.root / "coverage.lcov").unlink()
            self.assertEqual(gate.main(args), 2)


if __name__ == "__main__":
    unittest.main()
