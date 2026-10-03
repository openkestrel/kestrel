# Browser workbench accessibility acceptance

Tracking: [#424](https://github.com/openkestrel/kestrel/issues/424). The browser implementation
landed in [#448](https://github.com/openkestrel/kestrel/pull/448); the VoiceOver pass remains open.

## Verification on 2026-10-03

At commit `86209c7d1ed3d186e5838488adb5c689df10e5df`, `bun run typecheck` passed, and all nine
tests in this command passed:

```sh
cd packages/client
bun run e2e -- e2e/accessibility.spec.ts e2e/narrow.spec.ts e2e/keyboard.spec.ts --workers=1
```

The checks cover the 900 px breakpoint, no horizontal page scroll at 375 px, retained Diff
selection and tab focus, axe audits with zero serious or critical violations, and a keyboard path
that finds a running Session, follows its Workspace, opens a diff and posts a turn. The Session
and diff in the keyboard test are scripted responses; posting uses the control plane. The
announcement check inspects live-region markup; it does not establish what a screen reader speaks.

Safari's desktop accessibility tree exposed the Workspaces, Transcript and Work regions, disclosure
controls, labelled Post text area and Work view tabs in an isolated Workspace with a brief.

## VoiceOver: incomplete

VoiceOver was temporarily enabled through System Settings, with the existing caption-panel
preference enabled. The automation tool could read Safari but could not read VoiceOver's caption
window: `permission_denied`, with AX reads blocked after retries. Its permission query reported
Accessibility and Screen Recording granted. VoiceOver was restored to off, verified through the
System Settings switch. No spoken-announcement result or 375 px VoiceOver result was obtained.

Complete the following in Safari with VoiceOver, first at desktop width and then with a 375 px
viewport, recording findings and fixes on #424:

1. Navigate the live Transcript and its disclosure controls; at 375 px, change pane tabs and
   return, checking selection and keyboard focus.
2. Leave focus in the composer while another participant posts. Check that new shared-state
   content is spoken without moving focus, and that replayed history is not read as new content.
3. Follow a Session through meaningful phase changes and a failure. Check that announcements
   include the failure reason, while tool and usage updates do not produce item-by-item speech.
4. Post a turn and exercise a refused write. Check the send and refusal announcements and retained
   draft, without focus being stolen.

The automated pass and desktop accessibility-tree inspection do not replace this VoiceOver pass.
