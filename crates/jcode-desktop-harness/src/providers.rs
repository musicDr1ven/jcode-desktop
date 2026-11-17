//! Provider inventory and administration for Desktop.
//!
//! The runtime's auth report (`crate::accounts`) is the source of truth for
//! which providers exist; its model routes decide what is usable. This module
//! merges the two into one entry list every provider surface renders, and
//! exposes the admin operations the provider UI performs. No GPUI, so the
//! policy is testable without a window.

use crate::accounts;
use std::sync::atomic::{AtomicBool, Ordering};
use jcode_sdk::{ConnectOptions, Error, ErrorKind, JcodeClient, ModelRouteInfo, ProviderProfileSpec};

/// Whether the connected runtime advertised `provider_admin`. Set from the
/// handshake so every surface agrees without threading a flag through panels.
static PROVIDER_ADMIN_SUPPORTED: AtomicBool = AtomicBool::new(false);

/// Record the runtime's provider-admin capability from the handshake.
pub fn set_provider_admin_supported(supported: bool) {
    PROVIDER_ADMIN_SUPPORTED.store(supported, Ordering::Relaxed);
}

/// Whether the current runtime accepts provider add/remove requests.
pub fn provider_admin_supported() -> bool {
    PROVIDER_ADMIN_SUPPORTED.load(Ordering::Relaxed)
}

/// One action a provider row may offer. Derived from runtime state, never
/// guessed from a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAction {
    /// Switch the session to this provider.
    Use,
    /// Store or replace an API key.
    SetApiKey,
    /// Run the provider's OAuth/device-code sign-in.
    SignIn,
    /// Send one live request to confirm the account works.
    Test,
    /// Remove a user-created named profile.
    Remove,
}

/// One model route owned by a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModel {
    pub model: String,
    pub provider: String,
    pub api_method: String,
    pub available: bool,
}

/// One provider, as every Desktop surface should present it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderEntry {
    /// Credential/provider key (`openrouter`, `neuralwatt`, ...).
    pub id: String,
    pub display_name: String,
    pub auth_kind: String,
    pub method: String,
    pub status: String,
    pub credential_source: Option<String>,
    pub models: Vec<ProviderModel>,
    pub actions: Vec<ProviderAction>,
    /// True only for user-created named profiles, the only removable providers.
    pub removable: bool,
}

impl ProviderEntry {
    pub fn available(&self) -> bool {
        self.status == "available"
    }

    pub fn available_model_count(&self) -> usize {
        self.models.iter().filter(|model| model.available).count()
    }

    pub fn offers(&self, action: ProviderAction) -> bool {
        self.actions.contains(&action)
    }

    pub fn status_label(&self) -> &'static str {
        match self.status.as_str() {
            "available" if matches!(self.auth_kind.as_str(), "OAuth" | "device code") => {
                "Signed in"
            }
            "available" => "Connected",
            "expired" => "Expired · sign in again",
            "not_configured" if matches!(self.auth_kind.as_str(), "OAuth" | "device code") => {
                "Not signed in"
            }
            "not_configured" => "Not configured",
            _ => "Status unknown",
        }
    }
}

/// Facts that decide which actions are safe, kept out of the merge so tests
/// can state them directly.
#[derive(Debug, Clone, Default)]
pub struct InventoryContext {
    /// The attached runtime runs on this machine.
    pub local: bool,
    /// The runtime advertises `provider_admin`.
    pub provider_admin: bool,
    /// Provider ids the sign-in catalog supports.
    pub sign_in_ids: Vec<String>,
    /// User-created named profile ids.
    pub named_profiles: Vec<String>,
}

impl InventoryContext {
    /// Build the context from the live runtime: local sessions can configure
    /// providers; remote sessions are read-only. Named profiles and sign-in
    /// support come from the same config/catalog the runtime uses.
    pub fn current(local: bool, provider_admin: bool) -> Self {
        Self {
            local,
            provider_admin,
            sign_in_ids: sign_in_provider_ids(),
            named_profiles: named_provider_ids(),
        }
    }
}

