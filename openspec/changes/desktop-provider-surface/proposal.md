# Proposal

## Why

Jcode Desktop currently exposes two different provider universes. The sidebar lists every provider the runtime reports (`jcode auth status`), but the composer's account switcher and the Accounts panel render only the SDK's built-in sign-in catalog, so a session with several configured providers (for example OpenRouter, OpenCode Go, a named Neuralwatt/OpenAI-compatible profile, and Cursor) is offered only the two that happen to be in that catalog. There is also no way to add a provider or a named OpenAI-compatible profile from Desktop: users must drop to the CLI, and configured providers are invisible until they do.

## What Changes

- Make the runtime's provider report the single inventory for every provider surface (sidebar, composer account switcher, Accounts panel, new Providers surface), instead of the static sign-in catalog.
- Fix the account switcher so it offers every configured provider, not only the built-in sign-in catalog entries.
- Add a dedicated **Providers** surface with search, provider status, credential method, available model count, and per-provider actions: use/switch, set API key, sign in, test, and refresh.
- Add provider creation: a named OpenAI-compatible profile (name, base URL, default model, optional context window, API key) plus credential setup for built-in API-key providers.
- Add removal of user-created named provider profiles; built-in providers are never removable.
- Add the corresponding provider-administration capability to the Jcode SDK/harness so Desktop writes configuration and credentials through Jcode rather than editing runtime files itself. Configuration is offered only for local runtimes; remote runtimes stay read-only with guidance.
- Refresh provider surfaces after sign-in, credential changes, and profile creation/removal without restarting.

## Capabilities

### New Capabilities

- `desktop/provider-inventory`: every provider the attached runtime reports is visible and selectable in Desktop, with its status, credential method, and model availability, and sessions switch through the runtime's routing rules.
- `desktop/provider-configuration`: Desktop can create and remove named OpenAI-compatible provider profiles and store or replace credentials for providers through Jcode, with masked secret entry, local-runtime scoping, and immediate refresh.

### Modified Capabilities

_None. This repository has no existing specs, so both capabilities are new._

## Impact

- Desktop UI: `crates/jcode-desktop-ui` (account switcher, Accounts panel, sidebar accounts, provider rows and add/remove flows, model-route matching), `crates/jcode-desktop-accounts-ui` (login catalog policy and ordering).
- Desktop harness: `crates/jcode-desktop-harness` (provider inventory modeling, route correlation, provider administration commands, refresh).
- Jcode (owned sibling repository): `crates/jcode-sdk` (provider administration client; possibly generalized API-key provisioning), `crates/jcode-harness-api` and its server (provider add/remove requests if a protocol route is chosen), `src/cli` (shared provider add/remove logic, stdin key entry).
- Data: reads `~/.jcode/config.toml` named profiles and runtime provider reports; writes only through Jcode APIs/CLI; credentials remain in Jcode's private env files.
- Tests: GPUI-free harness tests for inventory merging and command construction, UI tests with fixtures, a screenshot/acceptance pass, and the existing `verify-runtime-source.py` preflight.
- No new third-party dependencies are expected. Git identity guard and cross-repository commit/push rules from `AGENTS.md` apply to both repositories.
