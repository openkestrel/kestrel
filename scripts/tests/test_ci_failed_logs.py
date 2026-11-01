from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("ci_failed_logs", ROOT / "scripts/ci-failed-logs.py")
CLI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLI)


def job(number, status="completed", conclusion="failure", name="workspace (rust)"):
    return {"id": number, "status": status, "conclusion": conclusion, "name": name}


def response(text, code=0):
    return subprocess.CompletedProcess([], code, text.encode() if code == 0 else b"", b"" if code == 0 else text.encode())


class FailedLogsTest(unittest.TestCase):
    def invoke(self, replies, *arguments, target=("123",)):
        stdout, stderr = io.StringIO(), io.StringIO()
        with patch.object(CLI.subprocess, "run", side_effect=replies) as requests, patch.object(CLI.time, "sleep") as sleep:
            with redirect_stdout(stdout), redirect_stderr(stderr):
                code = CLI.main([*target, "--repo", "openkestrel/kestrel", *arguments])
        self.slept = sleep.call_count
        return code, stdout.getvalue(), stderr.getvalue(), requests.call_args_list

    def test_waits_on_a_pull_requests_ci_run_until_a_job_fails(self):
        runs = {"workflow_runs": [
            {"id": 7, "path": ".github/workflows/ci.yml"},
            {"id": 9, "path": ".github/workflows/ci.yml"},
            {"id": 11, "path": ".github/workflows/issue-link.yml"},
        ]}
        with tempfile.TemporaryDirectory() as directory:
            code, output, errors, calls = self.invoke(
                [response(json.dumps({"head": {"sha": "abc"}})), response(json.dumps(runs)),
                 response(json.dumps([{"jobs": [job(1, "in_progress", None)]}])),
                 response(json.dumps({"status": "in_progress"})),
                 response(json.dumps([{"jobs": [job(1, "in_progress", None), job(2)]}])),
                 response("error: broke\n")],
                "--wait", "--output-dir", directory, target=("--pr", "913"),
            )
            self.assertEqual(code, 0, errors)
            self.assertEqual(self.slept, 1)
            self.assertIn("Run 9: 1 completed failed/cancelled jobs; 1 unfinished jobs.", output)
            self.assertTrue(any("head_sha=abc" in arg for arg in calls[1].args[0]))
            self.assertIn("repos/openkestrel/kestrel/actions/jobs/2/logs", calls[-1].args[0])

    def test_waiting_ends_when_the_run_completes_without_a_failure(self):
        code, output, errors, _ = self.invoke(
            [response(json.dumps([{"jobs": [job(1, conclusion="success")]}])),
             response(json.dumps({"status": "completed"}))],
            "--wait",
        )
        self.assertEqual(code, 0, errors)
        self.assertEqual(self.slept, 0)
        self.assertIn("completed with no failed or cancelled job", output)

    def test_a_pull_request_without_a_ci_run_is_an_error_unless_waiting(self):
        code, output, errors, _ = self.invoke(
            [response(json.dumps({"head": {"sha": "abc"}})), response(json.dumps({"workflow_runs": []}))],
            target=("--pr", "913"),
        )
        self.assertEqual(code, 1)
        self.assertIn("No CI run for pull request 913", errors)

    def test_reads_failed_jobs_across_pages_while_other_jobs_are_running(self):
        pages = [
            {"jobs": [job(1, "in_progress", None), job(2, conclusion="success"), job(3)]},
            {"jobs": [job(4, conclusion="timed_out"), job(5, conclusion="skipped")]},
        ]
        with tempfile.TemporaryDirectory() as directory:
            code, output, errors, calls = self.invoke(
                [response(json.dumps(pages)), response("error: bad contract\n"), response("runner timed out\n")],
                "--output-dir", directory,
            )
            self.assertEqual(code, 0, errors)
            self.assertIn("2 completed failed/cancelled jobs; 1 unfinished jobs", output)
            self.assertIn("Job 3:", output)
            self.assertIn("Job 4:", output)
            self.assertEqual(sorted(path.name for path in Path(directory).iterdir()), ["123-3.log", "123-4.log"])
            commands = [call.args[0] for call in calls]
            self.assertIn("--paginate", commands[0])
            self.assertIn("--slurp", commands[0])
            self.assertTrue(any("filter=latest" in arg for arg in commands[0]))
            self.assertIn("repos/openkestrel/kestrel/actions/jobs/3/logs", commands[1])
            self.assertIn("repos/openkestrel/kestrel/actions/jobs/4/logs", commands[2])
            for command in commands:
                self.assertIn("GET", command)
                self.assertIn("--allow-escape-sequences", command)

    def test_saves_full_sanitized_log_and_bounds_the_excerpt(self):
        log = "\x1b[31merror: first\x1b[0m\n" + "ordinary output\n" * 100
        log += "\x1b]8;;https://example.com\x1b\\error: " + "x" * 1000 + "\x1b]8;;\x1b\\\n"
        log += "\x1b]0;window title\x07done\x00\x08\u202e\r\n"
        with tempfile.TemporaryDirectory() as directory:
            code, output, errors, _ = self.invoke(
                [response(json.dumps([{"jobs": [job(7)]}])), response(log)],
                "--output-dir", directory, "--max-lines", "4",
            )
            self.assertEqual(code, 0, errors)
            saved = (Path(directory) / "123-7.log").read_text()
            self.assertEqual(len(saved.splitlines()), 103)
            self.assertIn("error: " + "x" * 1000, saved)
            self.assertTrue(saved.endswith("done\n"))
            self.assertNotIn("\x1b", saved)
            self.assertNotIn("window title", saved)
            self.assertNotIn("\u202e", saved)
            excerpts = [line for line in output.splitlines() if line.partition(":")[0].isdigit()]
            self.assertEqual(len(excerpts), 4)
            self.assertTrue(all(len(line.partition(": ")[2]) <= 240 for line in excerpts))
            self.assertIn("1: error: first", output)
            self.assertIn("102: error:", output)

    def test_excerpt_falls_back_to_tail_without_error_markers(self):
        self.assertEqual(CLI.excerpt("a\nb\nc\nd\n", 2), [(3, "c"), (4, "d")])

    def test_small_budget_is_shared_across_jobs_and_full_logs_are_saved(self):
        with tempfile.TemporaryDirectory() as directory:
            code, output, errors, _ = self.invoke(
                [response(json.dumps([{"jobs": [job(1), job(2), job(3)]}]))]
                + [response("error: bad\ncontext\n")] * 3,
                "--output-dir", directory, "--max-lines", "1",
            )
            self.assertEqual(code, 0, errors)
            self.assertEqual(len(list(Path(directory).iterdir())), 3)
            excerpts = [line for line in output.splitlines() if line.partition(":")[0].isdigit()]
            self.assertEqual(len(excerpts), 1)

    def test_empty_failure_list_does_not_claim_pass_or_create_logs(self):
        for jobs in [[], [job(1, "queued", None)], [job(1, conclusion="success")]]:
            with self.subTest(jobs=jobs), tempfile.TemporaryDirectory() as directory:
                target = Path(directory) / "logs"
                code, output, errors, calls = self.invoke(
                    [response(json.dumps([{"jobs": jobs}]))], "--output-dir", str(target),
                )
                self.assertEqual(code, 0, errors)
                self.assertIn("does not establish that the run passed or started", output)
                self.assertEqual(len(calls), 1)
                self.assertFalse(target.exists())

    def test_download_failure_keeps_other_jobs_and_returns_nonzero(self):
        with tempfile.TemporaryDirectory() as directory:
            code, output, errors, _ = self.invoke(
                [response(json.dumps([{"jobs": [job(1), job(2, conclusion="cancelled")]}])),
                 response("\x1b[31mlogs expired (HTTP 404)\x1b[0m", 1), response("error: cancelled\n")],
                "--output-dir", directory,
            )
            self.assertEqual(code, 1)
            self.assertIn("Job 1:", errors)
            self.assertIn("HTTP 404", errors)
            self.assertNotIn("\x1b", errors)
            self.assertIn("Job 2:", output)
            self.assertFalse((Path(directory) / "123-1.log").exists())
            self.assertEqual((Path(directory) / "123-2.log").read_text(), "error: cancelled\n")

    def test_listing_failure_is_distinct_from_no_failed_jobs(self):
        code, output, errors, calls = self.invoke([response("HTTP 403: insufficient permissions", 1)])
        self.assertEqual(code, 1)
        self.assertEqual(output, "")
        self.assertIn("HTTP 403", errors)
        self.assertEqual(len(calls), 1)

    def test_malformed_jobs_response_is_an_error(self):
        for text in ["not JSON", "{}", '[{"jobs": null}]', '[{"jobs": [{"id": "7"}]}]']:
            with self.subTest(text=text):
                code, output, errors, _ = self.invoke([response(text)])
                self.assertEqual(code, 1)
                self.assertEqual(output, "")
                self.assertIn("Invalid GitHub jobs response", errors)

    def test_missing_gh_and_request_timeout_are_reported(self):
        for error, message in [
            (FileNotFoundError(), "gh is not installed"),
            (subprocess.TimeoutExpired("gh", 60), "timed out"),
        ]:
            with self.subTest(message=message):
                code, output, errors, _ = self.invoke([error])
                self.assertEqual(code, 1)
                self.assertEqual(output, "")
                self.assertIn(message, errors)

    def test_unwritable_output_location_is_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "file"
            target.write_text("existing file")
            code, _, errors, calls = self.invoke(
                [response(json.dumps([{"jobs": [job(1)]}]))], "--output-dir", str(target),
            )
            self.assertEqual(code, 1)
            self.assertTrue(errors)
            self.assertEqual(len(calls), 1)
            self.assertEqual(target.read_text(), "existing file")

    def test_sanitizes_c1_and_unterminated_terminal_strings(self):
        self.assertEqual(CLI.sanitize("\x9b31mred\x9b0m\x9dtitle\x9c\n"), "red\n")
        self.assertEqual(CLI.sanitize("before\x1b]unterminated title"), "before")


if __name__ == "__main__":
    unittest.main()
