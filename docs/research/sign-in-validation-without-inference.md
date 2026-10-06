# Sign-in validation without inference

Researched 2026-10-05 against provider documentation and first-party source. No credentials were tested. This note informs the sign-in catalogue decision; it does not change that decision.

## Findings

| Sign-in material | Non-inference mechanism | Evidence and limits |
| --- | --- | --- |
| Anthropic API key | Authenticated `GET /v1/models`, or retrieve a model | Lists model metadata. Success is evidence that the supplied credential was accepted for this endpoint. It does not demonstrate generation, available credits, remaining generation quota, or the harness using the intended material. [Models reference](https://platform.claude.com/docs/en/api/models), [API authentication](https://platform.claude.com/docs/en/api/overview) |
| Anthropic API key | `POST /v1/messages/count_tokens` | Selects a model and returns an input-token estimate without generating a response. Anthropic explicitly documents this as free, with separate rate limits from message creation. It is not documented as a guarantee of billing readiness or generation entitlement. [Token counting](https://platform.claude.com/docs/en/build-with-claude/token-counting) |
| OpenAI API key | Authenticated `GET /v1/models`, or retrieve a model | Lists model metadata using bearer API-key authentication. It performs no generation. Acceptance establishes endpoint access, not credits, available generation quota, or harness correctness. [Models reference](https://developers.openai.com/api/reference/resources/models/methods/list) |
| Claude subscription setup-token | `claude auth status` | Documents logged-in status and authentication method as JSON. The public contract does not promise online token validation, freshness checking, model entitlement, or quota checking. `setup-token` creates a long-lived OAuth token for CI/scripts and requires a subscription; it is distinct from a Console API key. [CLI reference](https://code.claude.com/docs/en/cli-reference) |
| Codex ChatGPT device-auth material | `codex login status` | Documentation explicitly describes credential presence. The CLI source loads saved auth and prints the ChatGPT mode without an explicit online token probe in this command. Do not label this server-validated. [Command documentation](https://learn.chatgpt.com/docs/developer-commands?surface=cli), [CLI implementation](https://github.com/openai/codex/blob/main/codex-rs/cli/src/login.rs#L421) |
| OpenCode Go/Zen console key | Model catalogue endpoints | Zen documents `/zen/v1/models`, but the first-party implementation includes a public catalogue path without a key. Listing a catalogue is therefore not inherently credential validation. No documented, stable credential-validation contract was established in this research. [Zen documentation](https://opencode.ai/docs/zen/), [Zen route source](https://github.com/anomalyco/opencode/blob/dev/packages/console/app/src/routes/zen/v1/models.ts), [Go documentation](https://opencode.ai/docs/go/) |

## Interpret failures narrowly

Authentication, endpoint permission, model entitlement, billing, and runtime ability are different questions. Anthropic distinguishes authentication errors, permission errors, billing errors, and rate limits. OpenAI likewise documents invalid credentials, endpoint permissions, organization/project restrictions, IP restrictions, and exhausted credits separately. A metadata endpoint refusal is not sufficient to declare the key unusable for inference: endpoint permissions can be restricted independently. [Anthropic errors](https://platform.claude.com/docs/en/api/errors), [OpenAI errors](https://developers.openai.com/api/docs/guides/error-codes)

The inference from a successful authenticated metadata request is deliberately narrow: the provider accepted these credentials for this request at this time. A timeout or provider outage supplies no negative authentication evidence. An explicit authentication response can establish a problem, but preserve its provider reason rather than treating every 401 as a mistyped secret; OpenAI also uses 401 for organization and IP restrictions. [OpenAI errors](https://developers.openai.com/api/docs/guides/error-codes)

## Unsupported assumptions

- The Console API mechanisms above are not established as supported validators for Claude subscription setup-token or Codex ChatGPT credentials. OAuth material is not interchangeable with a provider API key.
- Claude's status command is not established as either purely local or an online validation contract by the documentation reviewed. Only the documented status semantics should be promised.
- Public model visibility does not establish per-user model entitlement. In particular, OpenCode model listing can be public.
- No generic provider endpoint was established that certifies all of authentication, model access, billing readiness, remaining quota, and successful harness execution without a real inference request.
- The models references describe metadata rather than generation; an explicit universal no-fee guarantee for model-list requests was not found. Anthropic token counting does carry an explicit free-to-use statement.

## Planning implication

Use non-inference checks where supported and describe the resulting evidence as credential acceptance or login completion. Save unsupported imports with narrower evidence. Optional generation checks or actual Sessions can establish that a specific material revision worked through a particular harness and model at a particular time. Requiring generation on every save is unnecessary if setup only promises accepted credentials, rather than demonstrated runtime ability.

## Optional-test model selection and credential refresh

Claude Code documents a runtime default with configuration/organization overrides. Codex has built-in configuration defaults, and its ACP adapter exposes the model marked as default by Codex App Server. These supply defaults for an optional test, not a guarantee that the selected account can use them; report the model actually resolved. [Claude model configuration](https://code.claude.com/docs/en/model-config), [Codex configuration](https://learn.chatgpt.com/docs/config-file/config-basic), [Codex ACP recommended values](https://github.com/agentclientprotocol/codex-acp/blob/main/docs/air-extensions.md#recommended-config-values)

OpenCode selects from explicit configuration, last-used model and an internal priority order. That default does not establish which saved provider credential will be used. For an isolated credential test, require an explicit provider/model and enable only that provider, excluding unrelated credentials and configuration. The isolation requirement is an inference from the documented selection/allowlist behavior. [OpenCode models](https://opencode.ai/docs/models/), [provider allowlist](https://opencode.ai/docs/config/#enabled-providers)

OpenAI's CI/CD guidance requires a single machine or serialized job stream per managed `auth.json` copy and persistence of the refreshed file. It identifies concurrent token rotation as a refresh-failure cause. An optional test must therefore coordinate with Sessions sharing that managed login rather than treating a copied file as read-only; this conclusion follows from the vendor's explicit concurrency restriction. Kestrel already serializes Codex Sessions per Profile by default, and the new test must join that coordination and safely return refreshed material. [OpenAI CI/CD authentication](https://learn.chatgpt.com/docs/auth/ci-cd-auth)

The official OpenCode Console entry is `https://opencode.ai/auth`, linked from its first-party documentation configuration. [Documentation configuration](https://github.com/anomalyco/opencode/blob/dev/packages/web/config.mjs)
