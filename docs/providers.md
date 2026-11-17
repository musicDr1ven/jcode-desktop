# Providers in Jcode Desktop

Desktop shows every provider the attached runtime reports — not just the
built-in sign-in catalog. The sidebar accounts list, the composer account
switcher, and the Providers surface all render the same inventory.

## Where the data comes from

- **Existence, credential state, and account labels:** `jcode auth status --json`
  and `jcode usage --json`, polled in the background (`crates/jcode-desktop-harness/src/accounts.rs`).
- **Usable models and switch targets:** the runtime's advertised model routes
  (`RuntimeInfo.routes`). A provider with no available route is shown as having
  no usable model instead of being hidden.
- **Actions:** derived from that state plus the runtime context
  (`crates/jcode-desktop-harness/src/providers.rs`).

## What each provider can offer

| Action | Offered when |
| --- | --- |
| Use | The provider has at least one available model route. |
| Set API key | The session is local, the runtime advertises `provider_admin`, and the provider authenticates with an API key. |
| Sign in | The session is local and the SDK can run the provider's OAuth/device-code flow. |
| Test | The session is local and the provider is connected. |
| Remove | The session is local and the provider is a user-created named profile. Built-in providers are never removable. |

Remote and managed-cloud runtimes are read-only in the provider surface; the
panel explains that providers must be configured on the runtime host.

## Adding a provider

The add dialog collects a profile name, base URL, default model, optional
context window and env-var name, and a masked API key. It accepts the same
shapes as `jcode provider add`:

```sh
printf '%s\n' "$KEY" | jcode provider add my-gateway \
  --base-url https://llm.example.com/v1 --model my-model --api-key-stdin
```

Base URLs may be `https://`, or `http://` for localhost, private-LAN, and
bare-IP hosts. A local endpoint may be configured without a key. Names are
ASCII letters, numbers, `-`, and `_`, at most 64 characters, and may not reuse a
built-in provider id or alias.

## Security

- API keys are masked in the UI and never written to transcripts, logs, or
  telemetry.
- Keys travel to the runtime through the harness API; they are not placed on a
  process command line.
- Stored keys live in owner-only env files under the runtime's app config
  directory (`0600`), never in `config.toml`.
- Failures report the reason without echoing the submitted secret.

## Compatibility

Older runtimes that do not advertise `provider_admin` still contribute their
providers and models to the inventory, but add/remove/key actions are hidden
and the surface points at the CLI instead. See the runtime documentation in
`docs/PROVIDER_ADMIN.md` (Jcode repository).
