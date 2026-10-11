# Reading a failed CI job

Find the run ID in `gh pr checks` or `gh run list`. When a completed job has failed while other
jobs are still running, read that job without waiting for the whole-run log command:

```sh
mise exec -- python3 scripts/ci-failed-logs.py RUN_ID --repo openkestrel/kestrel
```

The helper uses the installed `gh` authentication and read-only API requests. It follows the
[job-log download](https://docs.github.com/en/rest/actions/workflow-jobs#download-job-logs-for-a-workflow-run)
endpoint for individual completed failed or cancelled jobs. The latest attempt's jobs are
paginated; successful, skipped and unfinished jobs are not downloaded.

Output names each job and shows at most 60 log lines across all jobs, with lines clipped to 240
characters. It selects context around error markers, or the tail when no marker matches. Original
line numbers identify gaps. Full sanitized UTF-8 logs remain in the printed temporary directory;
pass `--output-dir DIRECTORY` to choose it, or `--max-lines NUMBER` to change the excerpt budget.
Terminal escape sequences and other control characters are removed before saving or printing.
Sanitization preserves log contents otherwise; it is not credential redaction.

A successful helper exit means retrieval succeeded, not that CI passed. An empty failure list
does not establish that the run started or completed. A failed download reports its job ID,
continues retrieving the other failed jobs, and returns nonzero; expired logs, missing access and
network errors remain visible rather than looking like an empty log.

Run the fixture-based helper and release tests with:

```sh
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
```

CI runs this discovery in its `release-version` job. `scripts/**` changes select that job through
`.github/scripts/changed-areas.sh`.