/// Sign-in providers the SDK can start a native flow for.
pub fn sign_in_provider_ids() -> Vec<String> {
    jcode_sdk::AuthClient::new(jcode_sdk::AuthOptions::default())
        .providers()
        .into_iter()
        .map(|provider| provider.id.to_string())
        .collect()
}

/// User-created `[providers.<name>]` profile names.
pub fn named_provider_ids() -> Vec<String> {
    jcode_base::config::config()
        .providers
        .keys()
        .cloned()
        .collect()
}

/// Merge the runtime's account report with its model routes.
///
/// Each route is assigned to the best-matching provider: an exact
/// `openai-compatible:<profile>` or `api_method == id` match wins, then a
/// display-name match, then a `<id>-...` method prefix. Ties break toward the
/// longest id so `openai-api-key` belongs to `openai-api`, not `openai`.
pub fn inventory(
    accounts: &[accounts::Account],
    routes: &[ModelRouteInfo],
    context: &InventoryContext,
) -> Vec<ProviderEntry> {
    let mut models: Vec<Vec<ProviderModel>> = vec![Vec::new(); accounts.len()];
    for route in routes {
        if let Some(index) = best_account_for_route(route, accounts) {
            models[index].push(ProviderModel {
                model: route.model.clone(),
                provider: route.provider.clone(),
                api_method: route.api_method.clone(),
                available: route.available,
            });
        }
    }

    accounts
        .iter()
        .zip(models)
        .map(|(account, models)| {
            let removable = context
                .named_profiles
                .iter()
                .any(|profile| profile == &account.id);
            let mut actions = Vec::new();
            if account.available() && models.iter().any(|model| model.available) {
                actions.push(ProviderAction::Use);
            }
            if context.local && context.provider_admin {
                if is_api_key_provider(account) {
                    actions.push(ProviderAction::SetApiKey);
                }
                if removable {
                    actions.push(ProviderAction::Remove);
                }
            }
            if context.local
                && context
                    .sign_in_ids
                    .iter()
                    .any(|id| id == &account.id)
                && is_sign_in_provider(account)
            {
                actions.push(ProviderAction::SignIn);
            }
            if context.local && account.available() {
                actions.push(ProviderAction::Test);
            }

            ProviderEntry {
                id: account.id.clone(),
                display_name: account.display_name.clone(),
                auth_kind: account.auth_kind.clone(),
                method: account.method.clone(),
                status: account.status.clone(),
                credential_source: None,
                models,
                actions,
                removable,
            }
        })
        .collect()
}

fn is_api_key_provider(account: &accounts::Account) -> bool {
    let kind = account.auth_kind.to_ascii_lowercase();
    kind.contains("api key") || account.method.to_ascii_lowercase().contains("api key")
}

fn is_sign_in_provider(account: &accounts::Account) -> bool {
    matches!(
        account.auth_kind.as_str(),
        "OAuth" | "device code" | "CLI" | "API key / CLI"
    )
}

fn best_account_for_route(route: &ModelRouteInfo, accounts: &[accounts::Account]) -> Option<usize> {
    let method = route.api_method.trim().to_ascii_lowercase();
    let provider = route.provider.trim().to_ascii_lowercase();
    let mut best: Option<(u8, usize, usize)> = None;
    for (index, account) in accounts.iter().enumerate() {
        let id = account.id.to_ascii_lowercase();
        let display = account.display_name.to_ascii_lowercase();
        // A method prefix beats a display-name match: `openai-api-key` belongs
        // to `openai-api`, even though its provider label is just "OpenAI".
        let score = if method == id || method == format!("openai-compatible:{id}") {
            3
        } else if method
            .strip_prefix(&id)
            .is_some_and(|rest| rest.starts_with('-') || rest.starts_with(':'))
        {
            2
        } else if provider == display || provider == id {
            1
        } else {
            continue;
        };
        if best.is_none_or(|(best_score, best_len, _)| (score, id.len()) > (best_score, best_len)) {
            best = Some((score, id.len(), index));
        }
    }
    best.map(|(_, _, index)| index)
}

