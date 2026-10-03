# AI Elements and shadcn chat components on `base-lyra`

Research checked 3 October 2026 for [#483](https://github.com/openkestrel/kestrel/issues/483), feeding [Spec #461](https://github.com/openkestrel/kestrel/issues/461) and [Spec #460](https://github.com/openkestrel/kestrel/issues/460). Sources: the `@ai-elements` registry ([index](https://elements.ai-sdk.dev/api/registry/registry.json), one `<name>.json` per item), the shadcn `base-lyra` registry ([index](https://ui.shadcn.com/r/styles/base-lyra/registry.json)), shadcn's docs, `@base-ui/react@1.8.0`'s published source, and open issues on [vercel/ai-elements](https://github.com/vercel/ai-elements). As a check, I ran a scratch install of all nineteen components into a copy of `packages/client` (`shadcn@4.21.0`, `components.json` style `base-lyra`), then ran `tsc --noEmit` against it. Nothing from the scratch copy was committed.

## Answer

- **All nineteen names resolve and install under `base-lyra`.** Seventeen come from `@ai-elements`. **Message Scroller** and **Questionnaire** are real, but they're shadcn `registry:ui` items, not AI Elements. They're published in `base-lyra` and in the Radix styles `radix-lyra` and `radix-nova`, but not in `new-york-v4`.
- **AI Elements ships one source, written against Radix-flavoured shadcn.** Every item lives at `registry/default/ai-elements/*`, and there are no per-style variants. Its `registryDependencies` (`collapsible`, `button`, `command`, `dialog`, `hover-card`, …) resolve against the project's style, so the primitives arrive as Base UI. When the CLI copies the files, it rewrites `asChild` to Base UI's `render` prop. It doesn't touch the Radix data attributes, Radix event names, or Radix-only props.
- **15 of the 17 AI Elements files typecheck on Base UI unchanged. Context and Prompt Input don't.** Several of the ones that typecheck still carry `data-[state=…]` styling that Base UI never matches. That bug is cosmetic and silent.
- **Six components import AI SDK types, and all of them use `import type`:** Conversation, Message, Tool, Confirmation, Context and Prompt Input. `ai` is still listed as a registry `dependency`, so the CLI adds `ai` (7.0.127 at the time of writing) to `dependencies`. No component imports a runtime value from `ai`.

Upstream doesn't support Base UI yet. [vercel/ai-elements#383](https://github.com/vercel/ai-elements/issues/383) ("Support for shadcn Base UI version?") and [#489](https://github.com/vercel/ai-elements/issues/489) are open, and [#498](https://github.com/vercel/ai-elements/issues/498) reports the Prompt Input menu bug described below.

## Per component

"Radix assumption" lists what's left after the CLI's `asChild` → `render` rewrite. Base UI's Collapsible emits `data-panel-open` on the trigger and `data-open`/`data-closed` on the panel (`collapsible/panel/CollapsiblePanelDataAttributes` and the trigger's `panelOpen` attribute in the published `@base-ui/react@1.8.0`). It never emits `data-state`.

