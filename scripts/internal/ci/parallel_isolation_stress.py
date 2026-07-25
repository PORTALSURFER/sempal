#!/usr/bin/env python3
"""Run the Wavecrate library test harness repeatedly in fresh parallel processes."""

from __future__ import annotations

import argparse
import json
import os
import re
import signal
import subprocess
import sys
import time
from collections import Counter
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import TextIO

from parallel_isolation_windows_job import (
    WindowsJob,
    bootstrap_command,
    run_bootstrap,
)

SCHEMA_VERSION = 1
PROCESS_LEAK_ENV = "WAVECRATE_ISOLATION_INJECT_PROCESS_LEAK"
GLOBAL_HOOK_LEAK_ENV = "WAVECRATE_ISOLATION_INJECT_GLOBAL_HOOK_LEAK"
INJECTION_ENV_VARS = (PROCESS_LEAK_ENV, GLOBAL_HOOK_LEAK_ENV)
PROCESS_SENTINEL = (
    "test_isolation_sentinels::"
    "parallel_isolation_sentinel_process_state_guard_under_contention"
)
GLOBAL_HOOK_SENTINEL = (
    "app::controller::library::source_write_priority::tests::"
    "parallel_isolation_sentinel_scoped_global_control_lifecycle_under_contention"
)
QUARANTINED_TESTS = (
    {
        "test_name": "prepare_auto_rename_requests_logs_looped_provenance",
        "disposition": "known_quarantine",
        "reason": "excluded from the agent-safe and isolation-stress library lanes",
    },
    {
        "test_name": "rating_previous_random_history_entry_restores_waveform_for_replacement",
        "disposition": "known_quarantine",
        "reason": "excluded from the agent-safe and isolation-stress library lanes",
    },
)
FAILURE_MARKERS = {
    "WAVECRATE_ISOLATION:process_state_contamination": "process_state_contamination",
    "WAVECRATE_ISOLATION:mutable_global_control_leak": "mutable_global_control_leak",
    "WAVECRATE_ISOLATION:leaked_worker": "leaked_worker",
}
FAILED_TEST_RE = re.compile(r"^test (?P<name>.+) \.\.\. FAILED(?: .*)?$")
FAILURE_SECTION_RE = re.compile(
    r"^---- (?P<name>[^\r\n]+) stdout ----\r?\n"
    r"(?P<body>.*?)(?=^---- [^\r\n]+ stdout ----\r?\n|^failures:\r?\n|\Z)",
    re.MULTILINE | re.DOTALL,
)


@dataclass(frozen=True)
class Failure:
    """One classified test-harness failure."""

    test_name: str | None
    failure_class: str
    disposition: str
    evidence: str


@dataclass(frozen=True)
class ProcessResult:
    """Result of one fresh test-binary process."""

    status: str
    exit_code: int | None
    duration_ms: int
    failures: list[Failure]
    output: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--iterations",
        type=lambda value: bounded_int(value, minimum=1, maximum=100),
        default=5,
        help="Fresh library-test processes to run, 1-100 (default: 5).",
    )
    parser.add_argument(
        "--test-threads",
        type=lambda value: bounded_int(value, minimum=2, maximum=256),
        default=min(8, max(2, os.cpu_count() or 2)),
        help="Explicit libtest parallelism, 2-256, for every stress iteration.",
    )
    parser.add_argument(
        "--timeout-seconds",
        type=lambda value: bounded_int(value, minimum=1, maximum=3600),
        default=900,
        help="Per-process timeout, 1-3600 seconds (default: 900).",
    )
    parser.add_argument(
        "--output",
        type=Path,
        help="JSONL report path. Defaults to a timestamped target/test-isolation-stress file.",
    )
    parser.add_argument(
        "--test-binary",
        type=Path,
        help=argparse.SUPPRESS,
    )
    return parser.parse_args()


def bounded_int(value: str, *, minimum: int, maximum: int) -> int:
    parsed = int(value)
    if not minimum <= parsed <= maximum:
        raise argparse.ArgumentTypeError(f"must be between {minimum} and {maximum}")
    return parsed


def repo_root() -> Path:
    result = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit("parallel-isolation stress must run inside the Wavecrate repository")
    return Path(result.stdout.strip()).resolve()


def default_report_path(root: Path) -> Path:
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return root / "target" / "test-isolation-stress" / f"{stamp}-{os.getpid()}.jsonl"


