import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import unicodedata


POLL = 30
PATIENCE = 2 * 60 * 60
FAILED = {"failure", "timed_out", "startup_failure", "action_required", "cancelled"}
ESCAPES = re.compile(
    r"(?:\x1b\[[0-?]*[ -/]*[@-~]|\x9b[0-?]*[ -/]*[@-~]"
    r"|(?:\x1b[\]PX^_]|[\x90\x9d\x98\x9e\x9f])[\s\S]*?(?:\x07|\x1b\\|\x9c|$)"
    r"|\x1b[ -/]*[@-~])"
)
ERROR = re.compile(
    r"##\[error\]|\berror(?:\[|:)|\bFAILED\b|\bfailures:|\bpanicked\b"
    r"|\bTraceback\b|\b(?:AssertionError|FAIL|ERROR):",
    re.IGNORECASE,
)


def sanitize(text):
    text = ESCAPES.sub("", text).replace("\r\n", "\n").replace("\r", "\n")
    return "".join(
        char for char in text
        if char in "\n\t" or unicodedata.category(char) not in {"Cc", "Cf"}
    )


def clipped(text, limit=240):
    text = sanitize(text).replace("\n", " ").replace("\t", " ")
    return text if len(text) <= limit else text[:limit - 3] + "..."


def positive(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be a positive integer")
    return number


def repository(value):
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", value):
        raise argparse.ArgumentTypeError("must be OWNER/REPO")
    return value


def api(endpoint, paginate=False):
    command = ["gh", "api", "--method", "GET", "--allow-escape-sequences", endpoint]
    if paginate:
        command.extend(["--paginate", "--slurp"])
    try:
        result = subprocess.run(command, capture_output=True, timeout=60)
    except FileNotFoundError as error:
        raise RuntimeError("gh is not installed or is not on PATH") from error
    except subprocess.TimeoutExpired as error:
        raise RuntimeError("GitHub log request timed out after 60 seconds") from error
    if result.returncode:
        detail = result.stderr.decode("utf-8", errors="replace")
        raise RuntimeError(f"GitHub request failed: {clipped(detail, 1000)}")
    return result.stdout.decode("utf-8", errors="replace")


def jobs_from_pages(text):
    try:
        pages = json.loads(text)
        if not isinstance(pages, list):
            raise ValueError("expected paginated jobs")
        jobs = []
        for page in pages:
            if not isinstance(page, dict) or not isinstance(page.get("jobs"), list):
                raise ValueError("expected a jobs array on each page")
            for job in page["jobs"]:
                if (
                    not isinstance(job, dict)
                    or type(job.get("id")) is not int
                    or job["id"] <= 0
                    or not isinstance(job.get("name"), str)
                    or not isinstance(job.get("status"), str)
                    or (job.get("conclusion") is not None and not isinstance(job["conclusion"], str))
                ):
                    raise ValueError("invalid job metadata")
                jobs.append(job)
        return jobs
    except (ValueError, TypeError) as error:
        raise RuntimeError(f"Invalid GitHub jobs response: {error}") from error


def parsed(text, what):
    try:
        return json.loads(text)
    except ValueError as error:
        raise RuntimeError(f"Invalid GitHub {what} response: {error}") from error


def run_of_pull(repo, number):
    sha = parsed(api(f"repos/{repo}/pulls/{number}"), "pull request")["head"]["sha"]
    runs = parsed(api(f"repos/{repo}/actions/runs?head_sha={sha}&event=pull_request&per_page=100"), "runs")
    ci = [run["id"] for run in runs["workflow_runs"] if run.get("path") == ".github/workflows/ci.yml"]
    return max(ci) if ci else None


def settled(repo, run_id, jobs):
    if any(job["status"] == "completed" and job.get("conclusion") in FAILED for job in jobs):
        return True
    return parsed(api(f"repos/{repo}/actions/runs/{run_id}"), "run")["status"] == "completed"


def excerpt(text, limit):
    lines = text.splitlines()
    selected = set()
    for index, line in enumerate(lines):
        if ERROR.search(line):
            selected.update(range(max(0, index - 2), min(len(lines), index + 3)))
    indexes = sorted(selected) if selected else list(range(max(0, len(lines) - limit), len(lines)))
    if len(indexes) > limit:
        first = (limit + 1) // 2
        indexes = indexes[:first] + indexes[-(limit - first):] if limit > 1 else indexes[:1]
    return [(index + 1, clipped(lines[index])) for index in indexes]


def main(argv=None):
    parser = argparse.ArgumentParser(description="Read completed failed-job logs, including during an active run.")
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("run_id", type=positive, nargs="?")
    target.add_argument("--pr", type=positive, help="read the CI run of this pull request's head commit")
    parser.add_argument("--wait", action="store_true", help="first wait until a job fails or the run completes")
    parser.add_argument("--repo", type=repository, help="OWNER/REPO; defaults to gh's current repository or GH_REPO")
    parser.add_argument("--output-dir", type=Path, help="directory for full sanitized logs; defaults to a new temporary directory")
    parser.add_argument("--max-lines", type=positive, default=60, help="total excerpt lines across jobs (default: 60)")
    args = parser.parse_args(argv)
    repo = args.repo or "{owner}/{repo}"
    try:
        deadline = time.monotonic() + PATIENCE
        while args.run_id is None:
            args.run_id = run_of_pull(repo, args.pr)
            if args.run_id is None:
                if not args.wait or time.monotonic() > deadline:
                    raise RuntimeError(f"No CI run for pull request {args.pr}'s head commit yet")
                time.sleep(POLL)
        while True:
            jobs = jobs_from_pages(api(f"repos/{repo}/actions/runs/{args.run_id}/jobs?filter=latest&per_page=100", paginate=True))
            if not args.wait or settled(repo, args.run_id, jobs):
                break
            if time.monotonic() > deadline:
                raise RuntimeError(f"Run {args.run_id} neither failed nor completed within {PATIENCE // 60} minutes")
            time.sleep(POLL)
        failed = [job for job in jobs if job["status"] == "completed" and job.get("conclusion") in FAILED]
        unfinished = sum(job["status"] != "completed" for job in jobs)
        print(f"Run {args.run_id}: {len(failed)} completed failed/cancelled jobs; {unfinished} unfinished jobs.")
        if not failed:
            if args.wait and not unfinished:
                print("The run completed with no failed or cancelled job.")
            else:
                print("No failed-job logs to retrieve; this does not establish that the run passed or started.")
            return 0
        directory = args.output_dir or Path(tempfile.mkdtemp(prefix=f"ci-failed-{args.run_id}-"))
        directory.mkdir(parents=True, exist_ok=True)
        remaining = args.max_lines
        errors = False
        for index, job in enumerate(failed):
            print(f"\nJob {job['id']}: {clipped(job['name'])} ({job['conclusion']})")
            try:
                log = sanitize(api(f"repos/{repo}/actions/jobs/{job['id']}/logs"))
                path = directory / f"{args.run_id}-{job['id']}.log"
                path.write_text(log, encoding="utf-8")
                allowance = (remaining + len(failed) - index - 1) // (len(failed) - index)
                selected = excerpt(log, allowance) if allowance else []
                remaining -= len(selected)
                print(f"Saved {clipped(str(path.resolve()), 1000)}; showing {len(selected)} of {len(log.splitlines())} lines.")
                for number, line in selected:
                    print(f"{number}: {line}")
            except (RuntimeError, OSError) as error:
                print(f"Job {job['id']}: {clipped(str(error), 1000)}", file=sys.stderr)
                errors = True
        return 1 if errors else 0
    except (RuntimeError, OSError) as error:
        print(clipped(str(error), 1000), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
