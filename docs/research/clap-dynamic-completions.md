# clap_complete's dynamic completions for `kestrel completions`

Research checked 3 October 2026 against the `clap_complete` 4.6.11 source (the latest release, 15 September 2026, read from the published crate), clap's issue tracker and changelog, and the code of CLIs that ship the engine. `kestrel-client` is on clap 4.6.6; `clap_complete` 4.6.11 requires `clap ^4.6.6`, so adding it moves nothing.

Spec #460 asks for `kestrel completions <shell>` for bash, zsh and fish, completing Workspace, Agent, Profile, harness and Sign-in Method names from the control plane, through `clap_complete`'s dynamic engine.

## Answer in brief

- **Shippable, behind a feature named unstable.** The engine is the `unstable-dynamic` feature; clap's policy is that experimental features [may break between minor releases](https://github.com/clap-rs/clap/blob/master/src/_features.rs). The [stabilization tracking issue #3166](https://github.com/clap-rs/clap/issues/3166) is still open. Cargo, rustup, jj, just, devenv and others ship it today. Pin `clap_complete` to an exact minor and the risk is a small, compile-time API break on upgrade.
- **Installed by the shell sourcing the binary's own output.** `COMPLETE=<shell> kestrel` prints a registration script, and every Tab re-runs `kestrel` with `COMPLETE` set.
- **No timeout anywhere.** Neither the engine nor the bash, zsh and fish scripts bound how long a completer runs. The shell waits until `kestrel` exits, so the completer must bound its own network call and return nothing on failure.
- **A completer sees only the word under the cursor.** It can't see `--control-plane` or `--organization` on the same line. [#5784](https://github.com/clap-rs/clap/issues/5784) is open and the maintainer calls it low priority. The workaround in use (jj's) is to re-parse `std::env::args_os()` inside the completer.

## Stability

