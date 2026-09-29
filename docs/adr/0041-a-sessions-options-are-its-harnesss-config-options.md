# A Session's options are its harness's config options

ACP gives a Client a Session's model, mode and thought level as one list of config options, each tagged with a category, and resends it whenever a value or the offered values change. `current_mode_update` is the older way to report the mode. Kestrel read the model once, at setup, and dropped every later update, so the model it showed could go stale and nobody could see or change the mode.

A Session's **options** are that list. The supervisor keeps the whole list with its offered and current values, and the latest `current_mode_update` sets the Mode-category option. The Session read exposes the list, the harness's title and the available commands. It also exposes usage as of the last Turn or trailing end. The Workspace follow streams live usage as transient Session state, at most once a second. The supervisor drops an update that changes nothing, a changed Session sends at most one Organization change notice a second (ADR-0035), and the Session read serves an `ETag`, because harnesses resend identical lists and a reconnecting Client refetches every view it subscribes to. None of this starts or extends trailing (ADR-0040).

- **Declared by category.** A Trigger and `workspace open` declare a model, a mode and a thought level, never a harness's option id. The supervisor checks each at setup against what the harness offers and fails the Session on a value it does not offer, as it already does for a model.
- **Changed between Turns.** A person changes an option while the Session is queued, which sets the requested value, or while it is waiting or trailing. A change during a working Turn is refused, because what a harness does with an option changed mid-Turn is undefined. A person's change is a shared-state Transcript entry naming who made it. An agent's own mode switch already shows as its `switch_mode` tool call.
- **The cache warning.** Changing a Model, ThoughtLevel or ModelConfig option warns that the next Turn re-reads the context, of the size usage last reported, without the prompt cache. Anthropic invalidates the message cache on a thinking or effort change as well as on a model change. An adapter may declare a category that keeps the cache for a given model, such as Claude's per-message effort, and no warning shows for it. A mode change never warns.
- **A command is its own Turn.** A Held Message that starts with an offered command drains as a Turn of its own, and its whole text is the command's input. Messages held before it drain first; those held after it form the next Turn. This amends the one-Turn drain of held messages.

Mode is a choice that governance constrains. A mode that stops the agent asking permission would let it act outside Policy, so at `0.4` Policy governs which modes an operator, a Trigger, or the agent itself may select, and the audit records each change.

## Considered options

**Only model and mode.** Rejected: the list arrives whole, and a Client can render any category it has not seen as a selector.

**Declaring options by the harness's own ids.** Rejected: a Trigger would be bound to one adapter release's option names.

**Holding a change until the next Turn, like a Held Message.** Rejected: it adds a second kind of held state for no gain, and during a Turn the change is refused instead.

**Persisting every usage update and sending a notice for it.** Rejected: Claude reports usage at every model step, and each report would make every Client in the Organization refetch.

## Consequences

- The browser's composer cycles the mode on Shift+Tab, as terminal harnesses do, and Escape leaves the composer, which a screen reader announces.
- A mode that bypasses permission requests is selectable at `0.3`, where every request is already allowed. At `0.4` it must be governed before Policy can enforce anything at the execution layer.
