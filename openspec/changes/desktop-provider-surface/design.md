# Design

See `proposal.md` for motivation and `specs/desktop/` for the behavior contract.

## Context

- `crates/jcode-desktop-harness/src/accounts.rs` already builds the full provider inventory from `jcode auth status --json` plus `jcode usage --json` into `Account { id, display_name, status, auth_kind, method, limits, usage_reports }`, and the sidebar (`workspace.rs`, `render_accounts`) renders it read-only.
- The composer account switcher (`panel_provider_picker.rs`) and the Accounts panel (`panel_login.rs`) instead build rows from `LoginState.providers`, which is `jcode_sdk::auth::providers()` — a static, `&'static str` catalog filtered by the SDK's login-surface policy. Providers outside that catalog (OpenCode Go, named OpenAI-compatible profiles) never get a row, and the switcher also filters to connected entries, which is why a fully configured machine shows only two.
- Model availability is runtime-driven: `ApiEvent::RuntimeInfo.routes` feeds `input.set_model_routes`, and route specifications are derived with the runtime's own routing policy (`input_model_menu::route_spec`, `RouteSelection`).
- Credential writes go through `JcodeClient::set_api_key`, whose harness handler (`SetApiKey`) maps only `claude-api`, `openai-api`, `openrouter`, `cursor`, `gemini`, and `jcode` to env files. Named profiles and other API-key providers have no protocol path today; only the CLI (`jcode login`, `jcode provider add --api-key-stdin`) can configure them.
- The harness already shells out to the trusted companion `jcode` binary for `auth status`/`usage`, and the SDK's `AuthClient` shells out for sign-in, so a CLI-backed admin path is consistent with existing patterns.
- Desktop can attach to local, SSH, and managed-cloud runtimes; configuration must not assume the local filesystem is the runtime's.

## Goals / Non-Goals

**Goals:**

- One provider inventory model used by every provider surface, with no duplicated filtering policy between harness and UI.
- A switcher that lists all configured providers and switches using runtime routing.
- A Providers surface with add, key, remove, test, and switch actions that works for every provider the runtime reports.
- A protocol-level admin path in Jcode so Desktop never edits runtime files directly, including for remote runtimes where the desktop cannot touch the runtime's disk.
- Compatibility with older runtimes that lack the new admin requests.

**Non-Goals:**

- Editing an existing named profile's base URL or model list (re-adding with overwrite is the supported path for now).
- Reordering provider priority or changing multi-account pool policy; existing account-pool ordering stays.
- Configuring providers on remote/cloud runtimes; those surfaces stay read-only with guidance.
- Adding new sign-in protocols for providers the runtime cannot authenticate.
- Model catalog management (models remain runtime-owned and read-only).

## Decisions

### D1 — Inventory = runtime auth report merged with runtime model routes

Build a harness-owned `ProviderInventory`:

```text
ProviderEntry {
  id, display_name, auth_kind, method, status,        // auth report
  credential_source,                                   // report/usage extras
  models: Vec<RouteModel { spec, model, available }>,  // runtime routes
  available_model_count,
  usage, limits,                                       // existing Account data
  supported_actions: Vec<ProviderAction>,              // derived
}
```

The report is authoritative for *existence and credential state*; routes are authoritative for *model availability and switch targets*. Routes correlate to a provider by `api_method` (`openai-compatible:<profile>`, `<id>`, `<id>-key`) and by provider label/display name, using the same policy as `input.rs`'s provider spec matching so the switcher and `/model` agree.