| Spec name | Registry item | Typechecks on base-lyra | Radix assumption left after install | Base UI fix | AI SDK types | Notable deps |
| --- | --- | --- | --- | --- | --- | --- |
| Conversation | `@ai-elements/conversation` | yes | none | none | `UIMessage`, used only by `ConversationDownload` / `messagesToMarkdown` | `use-stick-to-bottom` |
| Message | `@ai-elements/message` | yes | none (Tooltip trigger rewritten to `render`) | none | `UIMessage["role"]` for the `from` prop | `streamdown` + `@streamdown/{cjk,code,math,mermaid}` |
| Message Scroller | `@shadcn/message-scroller` (shadcn, not AI Elements) | yes | none, it's native to the Base style | none | none in the component. Its docs demo uses `@ai-sdk/react` `useChat` | `@shadcn/react` |
| Tool | `@ai-elements/tool` | yes | `group-data-[state=open]:rotate-180` on the chevron, and `data-[state=open/closed]:animate-*` on the content | `group-data-[panel-open]:…`, and `data-open:` / `data-closed:` | `ToolUIPart`, `DynamicToolUIPart` (the `state` union drives the status badge) | pulls `code-block` |
| Reasoning | `@ai-elements/reasoning` | yes | `@radix-ui/react-use-controllable-state`, `data-[state=…]` animation classes | The hook is standalone and works without Radix primitives. Keep it, or replace it with a few lines of controlled-state code, because Base UI exports no public `useControlled`. Change the classes to `data-open:` / `data-closed:` | none | `streamdown`, `shimmer` |
| Chain of Thought | `@ai-elements/chain-of-thought` | yes | as Reasoning | as Reasoning | none | — |
| Confirmation | `@ai-elements/confirmation` | yes | none | none | `ToolUIPart["state"]` plus its `approval` shape | uses `alert` and `button` |
| Questionnaire | `@shadcn/questionnaire` (shadcn, not AI Elements) | yes | none, it's native | none | none | `@shadcn/react` |
| Queue | `@ai-elements/queue` | yes | `group-data-[state=closed]:-rotate-90` | `group-data-[panel-open]:` (inverted) | none | `scroll-area` |
| Prompt Input | `@ai-elements/prompt-input` | **no**, 6 errors | `DropdownMenuItem onSelect` in `PromptInputActionAddAttachments` / `…AddScreenshot`. Base UI's Menu has no `onSelect`, so the handler never fires ([ai-elements#498](https://github.com/vercel/ai-elements/issues/498)). Event handlers are typed as DOM `Event`, not `BaseUIEvent`. `HoverCard openDelay`/`closeDelay` | Use `onClick`. Retype the handlers. Move `delay`/`closeDelay` to `HoverCardTrigger`, where Base UI's `PreviewCard.Trigger` takes them | `ChatStatus`, `FileUIPart`, `SourceDocumentUIPart` | `nanoid`, `command` (`cmdk`) |
| Suggestion | `@ai-elements/suggestion` | yes | none | none | none | `scroll-area` |
| Context | `@ai-elements/context` | **no**, 3 errors | `HoverCard openDelay={0} closeDelay={0}` (Root props in Radix) | Move them to `HoverCardTrigger` (`delay`, `closeDelay`) | `LanguageModelUsage`. It also reads `reasoningTokens` and `cachedInputTokens`, which `ai@7` moved under `outputTokenDetails` / `inputTokenDetails`. This is version skew, not Base UI | `tokenlens` (runtime, prices tokens by model id) |
| Commit | `@ai-elements/commit` | yes | none | none | none | `avatar`, `collapsible` |
| Code Block | `@ai-elements/code-block` | yes | Language selector uses `SelectValue`. Base UI's `Select.Value` shows the raw value unless `items` is given to the root | Pass `items` if the selector is used | none | `shiki` |
| File Tree | `@ai-elements/file-tree` | yes | none | none | none | — |
| Model Selector | `@ai-elements/model-selector` | yes | none. It sits on base-lyra's `CommandDialog` (Base UI Dialog + `cmdk`) | none | none | `cmdk`, which pulls `@radix-ui/react-dialog` transitively but doesn't render it here. `ModelSelectorLogo` loads `https://models.dev/logos/<provider>.svg` at runtime |
| Terminal | `@ai-elements/terminal` | yes | none | none | none | `ansi-to-react` (peer range includes React 19) |
| Stack Trace | `@ai-elements/stack-trace` | yes | `@radix-ui/react-use-controllable-state`, `data-[state=…]` classes | as Reasoning | none | — |
| Shimmer | `@ai-elements/shimmer` | yes | none | none | none | `motion` |

The scratch `tsc` run also flagged `ui/scroll-area.tsx` from the base-lyra registry, because of an unused `React` import under the Client's `noUnusedLocals`. That's a one-line fix.

## Installing them into the Client

- **Install without `-o`.** With `--overwrite`, the CLI re-fetches `button`, `badge`, `collapsible`, `alert`, `input`, `textarea` and the hand-edited `ai-elements/conversation.tsx`. The primitives come back with formatting-only diffs (`oxfmt` versus the registry's style), but `conversation.tsx` gets the `UIMessage` download helpers the Client had removed.
- The CLI writes imports as `#/components/ui/button.tsx` and `#/lib/utils.ts`, which matches the Client's aliases and `allowImportingTsExtensions`.
- Adding the full set adds these runtime dependencies: `ai`, `streamdown` and four `@streamdown/*` plugins, `shiki`, `motion`, `cmdk`, `tokenlens`, `nanoid`, `ansi-to-react`, `@shadcn/react`, and `@radix-ui/react-use-controllable-state`. Because every use of `ai` is type-only, `ai` could move to `devDependencies`. Alternatively, the six imports could be rewritten to kestrel-owned types in the mapping module that #461 calls for, which removes `ai` entirely.
- `tooltip` arrives with a note to wrap the app in `TooltipProvider`. Base UI tooltips work without it, but it groups their open delays.

## What this means for #461 and #460

- Use Message Scroller and Questionnaire under shadcn's name and registry (`@shadcn/…`, into `components/ui`), not `@ai-elements`. shadcn also publishes its own `message`, `bubble`, `attachment` and `marker` in `base-lyra`. Those are distinct from AI Elements' `Message`, and installing both doesn't collide because they land in different directories.
- None of the components needs a fork. The edits fall into four kinds: `data-[state=…]` → Base UI data attributes (Tool, Reasoning, Chain of Thought, Queue, Stack Trace), hover-card delay props (Context, Prompt Input), `onSelect` → `onClick` (Prompt Input), and the `ai@7` usage field names (Context).
- Conversation, Message, Tool, Confirmation, Context and Prompt Input take AI SDK shapes. Message's dependency is only `role`. Tool and Confirmation depend on the `ToolUIPart` `state` union (`input-streaming`, `input-available`, `approval-requested`, `output-available`, `output-error`, …). That union is the main contract #461's mapping module has to produce from Activity.
- Model Selector's logos come from a third-party host at runtime. #460's setup flow should vendor the provider marks, or accept the outbound request.
