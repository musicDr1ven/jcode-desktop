# Spec Delta

## Purpose

Guarantees that every provider the attached Jcode runtime reports is visible in Desktop and that a session can switch to any provider with an available model route.

## ADDED Requirements

### Requirement: Runtime provider report is the inventory

The desktop SHALL treat the attached runtime's provider report (`jcode auth status`) as the source of truth for which providers exist, in addition to the SDK's built-in sign-in catalog, and SHALL surface every entry in Desktop's provider lists.

#### Scenario: Configured provider outside the sign-in catalog is listed

- **WHEN** the runtime reports a provider such as `opencode-go` or a named OpenAI-compatible profile that is not in the SDK sign-in catalog
- **THEN** the sidebar accounts list, the composer account switcher, the Accounts panel, and the Providers surface each include that provider with its runtime display name

#### Scenario: Unconfigured provider remains visible

- **WHEN** the runtime reports a provider whose status is `not_configured`
- **THEN** provider surfaces list it as not connected rather than omitting it

### Requirement: Account switcher offers every configured provider

The composer's account switcher SHALL list every provider the runtime reports as available, plus every login for multi-account providers, instead of only providers present in the built-in sign-in catalog.

#### Scenario: Switcher matches the sidebar

- **WHEN** the runtime reports OpenRouter, OpenCode Go, a named OpenAI-compatible profile, and Cursor as available
- **THEN** the account switcher offers all four providers, each with its account label when one exists

#### Scenario: Selection switches the session

- **WHEN** the user selects a provider from the switcher
- **THEN** the session switches to that provider's preferred available model route and the switcher closes

### Requirement: Provider rows expose status and credential method

Each provider surface SHALL show, for every provider, its display name, credential method (for example OAuth, API key, or local endpoint), and connection state derived from the runtime report.

#### Scenario: Connected and unconfigured rows are distinguishable

- **WHEN** the Providers surface renders a mixed inventory
- **THEN** connected providers and not-configured providers are distinguishable by their state text, without relying on color alone

### Requirement: Model availability comes from runtime routes

The desktop SHALL derive each provider's available model count and switch target from the runtime's advertised model routes. A provider with no available route SHALL be shown as having no usable model, and selecting it SHALL produce an actionable message instead of a silent no-op.

#### Scenario: Provider has models

- **WHEN** the runtime advertises available routes owned by the provider
- **THEN** the provider row reports the count of available models and selecting the provider uses one of them

#### Scenario: Provider has no available route

- **WHEN** a provider has credentials but no available model route (for example every route is unavailable)
- **THEN** selecting it shows a message explaining that no model is currently usable and leaves the session unchanged

### Requirement: Provider surfaces refresh after credential changes

Provider surfaces SHALL update after a sign-in, sign-out, API-key change, or provider profile change without requiring an application restart.

#### Scenario: Signing in updates the switcher

- **WHEN** the user completes a sign-in or saves a credential from Desktop
- **THEN** the provider's state and model list update in every open provider surface

### Requirement: Provider switching preserves runtime routing rules

When switching providers, the desktop SHALL select the route using the runtime's routing rules and SHALL preserve the currently selected model when that model is available on the target provider.

#### Scenario: Model preserved across providers

- **WHEN** the session's current model is available on the selected provider
- **THEN** switching keeps that model and changes only the provider route

#### Scenario: Preferred model used when the current model is absent

- **WHEN** the session's current model is not available on the selected provider
- **THEN** switching uses the target provider's preferred available model
