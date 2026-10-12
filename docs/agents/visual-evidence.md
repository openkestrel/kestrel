# Visual evidence

A pull request that changes what someone sees in the browser Client shows it in the body's
**Evidence** section, beside the test output the `pr` skill already asks for. #703 (a changed
screen, before and after) and #702 (a new screen, a recording and stills) are the models.

| The change | Its evidence |
| --- | --- |
| An existing screen looks or behaves differently | the same state before and after, as stills |
| A new screen, state or control | a still of each state worth a reviewer's eye |
| Something that moves: an interaction, a stream, keyboard use | a recording, plus stills of the states it passes through |
| Layout or colour | add a dark still and a 375px still |
| A `kestrel` command's output | the output in a `text` block, before and after; no image |
| Nothing a person sees | none |

## Capture

`playwright-cli` drives the browser and an e2e spec supplies the page: the spec boots the control
plane and the Client, seeds them, installs its scripted streams and reads, then holds still.
`playwright-cli --help` names the skill it ships with; read that skill's `video-recording.md`
before recording. Run these from `packages/client`.

1. Pick the test in `e2e/` that reaches the state, or the nearest one. Any test gives a running,
   empty control plane to drive by hand from its first line.

2. Start it paused, in the background, and wait for `Debugging Instructions` in its output:

   ```sh
   bun run build
   bunx playwright test e2e/transcript.spec.ts -g "attributed to its Participant" --debug=cli
   ```

3. Attach with the session it names, run to the line where the state exists, and capture into
   `evidence/`, which git ignores:

   ```sh
   playwright-cli attach tw-a0180e
   playwright-cli -s=tw-a0180e pause-at e2e/transcript.spec.ts:47
   playwright-cli -s=tw-a0180e resize 1280 800
   playwright-cli -s=tw-a0180e screenshot --filename=evidence/transcript-after.png
   playwright-cli -s=tw-a0180e set-color-scheme dark
   playwright-cli -s=tw-a0180e resize 375 800
   playwright-cli -s=tw-a0180e screenshot --filename=evidence/transcript-dark-375-after.png
   playwright-cli -s=tw-a0180e video-start evidence/steps.webm --size=1280x800 --cursor
   playwright-cli -s=tw-a0180e click "getByRole('button', { name: 'Steps' })"
   playwright-cli -s=tw-a0180e video-stop
   ```

   From the pause the page is yours: click, type, `route` a read, `goto` another route.

4. Read every PNG. Capture is done when each still shows the state its name claims, with no
   spinner, empty pane or half-loaded font.

5. `playwright-cli -s=tw-a0180e resume` lets the test finish, which stops the servers.

### Before

When an existing screen changed, repeat steps 2 to 5 with the Client built from the merge-base,
pausing at the same line and naming the files `-before`. The spec, its fixtures and the control
plane stay this branch's; only the served Client is old.

```sh
base=$(git merge-base HEAD origin/main)
mkdir -p "$TMPDIR/kestrel-base" && git archive "$base" | tar -x -C "$TMPDIR/kestrel-base"
(cd "$TMPDIR/kestrel-base/packages/client" && bun install --frozen-lockfile && bun run build)
KESTREL_CLIENT_ROOT="$TMPDIR/kestrel-base/packages/client/dist/client" \
	bunx playwright test e2e/transcript.spec.ts -g "attributed to its Participant" --debug=cli
```

## Publish

1. Write the whole body to `evidence/body.md`, following the `pr` skill. In Evidence, place each
   file by its relative path: `![The Transcript after, dark at 375px](./transcript-dark-375-after.png)`,
   and a recording the same way. Pair stills in a `| | Before | After |` table with rows labelled
   by what differs (`Dark, 375px`). Say in one line what was captured against: a real control
   plane, a scripted stream, a stubbed read.

2. From `evidence/`, upload exactly the files the body places; `gh` rewrites each path to its
   uploaded URL:

   ```sh
   gh pr edit <number> --body-file body.md \
   	$(grep -o '](\./[^)]*' body.md | cut -c3- | sort -u | sed 's/^/--attach /')
   ```

   `gh pr create` takes the same flags.

The published body now lives on GitHub. A later edit starts from
`gh pr view --json body -q .body`.

## Limits

- The paused test holds the e2e suite's ports, so one capture or e2e run at a time on a machine.
- A state no spec reaches and no `route` can stub (a real harness, a compose deployment) is
  captured from that deployment with `playwright-cli open <url>` and published the same way.
