# Tasks

## 1. Jcode provider administration (jcode repository)

- [x] 1.1 Extract the provider-profile creation logic from `src/cli/commands/provider_setup.rs` into a shared library function the daemon can call; verify `cargo test -p jcode --lib provider_setup` passes and `jcode provider add` still writes an identical profile in a temp home
- [x] 1.2 Generalize the harness `SetApiKey`/`ClearApiKey` provider binding (replace the hardcoded match in `crates/jcode-harness-api-server/src/translate.rs` with provider-metadata lookup of `api_key_env`/`env_file`); verify tests cover the existing six providers plus a named OpenAI-compatible profile and keep current error codes
- [x] 1.3 Add `ProviderAdd`/`ProviderRemove` requests to `crates/jcode-harness-api` and handle them in the server (named profiles only, no built-in deletion); verify translation tests round-trip both requests and reject removing a built-in provider
- [x] 1.4 Add `jcode provider remove <name>` and `jcode login --api-key-stdin` so CLI parity exists without passing secrets on argv; verify CLI tests cover success, unknown profile, and invalid key input
- [x] 1.5 Add `JcodeClient::add_provider_profile` / `remove_provider_profile` to `crates/jcode-sdk` with capability-aware errors; verify `cargo test -p jcode-sdk client_behavior` passes including a fake-server round trip
- [x] 1.6 Document provider administration (config profile shape, key storage, removal) in the Jcode docs and verify the documented commands run as written
- [x] 1.7 Commit and push the Jcode changes on the feature branch; verify `git status` is clean and the branch tip is pushed

## 2. Desktop provider inventory and admin client (jcode-desktop-harness)

- [x] 2.1 Add a GPUI-free `providers` module that merges the runtime auth report with runtime model routes into `ProviderEntry` rows (id, display name, auth kind, status, model list/count, actions); verify unit tests cover OpenRouter, OpenCode Go, a named OpenAI-compatible profile, Cursor, and an OAuth provider
- [x] 2.2 Derive `supported_actions` from visibility, local/remote runtime, and runtime capabilities; verify unit tests for local vs remote and for an older runtime without provider admin requests
- [x] 2.3 Implement the admin client calls (set/replace key, add profile, remove profile) through the SDK, with request construction and validation mirrored from the server; verify unit tests for valid input, empty/whitespace keys, invalid base URLs, and duplicate names
- [x] 2.4 Wire inventory refresh into the existing account poller (`accounts::request_refresh`) and verify a test that a configuration action triggers a fresh snapshot
- [x] 2.5 Add `docs/providers.md` for Desktop provider behavior and limits, and verify it matches the implemented actions

## 3. Desktop provider surfaces (jcode-desktop-ui)

- [x] 3.1 Replace `LoginProvider`-based row models in the composer account switcher with the owned provider entries and list every configured provider, connected first; verify UI tests construct the switcher from a runtime fixture and include providers outside the sign-in catalog
- [x] 3.2 Select a provider using the runtime routing policy, preserving the current model when available and falling back to the preferred route; verify tests for both cases and for the no-available-model message
- [x] 3.3 Build the Providers surface (search, in-use/connected/not-configured sections, per-row Use / Set key / Sign in / Test actions, "Manage providers…" entry from the composer and sidebar accounts header); verify UI tests cover row actions and the entry points
- [x] 3.4 Add the add-provider dialog (name, base URL, model, context window, env-var name, masked key) with field-level validation and local-runtime gating; verify UI tests for the success path, each invalid field, and masked input
- [x] 3.5 Add key entry for any API-key provider, including named profiles, with server-rule validation and refresh on success; verify UI tests cover success, rejection, and no-secret-in-error behavior
- [x] 3.6 Add removal for named profiles behind a confirmation, with built-in providers excluded; verify UI tests cover confirm, cancel, and built-in rows without a remove action
- [x] 3.7 Make the surface read-only with configuration guidance for remote/SSH runtimes; verify a UI test asserts configuration actions are unavailable and guidance renders
- [ ] 3.8 Apply the project visual rules to new rows, pills, and dialogs (fully rounded pills, subtle row fill, no decorative dots), and verify with `python3 scripts/screenshot.py target/ui-review.png` using the provider fixture
- [ ] 3.9 Commit and push the Desktop changes; verify `git status` is clean, the pushed branch matches the local tip, and CI-required checks are unaffected

## 4. Integration verification

- [x] 4.1 Run `python3.12 scripts/verify-runtime-source.py --runtime ../jcode` and verify the preflight passes with the updated runtime requirements
- [ ] 4.2 Run `cargo build` and `justrust test --workspace` (or the smallest affected crates plus the UI once) and verify all pass
- [ ] 4.3 Extend the offline fixture (screenshot mode) with a provider inventory containing catalog, named-profile, and OAuth providers, and verify `python3 scripts/screenshot.py` succeeds
- [ ] 4.4 Rebuild and reload the running Desktop app and verify the switcher lists every configured provider and the Providers surface opens and reflects a credential change

## Workflow follow-up

- Review the change with the user, then start the apply workflow explicitly.
- Archive the change after implementation and verification are complete.
