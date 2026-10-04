# Browser setup and fix-it states

Throwaway prototype for [Prototype the browser setup flow and fix-it states](https://github.com/openkestrel/kestrel/issues/491), under [Map: Plan 0.4 — kestrel starts from one command](https://github.com/openkestrel/kestrel/issues/481). No design here is approved yet.

## Run

From the repository root, after `bun install --frozen-lockfile`:

```sh
bun run --cwd packages/client dev --host 127.0.0.1 --port 3011
```

Open <http://127.0.0.1:3011/prototype/setup>. No control plane, Docker, GitHub account or credentials are needed. Use dummy text in secret fields. Nothing persists or calls an operator API. Auth URLs are deliberately inert examples. The GitHub button simulates a completed App installation, rather than creating an App.

Built on `prototype/workbench-ai-elements`, retaining the settled D workbench as the adjacent destination. Uses Geist and Geist Mono, Base UI buttons, shadcn Questionnaire, TanStack Router and AI Elements Conversation and Model Selector. Model Selector is the existing component with a small controlled fixture list, not a new implementation for the dependent workbench ticket. Model names stand in for harness ACP options, not a published catalogue.

## Review

All three layouts share the same fixtures, answers and transitions. The bottom switcher changes `?variant=` and theme; left/right arrows cycle layouts except inside forms and dialogs.

| Variant | Structure | Review URL |
| --- | --- | --- |
| A | Focused question, next/back, progress above the form | `/prototype/setup?variant=A` |
| B | Checklist beside the active form; collapsed checklist on narrow screens | `/prototype/setup?variant=B` |
| C | Answer history in a Conversation, active question at the bottom; edit previous answers | `/prototype/setup?variant=C` |

Recommendation: A. It asks the least of a new Operator; the review step makes the plan inspectable. B makes dependencies more visible. C makes previous answers visible but takes more space and risks implying an agent is guiding setup.

The fixture selector is review tooling. Each selection resets the in-memory state. The state drawer excludes secret values. Relay fixture controls let you finish, fail, expire, or diagnose a check without leaving the page. These controls and the switcher are development-only.

Walk the empty install through name → harness → sign-in → repository → Brief → review → queued Session. Select Codex to see device-code waiting and Claude Code to see code entry; OpenCode Go/Zen and API keys use manual entry. Every relay path has a manual fallback. Changing harness clears its previous sign-in and model selection. A model is optional and defaults to the harness's default.

Other fixtures cover:

- unreachable control plane with Compose inspection commands and retry;
- waiting, checking, successful, failed, expired and cancelled relay;
- established coverage failure, authentication failure without an expiry diagnosis, and inconclusive check;
- an interrupted flow after naming the Operator;
- ready with no Workspaces, an unused broken sign-in alongside a usable path, selected sign-in failure, missing harness and missing repository;
- GitHub access loss and a public repository that can start but cannot push;
- Sign-ins, Agents and Repositories settings;
- light/dark, narrow-screen layout and keyboard operation.

This represents recovery and navigation, not backend persistence, actual polling, real App manifest callbacks, secret storage or real sign-in verification. “Start Session” queues a fixture; its link opens the existing D workbench fixture. The workbench's independent fixture is not the just-created Session.

## Questions for the human review

1. Which layout should setup use? Recommendation: A, with its explicit final review.
2. Does leaving after saving the Operator's name resume the remaining setup, or immediately switch to fix-it states? Recommendation: resume the focused flow while prerequisites are missing, then use contextual fix-it states after readiness. The interrupted fixture currently demonstrates resuming.

The second question exposes a discrepancy in [Spec: 0.4 — kestrel starts from one command](https://github.com/openkestrel/kestrel/issues/460): it says setup appears only while there is no Operator, although naming the Operator is the first step. Saving that name must not strand a returning person halfway through setup. The prototype's final review also lists the Operator among new declarations; the production preview must distinguish already-saved prerequisites from declarations still to apply. Amend only after the lifecycle decision is confirmed.

## Validation

Client typecheck and production build passed. A headless Chromium walkthrough completed Codex device auth → repository → Brief → review → queued Session. All 22 fixtures rendered in all three layouts (66 combinations) without page errors. A 390px viewport showed no horizontal overflow. Desktop and narrow-screen screenshots are in this directory. The narrow checklist was collapsed after screenshot review; the final screenshot reflects that adjustment.

No production backend or workspace test suite was run for this throwaway prototype. CI remains the repository's full gate.