def discover_test_binary(root: Path) -> Path:
    command = [
        "cargo",
        "test",
        "-p",
        "wavecrate",
        "--lib",
        "--no-run",
        "--message-format=json",
    ]
    result = subprocess.run(
        command,
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
        env=clean_environment(),
    )
    if result.returncode != 0:
        sys.stderr.write(result.stdout)
        sys.stderr.write(result.stderr)
        raise SystemExit(f"failed to compile the Wavecrate library test harness ({result.returncode})")

    executables: list[Path] = []
    for line in result.stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        target = message.get("target", {})
        executable = message.get("executable")
        if (
            message.get("reason") == "compiler-artifact"
            and message.get("profile", {}).get("test") is True
            and target.get("name") == "wavecrate"
            and "lib" in target.get("kind", [])
            and executable
        ):
            executables.append(Path(executable).resolve())

    if len(executables) != 1:
        raise SystemExit(
            "expected exactly one Wavecrate library test executable, "
            f"found {len(executables)}"
        )
    return executables[0]


def clean_environment(overrides: dict[str, str] | None = None) -> dict[str, str]:
    environment = os.environ.copy()
    for key in INJECTION_ENV_VARS:
        environment.pop(key, None)
    environment["CARGO_TERM_COLOR"] = "never"
    environment["NO_COLOR"] = "1"
    if overrides:
        environment.update(overrides)
    return environment


def extract_failures(output: str, exit_code: int | None) -> list[Failure]:
    sections = {
        match.group("name"): match.group("body")
        for match in FAILURE_SECTION_RE.finditer(output)
    }
    names: list[str] = []
    for line in output.splitlines():
        match = FAILED_TEST_RE.match(line)
        if match and match.group("name") not in names:
            names.append(match.group("name"))

    failures = [
        Failure(
            test_name=name,
            failure_class=classify_failure(sections.get(name, output)),
            disposition="unexpected",
            evidence=failure_evidence(sections.get(name, output)),
        )
        for name in names
    ]
    if not failures and exit_code not in (0, None):
        failures.append(
            Failure(
                test_name=None,
                failure_class="test_binary_failure",
                disposition="unexpected",
                evidence=failure_evidence(output),
            )
        )
    return failures


def classify_failure(output: str) -> str:
    for marker, failure_class in FAILURE_MARKERS.items():
        if marker in output:
            return failure_class
    return "test_failure"


def failure_evidence(output: str) -> str:
    lines = [line.strip() for line in output.splitlines() if line.strip()]
    evidence = next(
        (line for line in lines if "WAVECRATE_ISOLATION:" in line),
        None,
    )
    if evidence is None:
        evidence = next(
            (
                line
                for line in lines
                if "panicked at" in line or line.startswith("error:")
            ),
            None,
        )
    if evidence is None:
        evidence = next(
            (line.strip() for line in output.splitlines() if line.strip()),
            "test binary exited without diagnostic output",
        )
    return evidence[:500]