- **Feature gate.** `engine` (the completers) and `env` (`CompleteEnv`, the shell adapters) are compiled only under `unstable-dynamic` ([`lib.rs`](https://docs.rs/crate/clap_complete/4.6.11/source/src/lib.rs)). That feature also turns on `clap/unstable-ext`, the `ArgExt` hook that `add = ArgValueCompleter::new(..)` uses ([`Cargo.toml`](https://docs.rs/crate/clap_complete/4.6.11/source/Cargo.toml)). clap documents experimental features as ones that ["may contain breaking changes between minor releases"](https://github.com/clap-rs/clap/blob/master/src/_features.rs).
- **Tracking issue [#3166](https://github.com/clap-rs/clap/issues/3166)**, open and last updated June 2026. Still unchecked: zsh and fish appear only as "communicate with" boxes (although both adapters exist and are fixed release by release), along with handling of space, non-UTF-8 paths, quoting verification, and lazy loading ([#5668](https://github.com/clap-rs/clap/issues/5668)). The maintainer's stated priority is parity with the static generator and with cargo's hand-written completions. Cargo is the engine's main test case.
- **Churn in practice.** The 4.6 changelog ([CHANGELOG](https://github.com/clap-rs/clap/blob/master/clap_complete/CHANGELOG.md)) shows only additions and fixes to the dynamic engine since March 2026:
  - `ValueCompleter::complete_at` (4.6.3);
  - `PossibleValue` helpers (4.6.8);
  - zsh sort order (4.6.4) and fish escaping (4.6.5).

  None broke the `ArgValueCompleter`/`CompleteEnv` API that Kestrel would use. One API break came at 4.5.x, when [#5671](https://github.com/clap-rs/clap/issues/5671) replaced a `complete` subcommand with env-var activation ([#5947](https://github.com/clap-rs/clap/issues/5947)).
- **Open bugs that touch Kestrel's shape.** Which shells are built by default is itself open ([#6168](https://github.com/clap-rs/clap/issues/6168)). Bash breaks when the binary name contains a hyphen ([#6421](https://github.com/clap-rs/clap/issues/6421)), which `kestrel` doesn't. Several open fish and zsh bugs concern the *static* (AOT) generator, not this engine ([#6295](https://github.com/clap-rs/clap/issues/6295)).

## How it works, and what it costs

- **Activation.** `CompleteEnv::with_factory(Client::command).complete()` runs first in `main`. Without `COMPLETE` set, or with it empty or `0`, it returns and the program runs as normal. With it set, it does one of two things and exits ([`env/mod.rs`](https://docs.rs/crate/clap_complete/4.6.11/source/src/env/mod.rs)):
  - with no further arguments, it prints the shell's registration script;
  - after `--`, it builds the `Command`, completes the words and prints candidates.

  `stdout` must not be written before it runs. It unsets `COMPLETE` so that child processes don't complete too. `.var("KESTREL_COMPLETE")` renames the variable, as cargo does with `CARGO_COMPLETE` ([cargo `main.rs`](https://github.com/rust-lang/cargo/blob/master/src/bin/cargo/main.rs)).
- **Per Tab:** one process spawn, one `Command` build, one engine walk, plus whatever the completer does. There's no daemon and no cache.
- **Per shell start** (bash and zsh in the recommended setup): one more spawn to print the registration script.
- **New dependencies:** `shlex` and `is_executable`. `clap_lex` is already in the tree through clap.
- **Two completer shapes** ([`engine/custom.rs`](https://docs.rs/crate/clap_complete/4.6.11/source/src/engine/custom.rs)). Both are synchronous: `Fn(..) -> Vec<CompletionCandidate> + Send + Sync`, with no `Result`, no async and no context.
  - `ArgValueCandidates::new(|| Vec<CompletionCandidate>)` returns everything, and the engine filters by prefix ([`engine/complete.rs`](https://docs.rs/crate/clap_complete/4.6.11/source/src/engine/complete.rs), `complete_value_candidates`).
  - `ArgValueCompleter::new(|current: &OsStr| ..)` receives the partial word and must filter itself.
- **Descriptions.** `CompletionCandidate::help` reaches fish (tab-separated) and zsh (`value:help`). Bash prints values only ([`env/shells.rs`](https://docs.rs/crate/clap_complete/4.6.11/source/src/env/shells.rs)). A Workspace's Brief or an Agent's harness could ride along as help in fish and zsh.

## Installation per shell

From the [`env` module docs](https://docs.rs/crate/clap_complete/4.6.11/source/src/env/mod.rs). Upstream recommends generating the script at shell start rather than saving it, because "the interface is unstable and a mismatch between the shell code and `your_program` may result in either invalid completions or no completions".

| Shell | Line | Notes |
|---|---|---|
| bash | `source <(COMPLETE=bash kestrel)` in `~/.bashrc` | Registers with `complete -o nospace -o bashdefault -o nosort -F` (bash 4.4 and later). |
| zsh | `source <(COMPLETE=zsh kestrel)` in `~/.zshrc` | The output starts with `#compdef kestrel`, so it can also be saved as `_kestrel` on `fpath`. That trades the per-start spawn for staleness on upgrade. |
| fish | `COMPLETE=fish kestrel \| source` in `~/.config/fish/completions/kestrel.fish` | fish autoloads this file on first completion, so it's lazy with no startup cost. |

**`kestrel completions <shell>`.** The spec's subcommand can print the same script without the env var. `Shells::builtins().completer("zsh")` returns a public `EnvCompleter` whose `write_registration(var, name, bin, completer, buf)` writes the script. The script's callback still uses `COMPLETE=<shell> kestrel -- …`, so `CompleteEnv` stays in `main` either way. Upstream's guidance implies the subcommand should tell people to `source <(kestrel completions zsh)` rather than redirect it to a file. The script calls back into whatever path `kestrel` was invoked as (`args[0]`), made absolute when it has a directory component; `.completer("kestrel")` forces `PATH` lookup instead.

## A slow or unreachable control plane

What each shell's script does, from [`env/shells.rs`](https://docs.rs/crate/clap_complete/4.6.11/source/src/env/shells.rs):

- **No timeout in any of the three scripts.** Each runs `kestrel` in a command substitution and blocks until it exits. A hung connection hangs the prompt until the person presses Ctrl-C.
- **bash:** a non-zero exit unsets `COMPREPLY`. Empty output falls back to bash's default (filename) completion through `-o bashdefault`. **stderr reaches the terminal**, mid-line.
- **zsh:** stderr is sent to `/dev/null`. Empty output offers nothing.
- **fish:** registered `--exclusive`, so empty output offers nothing, with no file fallback. stderr reaches the terminal.

So the completer itself must:

1. **Bound the call.** A refused loopback connection (the common "not running" case) fails in microseconds. A hung or unroutable `KESTREL_CONTROL_PLANE` doesn't. A total deadline well under a second keeps Tab responsive: a `reqwest` `connect_timeout` plus `tokio::time::timeout` around the request.
2. **Return an empty list on any error, and print nothing.** This is unlike jj, which `eprintln!`s, and bash and fish would show the text. Don't call `exit` and don't panic: `complete` turns its own errors into a clap error exit, which bash treats as "no completions".
3. **Run outside Kestrel's tokio runtime.** Today `main` is `#[tokio::main(flavor = "current_thread")]`, and a completer is a synchronous callback. Starting a runtime inside it would panic ("Cannot start a runtime from within a runtime"). `futures::executor::block_on` on reqwest would starve the single-thread reactor. The clean shape is a synchronous `main` that calls `CompleteEnv::…complete()` before building the runtime. Each completer then builds its own small current-thread runtime for one bounded request. Feldera's `fda`, the closest real case (pipeline names from its server), instead calls `complete()` inside a *multi-thread* runtime and uses `futures::executor::block_on` with no deadline beyond the user's `--timeout` ([`fda/src/cli.rs`](https://github.com/feldera/feldera/blob/main/crates/fda/src/cli.rs), [`fda/src/main.rs`](https://github.com/feldera/feldera/blob/main/crates/fda/src/main.rs)).
4. **Find the control plane and Organization without being told.** A completer gets only the current word ([#5784](https://github.com/clap-rs/clap/issues/5784)). Environment variables (`KESTREL_CONTROL_PLANE`, `KESTREL_ORGANIZATION`) are inherited. A `--control-plane` or `--organization` typed earlier on the line isn't visible unless the completer re-parses `std::env::args_os().skip(2)` (after `kestrel --`) with `Command::ignore_errors(true)`, as jj's `get_jj_command` does ([jj `complete.rs`](https://github.com/jj-vcs/jj/blob/main/cli/src/complete.rs)). Workspace names are scoped by Organization, so the completer should reuse the CLI's organization resolution: flag, then env, then `.kestrel/organization`, then the only Organization.

Harness and Sign-in Method names come from the catalogue. The control plane serves them like the rest, so the same bounded call covers them. If the catalogue also lived in the CLI, those could complete offline as `ArgValueCandidates`.

## Who ships it

From a GitHub code search for `CompleteEnv::with_factory`; the call sites in cargo, jj, rustup and feldera were read, the rest are search hits:

- **[cargo](https://github.com/rust-lang/cargo/blob/master/src/bin/cargo/main.rs):** only on nightly or dev channels, under `CARGO_COMPLETE`.
- **[jj](https://github.com/jj-vcs/jj/blob/main/cli/src/complete.rs):** `clap_complete = { version = "4.6.5", features = ["unstable-dynamic"] }`. It is the largest user. It completes bookmarks, revisions and config keys by shelling out to itself.
- **[rustup](https://github.com/rust-lang/rustup/blob/master/src/cli/rustup_mode.rs):** under `RUSTUP_COMPLETE`.
- **Also:** [just](https://github.com/casey/just/blob/master/src/main.rs), [devenv](https://github.com/cachix/devenv), [bacon](https://github.com/Canop/bacon), [slumber](https://github.com/LucasPickering/slumber), [prek](https://github.com/j178/prek), [Spin](https://github.com/spinframework/spin), [nixops4](https://github.com/nixops4/nixops4) and NVIDIA [OpenShell](https://github.com/NVIDIA/OpenShell).
- **[feldera `fda`](https://github.com/feldera/feldera/blob/main/crates/fda/src/cli.rs):** the one found that completes names over HTTP from a server, as Kestrel would.

## Fit for kestrel

The engine fits the spec's ask, and it is what the projects Kestrel resembles use. What Kestrel would write:

- `clap_complete = { version = "=4.6", features = ["unstable-dynamic"] }`, or a `~4.6` pin. Re-check the API on each bump.
- A synchronous `main` that runs `CompleteEnv` before the tokio runtime.
- A `completions <shell>` subcommand that prints `write_registration` output and the one line to add per shell.
- One shared completer helper. It resolves the control plane and Organization from env and a lenient re-parse of the line, makes one request under a sub-second deadline, and returns `[]` silently on any failure. Workspace, Agent and Profile args use `ArgValueCandidates` over it, with each name's description as `help` for fish and zsh.
- Tests of the engine through `CompleteEnv::try_complete(args, cwd)` with `COMPLETE` set, or through `clap_complete::engine::complete(cmd, args, index, cwd)` directly against an in-process control plane. No shell needed.
