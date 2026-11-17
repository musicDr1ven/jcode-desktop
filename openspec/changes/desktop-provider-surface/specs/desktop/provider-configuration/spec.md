# Spec Delta

## Purpose

Lets Desktop users create, credential, and remove providers through Jcode, with safe secret handling and without hand-editing runtime configuration files.

## ADDED Requirements

### Requirement: Add a named OpenAI-compatible provider profile

Desktop SHALL let a user create a named OpenAI-compatible provider profile by supplying a profile name, a base URL, a default model, and optionally a context window, an environment-variable name, and an API key.

#### Scenario: Profile is created and usable

- **WHEN** the user submits a unique profile name, a valid HTTP(S) base URL, a model id, and an API key
- **THEN** Jcode stores the profile and key, and the provider appears in every provider surface without restart

#### Scenario: Profile is created without a key

- **WHEN** the user chooses to configure a profile without a credential
- **THEN** the profile is stored and shown as not connected until a key is added

#### Scenario: Invalid input is rejected

- **WHEN** the name is empty or already used, the base URL is not HTTP(S), or the model is empty
- **THEN** Desktop shows a field-specific error, makes no configuration change, and keeps the entered values

### Requirement: Store or replace an API key for any API-key provider

Desktop SHALL let a user set or replace the API key for any provider whose runtime report identifies API-key authentication, including named OpenAI-compatible profiles, and SHALL route the write through Jcode.

#### Scenario: Key saved for a built-in provider

- **WHEN** the user submits a non-empty key for a provider that authenticates with an API key
- **THEN** Jcode stores it, the provider becomes connected, and its model routes refresh

#### Scenario: Key rejected for a provider that cannot store one

- **WHEN** the runtime does not support storing a key for the selected provider
- **THEN** Desktop explains why and offers the sign-in or CLI path instead of reporting success

#### Scenario: Invalid key input is refused

- **WHEN** the submitted key is empty, contains surrounding whitespace, or includes line breaks or NUL characters
- **THEN** Desktop rejects it before writing and explains the expected input

### Requirement: Remove user-created provider profiles

Desktop SHALL allow removing the named provider profiles a user created, after explicit confirmation, and SHALL NOT offer removal for built-in providers.

#### Scenario: Named profile removed

- **WHEN** the user confirms removal of a named profile
- **THEN** Jcode deletes the profile and its stored credential, and the provider disappears from every provider surface

#### Scenario: Built-in provider is not removable

- **WHEN** the user inspects a built-in provider
- **THEN** no remove action is offered and no configuration is deleted

### Requirement: Configuration is limited to local runtimes

Provider creation, key storage, and removal SHALL be offered only when the attached runtime runs on the local machine. For remote or SSH runtimes, the Providers surface SHALL be read-only and SHALL explain where to configure the provider.

#### Scenario: Remote runtime is read-only

- **WHEN** the session is attached to a remote runtime
- **THEN** add, edit, and remove actions are unavailable and the surface shows guidance to configure the provider on the runtime host

### Requirement: Existing sign-in paths remain available

OAuth and device-code sign-in SHALL remain available for providers that support it, and providers that require a CLI-only flow SHALL state the command needed rather than presenting a broken action.

#### Scenario: OAuth provider signs in from the surface

- **WHEN** the user selects a provider that supports OAuth or device code
- **THEN** the existing sign-in flow runs and its result refreshes the provider inventory

#### Scenario: CLI-only provider explains its requirement

- **WHEN** a provider's authentication cannot be completed from Desktop
- **THEN** the surface shows the required CLI action instead of an unusable button

### Requirement: Secrets never leave the runtime's private store

API key input SHALL be masked, SHALL NOT be written to session transcripts, logs, or telemetry, and SHALL be passed to Jcode without appearing on process command lines where the platform can avoid it.

#### Scenario: Key entry is masked

- **WHEN** the user types or pastes a key
- **THEN** the field renders masked characters and the key is absent from logs and transcripts

#### Scenario: Failure does not echo the key

- **WHEN** saving a key fails
- **THEN** the error message reports the failure without including the submitted value

### Requirement: Configuration failures are actionable and atomic

When Jcode rejects or fails a configuration change, Desktop SHALL show an actionable message and SHALL leave the previous configuration and provider state intact.

#### Scenario: Storage failure preserves prior state

- **WHEN** Jcode fails to write a profile or credential
- **THEN** Desktop reports the failure, the provider's previous state remains, and no partial profile is listed