def process_group_has_survivors(process_group: int) -> bool:
    try:
        os.killpg(process_group, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def terminate_posix_process_group(process: subprocess.Popen[str]) -> None:
    process_group = process.pid
    if not process_group_has_survivors(process_group):
        return
    try:
        os.killpg(process_group, signal.SIGTERM)
    except ProcessLookupError:
        return
    deadline = time.monotonic() + 1
    while process_group_has_survivors(process_group) and time.monotonic() < deadline:
        process.poll()
        time.sleep(0.02)
    if process_group_has_survivors(process_group):
        try:
            os.killpg(process_group, signal.SIGKILL)
        except (PermissionError, ProcessLookupError):
            if process.poll() is None:
                process.kill()


def collect_terminated_output(
    process: subprocess.Popen[str], previous_output: str | None
) -> str:
    try:
        output, _ = process.communicate(timeout=2)
        return output or previous_output or ""
    except subprocess.TimeoutExpired as timeout:
        process.kill()
        try:
            output, _ = process.communicate(timeout=1)
            return output or timeout.output or previous_output or ""
        except subprocess.TimeoutExpired as final_timeout:
            if process.stdout is not None:
                process.stdout.close()
            return final_timeout.output or timeout.output or previous_output or ""


def run_fresh_process(
    command: list[str],
    *,
    root: Path,
    timeout_seconds: float,
    environment: dict[str, str],
) -> ProcessResult:
    started = time.monotonic()
    popen_kwargs: dict[str, object] = {}
    owned_job: WindowsJob | None = None
    if os.name == "nt":
        command = bootstrap_command(command, Path(__file__).resolve())
        popen_kwargs["creationflags"] = subprocess.CREATE_NEW_PROCESS_GROUP
        popen_kwargs["stdin"] = subprocess.PIPE
    else:
        popen_kwargs["start_new_session"] = True

    process = subprocess.Popen(
        command,
        cwd=root,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        **popen_kwargs,
    )
    if os.name == "nt":
        try:
            owned_job = WindowsJob(process)
        except OSError as error:
            process.kill()
            output = collect_terminated_output(process, None)
            return ProcessResult(
                status="ownership_error",
                exit_code=process.returncode,
                duration_ms=round((time.monotonic() - started) * 1000),
                failures=[
                    Failure(
                        test_name=None,
                        failure_class="worker_ownership_error",
                        disposition="unexpected",
                        evidence=f"could not create an owned Windows process tree: {error}",
                    )
                ],
                output=output,
            )
        assert process.stdin is not None
        process.stdin.write("1")
        process.stdin.close()
        process.stdin = None

    try:
        output, _ = process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired as timeout:
        if owned_job is not None:
            owned_job.terminate()
        else:
            terminate_posix_process_group(process)
        output = collect_terminated_output(process, timeout.output)
        duration_ms = round((time.monotonic() - started) * 1000)
        if owned_job is not None:
            owned_job.close()
        return ProcessResult(
            status="timeout",
            exit_code=None,
            duration_ms=duration_ms,
            failures=[
                Failure(
                    test_name=None,
                    failure_class="timeout",
                    disposition="unexpected",
                    evidence=f"test process exceeded {timeout_seconds:g} seconds",
                )
            ],
            output=output,
        )

    duration_ms = round((time.monotonic() - started) * 1000)
    failures = extract_failures(output, process.returncode)
    leaked_worker = (
        owned_job.active_processes() > 0
        if owned_job is not None
        else process_group_has_survivors(process.pid)
    )
    if leaked_worker:
        if owned_job is not None:
            owned_job.terminate()
            owned_job.close()
        else:
            terminate_posix_process_group(process)
        failures.append(
            Failure(
                test_name=None,
                failure_class="leaked_worker",
                disposition="unexpected",
                evidence="test process exited while an owned worker process remained alive",
            )
        )
        return ProcessResult(
            status="leaked_worker",
            exit_code=process.returncode,
            duration_ms=duration_ms,
            failures=failures,
            output=output,
        )
    if owned_job is not None:
        owned_job.close()
    return ProcessResult(
        status="passed" if process.returncode == 0 else "failed",
        exit_code=process.returncode,
        duration_ms=duration_ms,
        failures=failures,
        output=output,
    )


def stress_command(test_binary: Path, test_threads: int) -> list[str]:
    command = [
        str(test_binary),
        f"--test-threads={test_threads}",
    ]
    for quarantine in QUARANTINED_TESTS:
        command.extend(["--skip", quarantine["test_name"]])
    return command


def sentinel_command(test_binary: Path, test_name: str) -> list[str]:
    return [
        str(test_binary),
        test_name,
        "--exact",
        "--test-threads=1",
    ]


def emit(report: TextIO, payload: dict[str, object]) -> None:
    line = json.dumps(payload, sort_keys=True, separators=(",", ":"))
    report.write(line + "\n")
    report.flush()
    print(line, flush=True)


def result_payload(
    *,
    phase: str,
    iteration: int,
    iterations: int,
    test_binary: Path,
    test_threads: int,
    result: ProcessResult,
) -> dict[str, object]:
    first_failure = result.failures[0] if result.failures else None
    failure_class_counts = Counter(failure.failure_class for failure in result.failures)
    return {
        "schema_version": SCHEMA_VERSION,
        "phase": phase,
        "iteration": iteration,
        "iterations": iterations,
        "test_binary": str(test_binary),
        "test_threads": test_threads,
        "status": result.status,
        "exit_code": result.exit_code,
        "duration_ms": result.duration_ms,
        "first_test_name": first_failure.test_name if first_failure else None,
        "first_failure_class": first_failure.failure_class if first_failure else None,
        "failure_class_counts": dict(sorted(failure_class_counts.items())),
        "failures": [asdict(failure) for failure in result.failures],
    }


def summary_payload(
    *,
    status: str,
    completed_iterations: int,
    test_binary: Path,
    report_path: Path,
    result: ProcessResult | None,
) -> dict[str, object]:
    failures = result.failures if result else []
    first_failure = failures[0] if failures else None
    failure_class_counts = Counter(failure.failure_class for failure in failures)
    return {
        "schema_version": SCHEMA_VERSION,
        "phase": "summary",
        "status": status,
        "completed_iterations": completed_iterations,
        "test_binary": str(test_binary),
        "report": str(report_path),
        "exit_code": result.exit_code if result else 0,
        "first_test_name": first_failure.test_name if first_failure else None,
        "first_failure_class": first_failure.failure_class if first_failure else None,
        "failure_class_counts": dict(sorted(failure_class_counts.items())),
        "failures": [asdict(failure) for failure in failures],
        "quarantined_coverage": list(QUARANTINED_TESTS),
    }


def verify_injected_sentinel(
    *,
    root: Path,
    test_binary: Path,
    test_name: str,
    injection_env: str,
    expected_class: str,
    timeout_seconds: int,
) -> ProcessResult:
    result = run_fresh_process(
        sentinel_command(test_binary, test_name),
        root=root,
        timeout_seconds=min(timeout_seconds, 60),
        environment=clean_environment({injection_env: "1"}),
    )
    detected = (
        result.status == "failed"
        and any(
            failure.test_name == test_name and failure.failure_class == expected_class
            for failure in result.failures
        )
    )
    if detected:
        return ProcessResult(
            status="expected_failure_detected",
            exit_code=result.exit_code,
            duration_ms=result.duration_ms,
            failures=[
                Failure(
                    test_name=failure.test_name,
                    failure_class=failure.failure_class,
                    disposition="injected_sentinel",
                    evidence=failure.evidence,
                )
                for failure in result.failures
            ],
            output=result.output,
        )
    return ProcessResult(
        status="detector_verification_failed",
        exit_code=result.exit_code,
        duration_ms=result.duration_ms,
        failures=result.failures,
        output=result.output,
    )


def print_failure_output(result: ProcessResult) -> None:
    if not result.output:
        return
    lines = result.output.splitlines()
    sys.stderr.write("\n[first failing test process output]\n")
    sys.stderr.write("\n".join(lines[-120:]) + "\n")


def main() -> int:
    args = parse_args()
    root = repo_root()
    test_binary = args.test_binary.resolve() if args.test_binary else discover_test_binary(root)
    report_path = (args.output or default_report_path(root)).resolve()
    report_path.parent.mkdir(parents=True, exist_ok=True)

    with report_path.open("w", encoding="utf-8", newline="\n") as report:
        emit(
            report,
            {
                "schema_version": SCHEMA_VERSION,
                "phase": "metadata",
                "test_binary": str(test_binary),
                "iterations": args.iterations,
                "test_threads": args.test_threads,
                "timeout_seconds": args.timeout_seconds,
                "quarantined_coverage": list(QUARANTINED_TESTS),
            },
        )

        sentinel_specs = (
            (
                PROCESS_SENTINEL,
                PROCESS_LEAK_ENV,
                "process_state_contamination",
            ),
            (
                GLOBAL_HOOK_SENTINEL,
                GLOBAL_HOOK_LEAK_ENV,
                "mutable_global_control_leak",
            ),
        )
        for index, (test_name, injection_env, expected_class) in enumerate(
            sentinel_specs, start=1
        ):
            result = verify_injected_sentinel(
                root=root,
                test_binary=test_binary,
                test_name=test_name,
                injection_env=injection_env,
                expected_class=expected_class,
                timeout_seconds=args.timeout_seconds,
            )
            emit(
                report,
                result_payload(
                    phase="detector_verification",
                    iteration=index,
                    iterations=len(sentinel_specs),
                    test_binary=test_binary,
                    test_threads=1,
                    result=result,
                ),
            )
            if result.status != "expected_failure_detected":
                emit(
                    report,
                    summary_payload(
                        status="failed",
                        completed_iterations=0,
                        test_binary=test_binary,
                        report_path=report_path,
                        result=result,
                    ),
                )
                print_failure_output(result)
                print(f"[parallel_isolation] report={report_path}", file=sys.stderr)
                return 1

        for iteration in range(1, args.iterations + 1):
            result = run_fresh_process(
                stress_command(test_binary, args.test_threads),
                root=root,
                timeout_seconds=args.timeout_seconds,
                environment=clean_environment(),
            )
            emit(
                report,
                result_payload(
                    phase="stress",
                    iteration=iteration,
                    iterations=args.iterations,
                    test_binary=test_binary,
                    test_threads=args.test_threads,
                    result=result,
                ),
            )
            if result.status != "passed":
                emit(
                    report,
                    summary_payload(
                        status="failed",
                        completed_iterations=iteration,
                        test_binary=test_binary,
                        report_path=report_path,
                        result=result,
                    ),
                )
                print_failure_output(result)
                print(f"[parallel_isolation] report={report_path}", file=sys.stderr)
                return 1

        emit(
            report,
            summary_payload(
                status="passed",
                completed_iterations=args.iterations,
                test_binary=test_binary,
                report_path=report_path,
                result=None,
            ),
        )

    print(f"[parallel_isolation] report={report_path}")
    return 0


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--windows-job-bootstrap":
        raise SystemExit(run_bootstrap(sys.argv[2]))
    raise SystemExit(main())
