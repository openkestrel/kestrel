# Preparing a release PR

[ADR-0053](adr/0053-a-tag-prepares-a-release-acceptance-publishes-it.md) separates preparation,
publication into a draft, and operator acceptance. This procedure prepares reviewable source
files only; it does not cut or accept a release. Python 3.11 or newer is required.

Start on a branch from current main, with a clean checkout. Fetch the existing version tags before
choosing an unused candidate version. The maintainer chooses the version explicitly; after a
failed candidate, use a new patch version rather than changing the published version's content.

Write curated notes in a file outside the checkout. Include each of these headings with actual
behavior changes, fixes and known limitations; say “None” when a section has no entries:

```markdown
### Behavior changes

- Describe what a person can now do.

### Fixes

- Describe corrected behavior.

### Known limitations

- Describe remaining limitations of this candidate.
```

Prepare the chosen version and notes without creating a tag or publishing anything:

```sh
python3 scripts/release.py prepare 0.4.1 --notes /tmp/release-notes.md
python3 scripts/release.py validate v0.4.1
```

The preparer changes `Cargo.toml`, the workspace packages in `Cargo.lock`,
`packages/client/public/version.json`, and the leading `CHANGELOG.md` entry. Rust packages keep
`version.workspace = true`; private npm packages have no independent version. Vite copies the
browser version record into the built Client as `version.json`. The committed `0.0.0` version is
a development placeholder and is refused as a release candidate.

Build the Client using its normal build command, then verify its actual output:

```sh
cd packages/client
bun install --frozen-lockfile
bun run build
cd ../..
python3 scripts/release.py validate v0.4.1 --browser-build packages/client/dist/client
```

Validation rejects disagreement among the tag argument, workspace, inherited packages, lockfile,
browser record and leading curated changelog entry. When the tag already exists locally, it must
resolve to HEAD, the version records and notes must be tracked, and tracked files must be clean. Fetching tags is a prerequisite: the validator
performs no network operation and cannot detect tags that are absent from the checkout.

Review the four changed files, commit them, and open a release PR. The maintainer reviews the
chosen version, notes and known limitations, and waits for the PR's checks before merging it.
Notes can be edited directly in that PR; validation must still pass. Preparation refuses an
existing changelog version or tag, so retries never silently replace reviewed notes.

After merge, the maintainer checks out the exact merged release commit, validates it again and
creates an annotated `v<product-version>` tag on that commit, then pushes that tag. Never tag the
unmerged branch tip. Tagging starts the release checks and draft-artifact preparation owned by the
release workflow; it does not authorize acceptance, `latest` promotion or a ROADMAP marker move.
Fresh-machine acceptance against the exact candidate authorizes those later actions. Failed
acceptance leaves the candidate in draft and requires a reviewed patch candidate.

The validator prints JSON containing `version` and `tag` on success and exits nonzero on refusal.
Release automation should invoke it on a clean checkout of the tagged commit, with all version
tags fetched, and pass `--browser-build` after building the Client. Automated publication and
acceptance are separate release-workflow responsibilities.