/// Ask every provider surface to re-read the runtime's provider state.
pub fn notify_provider_config_changed() {
    accounts::request_refresh();
}

fn client() -> jcode_sdk::Result<JcodeClient> {
    JcodeClient::connect(ConnectOptions {
        client_name: "jcode-desktop-providers".into(),
        ensure_runtime: false,
        ..Default::default()
    })
}

fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::InvalidOption, message)
}

const RESERVED_PROFILE_NAMES: &[&str] = &[
    "auto",
    "claude-subprocess",
    "compat",
    "custom",
    "azure-openai",
    "aoai",
];

/// Validate a named profile name the way the runtime does.
pub fn validate_profile_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("Enter a provider name.".into());
    }
    if name.len() > 64 {
        return Err("Provider names are at most 64 characters.".into());
    }
    let first = name.chars().next().unwrap_or_default();
    if !first.is_ascii_alphanumeric() {
        return Err("Provider names start with a letter or number.".into());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err("Use only letters, numbers, '-' and '_' in a provider name.".into());
    }
    if jcode_base::provider_catalog::resolve_login_provider(name).is_some()
        || RESERVED_PROFILE_NAMES
            .iter()
            .any(|reserved| name.eq_ignore_ascii_case(reserved))
    {
        return Err(format!(
            "'{name}' is a built-in provider name. Choose another name, such as '{name}-api'."
        ));
    }
    Ok(name.to_string())
}

/// Validate and normalize an OpenAI-compatible base URL.
pub fn validate_base_url(raw: &str) -> Result<String, String> {
    jcode_base::provider_catalog::normalize_api_base(raw).ok_or_else(|| {
        "Enter an http(s) base URL. Localhost, private-LAN, and bare-IP hosts may use http."
            .to_string()
    })
}

/// Validate a pasted API key before it is sent to the runtime.
pub fn validate_api_key(raw: &str) -> Result<(), String> {
    if raw.trim().is_empty() {
        return Err("Paste an API key first.".into());
    }
    if raw.trim() != raw {
        return Err("The API key has surrounding whitespace; paste it without spaces.".into());
    }
    if raw.contains(['\n', '\r', '\0']) {
        return Err("The API key must be a single line without control characters.".into());
    }
    Ok(())
}

/// Validate every field of a profile and return the normalized spec.
pub fn validate_profile_spec(spec: &ProviderProfileSpec) -> Result<ProviderProfileSpec, String> {
    let mut normalized = spec.clone();
    normalized.name = validate_profile_name(&spec.name)?;
    normalized.base_url = validate_base_url(&spec.base_url)?;
    if spec.model.trim().is_empty() {
        return Err("Enter the provider's default model id.".into());
    }
    normalized.model = spec.model.trim().to_string();
    if let Some(key) = spec.api_key.as_deref() {
        if spec.no_api_key {
            return Err("Choose either an API key or no authentication, not both.".into());
        }
        validate_api_key(key)?;
    } else if !spec.no_api_key && !normalized.base_url.starts_with("http://") {
        return Err("Add an API key, or choose no authentication for a local endpoint.".into());
    }
    Ok(normalized)
}

/// Reject a new profile whose name an existing provider already uses, unless
/// the caller asked to overwrite it.
fn ensure_profile_new(
    name: &str,
    overwrite: bool,
    existing: impl Iterator<Item = String>,
) -> Result<(), String> {
    if !overwrite && existing.into_iter().any(|id| id == name) {
        return Err(format!("A provider named '{name}' already exists."));
    }
    Ok(())
}

/// Create or replace a named provider profile and refresh provider surfaces.
pub fn add_provider_profile(spec: ProviderProfileSpec) -> jcode_sdk::Result<()> {
    let spec = validate_profile_spec(&spec).map_err(invalid)?;
    ensure_profile_new(&spec.name, spec.overwrite, named_provider_ids().into_iter())
        .map_err(invalid)?;
    let client = client()?;
    client.add_provider_profile(spec)?;
    notify_provider_config_changed();
    Ok(())
}