*Alternatives:* routes-only (loses credential state and account labels), auth-report-only (cannot pick a switch target), static catalog (today's bug). Rejected.

### D2 — Owned provider identity replaces `LoginProvider` in row models

`LoginProvider` is `Copy` and uses `&'static str`, so it cannot represent runtime providers. `AccountRow` and the picker move to owned identity (`ProviderId`, `ProviderEntry`), while the SDK catalog supplies a `SignInSupport` lookup (which providers support OAuth, device code, or API-key entry). Multi-account rows keep the existing credential `key` and login labels.

*Alternative:* keep `LoginProvider` and synthesize leaked strings — rejected (lifetime hacks, no path for dynamic fields).

### D3 — Configuration goes through Jcode, not Desktop file edits

- Generalize the harness `SetApiKey`/`ClearApiKey` handler: replace the hardcoded `credential_binding` match with a provider-metadata lookup (`api_key_env`, `env_file`) so any API-key provider can be written, preserving the current six-provider behavior and error codes.
- Add `ProviderAdd` and `ProviderRemove` requests to the harness API and `JcodeClient::add_provider_profile` / `remove_provider_profile` in the SDK. The server reuses the CLI's `configure_provider_profile` logic, extracted from `src/cli/commands/provider_setup.rs` into shared code, so CLI and daemon stay in sync.
- Add the missing CLI parity pieces: `jcode login --api-key-stdin` (avoid argv secrets) and `jcode provider remove <name>` (harness removes named profiles only).

*Alternatives:* Desktop spawns `jcode provider add` locally — simple but breaks remote runtimes and duplicates output parsing; used only as a fallback if the protocol route is rejected. Writing `config.toml`/env files from Desktop — rejected: bypasses validation, concurrency, and remote semantics.

### D4 — Surfaces reuse one panel and one feed

- The composer account pill opens the switcher: search, "In use" first, then connected providers, then not-configured providers; a "Manage providers…" row opens the full surface.
- The Providers surface is a panel mode (opened from the switcher, the sidebar accounts header, and the command palette) listing every provider with pills for Use, Set key / Sign in, Test, and an overflow action for Remove (named profiles only). It reuses `LoginState` and the account poller; no second inventory implementation.
- An "Add provider" dialog collects name, base URL, default model, optional context window, optional env-var name, and API key.
- Visual rules follow `AGENTS.md`: fully rounded pills, list rows on a subtle fill, no decorative status dots, no card-in-card nesting.

### D5 — Refresh is event- and action-driven

Credential actions call the existing refresh signal (`accounts::request_refresh`) and the runtime's `RefreshRuntime` command (`notify_auth_changed` already pushes new routes). The provider poller stays at its slow cadence otherwise, so no extra subprocesses per frame.

### D6 — Secret handling and scoping

Key entry reuses the masked `LoginInput`; keys are validated against the server's rules (non-empty, trimmed, no CR/LF/NUL) before submission, travel only in the protocol payload, and never enter logs, transcripts, or error strings. Configuration actions are gated on the attached runtime being local, using the existing remote detection in the panel; remote surfaces render read-only guidance.

### D7 — Degrade on older runtimes

The SDK exposes the runtime's `capabilities`; when `provider_add`/`provider_remove` or generalized key storage is absent, Desktop hides the add/remove actions and key input for unsupported providers, keeping the CLI guidance. The inventory/switcher fix does not depend on the protocol, so it works with today's runtime.

## Risks / Trade-offs

- **Provider identity ambiguity** (multiple labels or routes per provider) → canonicalize on credential key and `api_method`; cover OpenRouter, OpenCode Go, a named profile, Cursor, and an OAuth provider in fixtures.
- **Policy drift** between switcher, `/model`, and sidebar → one harness merge function; the UI only renders and dispatches.
- **Server behavior change for `set_api_key`** could alter errors for previously rejected providers → keep the existing supported set and error codes, add explicit tests, and gate new bindings on provider metadata.
- **Protocol version skew** → capability-gated degradation (D7) plus the existing `verify-runtime-source.py` preflight in CI.
- **Removal deletes credentials** → confirmation step, named profiles only, and built-in ids are never treated as removable.
- **Cross-repo sequencing** → land the Jcode SDK/harness/CLI changes first, then Desktop; pin the runtime revision and verify with the preflight; roll back by reverting the Desktop surface while stored profiles remain valid.
- **Empty model catalogs** for a configured provider → show "no usable model" state and an actionable message rather than blocking the row.

## Migration Plan

1. Jcode repository: shared provider-add/remove logic, generalized key storage, CLI parity, SDK methods, tests; commit and push.
2. Desktop: harness inventory + admin client behind capability checks; then UI switcher/surface; tests and screenshot acceptance; commit and push.
3. Rollout: no data migration; existing config and env files are read as-is. Older runtimes keep today's surface minus add/remove. Rollback is reverting the Desktop changes; Jcode's new APIs are additive.

## Test Strategy

- Harness: inventory merge, route correlation, action derivation, CLI-argument construction, and validation rules as GPUI-free unit tests.
- UI: switcher row construction from a runtime fixture (all providers, connected first), provider panel actions, add-form validation, remote read-only mode, and secret masking.
- Acceptance: extend the screenshot/fixture harness with a provider inventory fixture, and run the existing runtime preflight plus `justrust test` for touched crates.
