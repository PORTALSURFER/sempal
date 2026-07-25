#!/usr/bin/env python3
"""Focused tests for the parallel-isolation stress runner."""

from __future__ import annotations

import importlib.util
import json
import os
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("parallel_isolation_stress.py")
sys.path.insert(0, str(MODULE_PATH.parent))
SPEC = importlib.util.spec_from_file_location("parallel_isolation_stress", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
stress = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = stress
SPEC.loader.exec_module(stress)


class ParallelIsolationStressTests(unittest.TestCase):
    def test_failure_parser_preserves_test_identity_and_failure_family(self) -> None:
        output = textwrap.dedent(
            """
            running 1 test
            test test_isolation_sentinels::sentinel ... FAILED

            failures:

            ---- test_isolation_sentinels::sentinel stdout ----
            thread 'test_isolation_sentinels::sentinel' panicked at src/lib.rs:1:1:
            WAVECRATE_ISOLATION:process_state_contamination env,cwd

            failures:
                test_isolation_sentinels::sentinel
            """
        ).strip()

        failures = stress.extract_failures(output, 101)

        self.assertEqual(len(failures), 1)
        self.assertEqual(
            failures[0].test_name, "test_isolation_sentinels::sentinel"
        )
        self.assertEqual(
            failures[0].failure_class, "process_state_contamination"
        )
        self.assertIn("WAVECRATE_ISOLATION:", failures[0].evidence)

    def test_failure_parser_associates_each_test_with_its_own_evidence(self) -> None:
        output = textwrap.dedent(
            """
            test fixture::first ... FAILED
            test fixture::second ... FAILED

            failures:

            ---- fixture::first stdout ----
            thread 'fixture::first' panicked at first.rs:1: first evidence

            ---- fixture::second stdout ----
            thread 'fixture::second' panicked at second.rs:2: second evidence

            failures:
                fixture::first
                fixture::second
            """
        ).strip()

        failures = stress.extract_failures(output, 101)

        self.assertEqual(len(failures), 2)
        self.assertIn("first.rs", failures[0].evidence)
        self.assertIn("second.rs", failures[1].evidence)

    def test_each_iteration_starts_a_new_process(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pid_log = root / "pids.jsonl"
            fixture = self.write_fixture(
                root,
                """
                import json, os, pathlib
                log = pathlib.Path(os.environ["PID_LOG"])
                with log.open("a", encoding="utf-8") as output:
                    output.write(json.dumps({"pid": os.getpid()}) + "\\n")
                print("test fixture::passes ... ok")
                """,
            )
            command = [sys.executable, str(fixture)]
            environment = stress.clean_environment({"PID_LOG": str(pid_log)})

            first = stress.run_fresh_process(
                command, root=root, timeout_seconds=5, environment=environment
            )
            second = stress.run_fresh_process(
                command, root=root, timeout_seconds=5, environment=environment
            )

            self.assertEqual(first.status, "passed")
            self.assertEqual(second.status, "passed")
            pids = [
                json.loads(line)["pid"]
                for line in pid_log.read_text(encoding="utf-8").splitlines()
            ]
            self.assertEqual(len(pids), 2)
            self.assertNotEqual(pids[0], pids[1])

    def test_injected_process_and_global_hook_leaks_are_classified(self) -> None:
        cases = (
            (
                stress.PROCESS_LEAK_ENV,
                stress.PROCESS_SENTINEL,
                "process_state_contamination",
            ),
            (
                stress.GLOBAL_HOOK_LEAK_ENV,
                stress.GLOBAL_HOOK_SENTINEL,
                "mutable_global_control_leak",
            ),
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fixture = self.write_fixture(
                root,
                """
                import os, sys
                process_env = "WAVECRATE_ISOLATION_INJECT_PROCESS_LEAK"
                global_env = "WAVECRATE_ISOLATION_INJECT_GLOBAL_HOOK_LEAK"
                if process_env in os.environ:
                    name = (
                        "test_isolation_sentinels::"
                        "parallel_isolation_sentinel_process_state_guard_under_contention"
                    )
                    marker = "WAVECRATE_ISOLATION:process_state_contamination"
                elif global_env in os.environ:
                    name = (
                        "app::controller::library::source_write_priority::tests::"
                        "parallel_isolation_sentinel_scoped_global_control_lifecycle_under_contention"
                    )
                    marker = "WAVECRATE_ISOLATION:mutable_global_control_leak"
                else:
                    raise SystemExit(2)
                print(f"test {name} ... FAILED")
                print(f"\\n---- {name} stdout ----")
                print(f"thread '{name}' panicked at fixture:1: {marker}")
                raise SystemExit(101)
                """,
            )

            for injection_env, test_name, expected_class in cases:
                with self.subTest(expected_class=expected_class):
                    result = stress.run_fresh_process(
                        [sys.executable, str(fixture)],
                        root=root,
                        timeout_seconds=5,
                        environment=stress.clean_environment({injection_env: "1"}),
                    )
                    detected = any(
                        failure.test_name == test_name
                        and failure.failure_class == expected_class
                        for failure in result.failures
                    )
                    self.assertEqual(result.status, "failed")
                    self.assertTrue(detected)

    def test_timeout_is_blocking_and_machine_readable(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fixture = self.write_fixture(root, "import time; time.sleep(2)")
            result = stress.run_fresh_process(
                [sys.executable, str(fixture)],
                root=root,
                timeout_seconds=0.05,
                environment=stress.clean_environment(),
            )

            self.assertEqual(result.status, "timeout")
            self.assertEqual(result.failures[0].failure_class, "timeout")
            payload = stress.result_payload(
                phase="stress",
                iteration=1,
                iterations=1,
                test_binary=fixture,
                test_threads=4,
                result=result,
            )
            encoded = json.dumps(payload)
            self.assertEqual(json.loads(encoded)["failures"][0]["test_name"], None)

    def test_successful_parent_with_leaked_worker_is_blocking(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            result = self.run_leaked_worker_fixture(Path(temporary))

            self.assertEqual(result.status, "leaked_worker")
            self.assertEqual(result.failures[-1].failure_class, "leaked_worker")

    @unittest.skipUnless(os.name == "nt", "Windows Job Object coverage")
    def test_windows_job_object_owns_leaked_descendant(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            result = self.run_leaked_worker_fixture(Path(temporary))

            self.assertEqual(result.status, "leaked_worker")

    @unittest.skipUnless(os.name != "nt", "POSIX process-group coverage")
    def test_timeout_kills_child_that_ignores_sigterm_and_holds_output(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fixture = self.write_fixture(
                root,
                """
                import subprocess, sys, time
                subprocess.Popen([
                    sys.executable,
                    "-c",
                    "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(60)",
                ])
                time.sleep(60)
                """,
            )
            started = stress.time.monotonic()
            result = stress.run_fresh_process(
                [sys.executable, str(fixture)],
                root=root,
                timeout_seconds=0.05,
                environment=stress.clean_environment(),
            )

            self.assertEqual(result.status, "timeout")
            self.assertLess(stress.time.monotonic() - started, 4)

    def test_failed_summary_retains_complete_failure_inventory(self) -> None:
        result = stress.ProcessResult(
            status="failed",
            exit_code=101,
            duration_ms=10,
            failures=[
                stress.Failure("fixture::first", "test_failure", "unexpected", "first"),
                stress.Failure("fixture::second", "leaked_worker", "unexpected", "second"),
            ],
            output="",
        )
        payload = stress.summary_payload(
            status="failed",
            completed_iterations=1,
            test_binary=Path("fixture"),
            report_path=Path("report.jsonl"),
            result=result,
        )

        self.assertEqual(payload["phase"], "summary")
        self.assertEqual(payload["first_test_name"], "fixture::first")
        self.assertEqual(payload["failure_class_counts"], {"leaked_worker": 1, "test_failure": 1})
        self.assertEqual(len(payload["failures"]), 2)
        self.assertEqual(
            payload["quarantined_coverage"][0]["disposition"], "known_quarantine"
        )

    def run_leaked_worker_fixture(self, root: Path) -> stress.ProcessResult:
        fixture = self.write_fixture(
            root,
            """
            import subprocess, sys
            subprocess.Popen(
                [sys.executable, "-c", "import time; time.sleep(60)"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            print("test fixture::leaks_worker ... ok")
            """,
        )
        return stress.run_fresh_process(
            [sys.executable, str(fixture)],
            root=root,
            timeout_seconds=5,
            environment=stress.clean_environment(),
        )

    @staticmethod
    def write_fixture(root: Path, body: str) -> Path:
        fixture = root / "fixture.py"
        fixture.write_text(textwrap.dedent(body), encoding="utf-8")
        if os.name != "nt":
            fixture.chmod(0o755)
        return fixture


if __name__ == "__main__":
    unittest.main()