/// Remove a user-created named profile and its stored key.
pub fn remove_provider_profile(name: &str) -> jcode_sdk::Result<()> {
    let name = validate_profile_name(name).map_err(invalid)?;
    let client = client()?;
    client.remove_provider_profile(&name)?;
    notify_provider_config_changed();
    Ok(())
}

/// Store or replace an API key for any provider the runtime can bind.
pub fn set_provider_api_key(provider: &str, key: &str) -> jcode_sdk::Result<()> {
    validate_api_key(key).map_err(invalid)?;
    let client = client()?;
    client.set_api_key(provider, key)?;
    notify_provider_config_changed();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, display: &str, status: &str, auth_kind: &str) -> accounts::Account {
        accounts::Account {
            id: id.into(),
            display_name: display.into(),
            status: status.into(),
            auth_kind: auth_kind.into(),
            method: format!("{auth_kind} method"),
            limits: Vec::new(),
            usage_reports: Vec::new(),
        }
    }

    fn route(model: &str, provider: &str, api_method: &str, available: bool) -> ModelRouteInfo {
        ModelRouteInfo {
            model: model.into(),
            provider: provider.into(),
            api_method: api_method.into(),
            available,
            detail: String::new(),
            usage: None,
        }
    }

    fn local_context() -> InventoryContext {
        InventoryContext {
            local: true,
            provider_admin: true,
            sign_in_ids: vec!["claude".into(), "openai".into(), "cursor".into()],
            named_profiles: vec!["neuralwatt".into()],
        }
    }

    fn fixture() -> (Vec<accounts::Account>, Vec<ModelRouteInfo>) {
        let accounts = vec![
            account("openrouter", "OpenRouter", "available", "API key"),
            account("opencode-go", "OpenCode Go", "available", "API key"),
            account("neuralwatt", "Neuralwatt", "available", "API key"),
            account("cursor", "Cursor", "available", "API key"),
            account("claude", "Anthropic/Claude", "available", "OAuth"),
            account("openai", "OpenAI", "not_configured", "OAuth"),
            account("openai-api", "OpenAI API", "not_configured", "API key"),
        ];
        let routes = vec![
            route("openrouter/auto", "OpenRouter", "openrouter", true),
            route("go-model", "OpenCode Go", "openai-compatible:opencode-go", true),
            route("nw-model", "Neuralwatt", "openai-compatible:neuralwatt", false),
            route("cursor-model", "Cursor", "cursor", true),
            route("claude-5", "Anthropic", "claude-oauth", true),
            route("gpt-6", "OpenAI", "openai-api-key", true),
        ];
        (accounts, routes)
    }

    #[test]
    fn inventory_lists_every_runtime_provider_with_its_models() {
        let (accounts, routes) = fixture();
        let entries = inventory(&accounts, &routes, &local_context());
        let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "openrouter",
                "opencode-go",
                "neuralwatt",
                "cursor",
                "claude",
                "openai",
                "openai-api"
            ]
        );

        let openrouter = &entries[0];
        assert_eq!(openrouter.display_name, "OpenRouter");
        assert_eq!(openrouter.available_model_count(), 1);
        assert!(openrouter.offers(ProviderAction::Use));

        // A named profile owns its openai-compatible route and is removable.
        let neuralwatt = &entries[2];
        assert_eq!(neuralwatt.models.len(), 1);
        assert_eq!(neuralwatt.available_model_count(), 0);
        assert!(neuralwatt.removable);
        assert!(neuralwatt.offers(ProviderAction::Remove));
        assert!(neuralwatt.offers(ProviderAction::SetApiKey));
        assert!(!neuralwatt.offers(ProviderAction::Use));
    }

    #[test]
    fn api_key_route_belongs_to_the_api_key_provider_not_its_oauth_sibling() {
        let (accounts, routes) = fixture();
        let entries = inventory(&accounts, &routes, &local_context());
        let oauth = entries.iter().find(|entry| entry.id == "openai").unwrap();
        let api = entries.iter().find(|entry| entry.id == "openai-api").unwrap();
        assert_eq!(oauth.models.len(), 0);
        assert_eq!(api.models.len(), 1);
        assert_eq!(api.models[0].model, "gpt-6");
    }

    #[test]
    fn oauth_provider_offers_sign_in_and_test() {
        let (accounts, routes) = fixture();
        let entries = inventory(&accounts, &routes, &local_context());
        let claude = entries.iter().find(|entry| entry.id == "claude").unwrap();
        assert!(claude.offers(ProviderAction::SignIn));
        assert!(claude.offers(ProviderAction::Test));
        assert!(!claude.offers(ProviderAction::SetApiKey));
        assert!(!claude.removable);
    }

    #[test]
    fn remote_runtime_offers_no_configuration_actions() {
        let (accounts, routes) = fixture();
        let mut context = local_context();
        context.local = false;
        let entries = inventory(&accounts, &routes, &context);
        for entry in &entries {
            for action in [
                ProviderAction::SetApiKey,
                ProviderAction::SignIn,
                ProviderAction::Test,
                ProviderAction::Remove,
            ] {
                assert!(
                    !entry.offers(action),
                    "{} must not offer {action:?} on a remote runtime",
                    entry.id
                );
            }
        }
        assert!(entries[0].offers(ProviderAction::Use));
    }

    #[test]
    fn runtime_without_provider_admin_hides_admin_actions() {
        let (accounts, routes) = fixture();
        let mut context = local_context();
        context.provider_admin = false;
        let entries = inventory(&accounts, &routes, &context);
        for entry in &entries {
            assert!(!entry.offers(ProviderAction::SetApiKey));
            assert!(!entry.offers(ProviderAction::Remove));
        }
    }

    #[test]
    fn profile_validation_mirrors_runtime_rules() {
        assert_eq!(validate_profile_name(" my-gw ").unwrap(), "my-gw");
        assert!(validate_profile_name("").is_err());
        assert!(validate_profile_name("-bad").is_err());
        assert!(validate_profile_name("bad name").is_err());
        assert!(validate_profile_name("openai").is_err());
        assert!(validate_profile_name("custom").is_err());
        assert!(validate_base_url("not a url").is_err());
        assert_eq!(
            validate_base_url("https://llm.example.com/v1/").unwrap(),
            "https://llm.example.com/v1"
        );
        assert!(validate_api_key("").is_err());
        assert!(validate_api_key(" key").is_err());
        assert!(validate_api_key("line\nbreak").is_err());
        assert!(validate_api_key("sk-ok").is_ok());
    }

    #[test]
    fn profile_spec_validation_rejects_bad_combinations() {
        let base = ProviderProfileSpec {
            name: "my-gw".into(),
            base_url: "https://llm.example.com/v1".into(),
            model: "model-a".into(),
            api_key: Some("sk-test".into()),
            ..Default::default()
        };
        assert!(validate_profile_spec(&base).is_ok());

        let mut no_model = base.clone();
        no_model.model = "  ".into();
        assert!(validate_profile_spec(&no_model).is_err());

        let mut both = base.clone();
        both.no_api_key = true;
        assert!(validate_profile_spec(&both).is_err());

        let mut remote_no_key = base.clone();
        remote_no_key.api_key = None;
        assert!(validate_profile_spec(&remote_no_key).is_err());

        let mut local_no_key = remote_no_key.clone();
        local_no_key.base_url = "http://localhost:8000/v1".into();
        assert!(validate_profile_spec(&local_no_key).is_ok());
    }

    #[test]
    fn duplicate_profile_names_are_rejected_unless_overwrite_is_set() {
        let existing = vec!["my-gw".to_string(), "other".to_string()];
        let error = ensure_profile_new("my-gw", false, existing.clone().into_iter())
            .expect_err("duplicate must be refused");
        assert!(error.contains("already exists"), "{error}");
        assert!(ensure_profile_new("my-gw", true, existing.clone().into_iter()).is_ok());
        assert!(ensure_profile_new("new-gw", false, existing.into_iter()).is_ok());
    }

    #[test]
    fn configuration_changes_request_a_fresh_account_snapshot() {
        let before = accounts::refresh_generation();
        notify_provider_config_changed();
        assert!(accounts::refresh_generation() > before);
    }
}
