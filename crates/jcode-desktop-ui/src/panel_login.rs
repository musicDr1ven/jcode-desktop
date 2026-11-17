//! Native login UI. Secrets and OAuth state never enter the conversation or snapshots.
use super::*;
use crate::login_input::LoginInput;
use jcode_sdk::{
    AuthClient, AuthFlow, AuthInputKind, AuthOptions, AuthPrompt, LoginMethod, LoginProvider,
};

use connection::{ConnectionStatus, ConnectionStatuses};
use jcode_desktop_accounts_ui::connection;
#[path = "panel_login_accounts.rs"]
mod accounts;
use jcode_desktop_accounts_ui::catalog;
#[path = "panel_provider_picker.rs"]
mod provider_picker;
#[cfg(test)]
#[path = "panel_provider_picker_tests.rs"]
mod provider_picker_tests;
#[path = "panel_login_steps.rs"]
mod steps;
use steps::manual_input_visible;

pub(super) struct LoginState {
    /// Composer account selection never starts a sign-in flow.
    selection_only: bool,
    client: AuthClient,
    providers: Vec<LoginProvider>,
    provider: Option<LoginProvider>,
    flow: Option<AuthFlow>,
    prompt: Option<AuthPrompt>,
    input: Entity<LoginInput>,
    busy: bool,
    browser_opened: bool,
    scroll: ScrollHandle,
    error: Option<String>,
    complete: bool,
    focus_pending: bool,
    task: Option<Task<()>>,
    callback_task: Option<Task<()>>,
    callback_waiting: bool,
    /// The browser returned an authorization to the local callback. This is
    /// the only signal that confirms the approve step without guessing.
    authorized: bool,
    /// The user chose to paste a callback URL while automatic return waits.
    manual_entry: bool,
    statuses: Option<ConnectionStatuses>,
    status_loading: bool,
    status_task: Option<Task<()>>,
    search: Entity<PromptInput>,
    usage: Option<catalog::MethodUsage>,
    accounts: accounts::AccountsData,
    /// Runtime provider inventory (accounts merged with model routes).
    entries: Vec<crate::providers::ProviderEntry>,
    /// Open add-provider form, key prompt, or removal confirmation.
    add_provider: Option<AddProviderState>,
    key_provider: Option<ProviderKeyState>,
    remove_provider: Option<String>,
    accounts_task: Option<Task<()>>,
    /// Provider ids with a live smoke test in flight.
    live_testing: std::collections::HashSet<String>,
    /// A "Test all" run is in flight.
    live_test_all: bool,
    /// Failure reasons from the most recent live test, by provider id.
    live_errors: HashMap<String, String>,
    live_tasks: Vec<Task<()>>,
}

/// Inline add-provider form. Values live in the input entities so validation
/// reads exactly what the user sees.
pub(super) struct AddProviderState {
    pub name: Entity<LoginInput>,
    pub base_url: Entity<LoginInput>,
    pub model: Entity<LoginInput>,
    pub context_window: Entity<LoginInput>,
    pub api_key_env: Entity<LoginInput>,
    pub api_key: Entity<LoginInput>,
    pub no_api_key: bool,
    pub busy: bool,
    pub error: Option<String>,
}

/// Inline "set API key" prompt for any provider, including named profiles.
pub(super) struct ProviderKeyState {
    pub provider: String,
    pub display_name: String,
    pub busy: bool,
    pub error: Option<String>,
}

impl Drop for LoginState {
    fn drop(&mut self) {
        // Cancellation can involve process cleanup. Never block the UI thread.
        if let Some(flow) = self.flow.take() {
            std::thread::spawn(move || {
                let _ = flow.cancel();
            });
        }
    }
}

enum LoginUpdate {
    Prompt(AuthPrompt),
    Complete { validation_warning: bool },
}

impl Panel {
    pub(super) fn login_input_focus_handle(&self, cx: &App) -> Option<FocusHandle> {
        let state = self.login.as_ref()?;
        if state.provider.is_none() {
            return Some(state.search.focus_handle(cx));
        }
        (state
            .provider
            .as_ref()
            .is_some_and(|provider| provider.method == LoginMethod::ApiKey)
            || manual_input_visible(state))
        .then(|| state.input.read(cx).focus_handle.clone())
    }

    fn login_is_remote(&self) -> bool {
        let source = self
            .session_id
            .strip_prefix("accounts://")
            .unwrap_or(&self.session_id);
        crate::harness::remote_host(source).is_some()
    }

    pub(super) fn open_login_picker(&mut self, cx: &mut Context<Self>) {
        self.recovery_picker_open = false;
        if self.login.is_some() {
            return;
        }
        let client = AuthClient::new(AuthOptions {
            binary: crate::platform::companion_executable("jcode"),
            ..Default::default()
        });
        let providers = if self.preview_state.is_some() {
            vec![
                LoginProvider {
                    id: "openai",
                    display_name: "OpenAI",
                    detail: "Offline OAuth preview",
                    method: LoginMethod::OAuth,
                },
                LoginProvider {
                    id: "openai-api",
                    display_name: "OpenAI API key",
                    detail: "Offline credential preview",
                    method: LoginMethod::ApiKey,
                },
            ]
        } else {
            client.providers()
        };
        let change = cx.weak_entity();
        let cancel = change.clone();
        let search = cx.new(|cx| {
            let mut search = PromptInput::new(cx, "Search accounts…", |_, _, _, _| {})
                .without_command_completion()
                .with_on_overlay_cancel(move |app| {
                    let restored = cancel.update(app, |panel, cx| {
                        panel.close_login_picker(cx);
                        if panel.is_accounts_panel() {
                            cx.emit(AccountsPanelClosed);
                            None
                        } else {
                            Some(panel.input_focus_handle(cx))
                        }
                    });
                    if let Ok(Some(focus)) = &restored
                        && let Some(window) = app.active_window()
                    {
                        let _ = window.update(app, |_, window, cx| focus.focus(window, cx));
                    }
                    restored.is_ok()
                })
                .with_on_change(move |_, app| {
                    let _ = change.update(app, |panel, cx| {
                        if let Some(state) = &panel.login {
                            state.scroll.set_offset(point(px(0.), px(0.)));
                        }
                        cx.notify();
                    });
                });
            search.set_submission_enabled(false, cx);
            search
        });
        self.login = Some(LoginState {
            selection_only: false,
            client,
            providers,
            provider: None,
            flow: None,
            prompt: None,
            input: cx.new(|cx| LoginInput::new(cx, "Paste the code or callback URL")),
            busy: false,
            browser_opened: false,
            scroll: ScrollHandle::new(),
            error: None,
            complete: false,
            focus_pending: true,
            task: None,
            callback_task: None,
            callback_waiting: false,
            authorized: false,
            manual_entry: false,
            statuses: None,
            status_loading: false,
            status_task: None,
            search,
            usage: None,
            accounts: accounts::AccountsData::default(),
            entries: Vec::new(),
            add_provider: None,
            key_provider: None,
            remove_provider: None,
            accounts_task: None,
            live_testing: Default::default(),
            live_test_all: false,
            live_errors: HashMap::new(),
            live_tasks: Vec::new(),
        });
        // Local credentials cannot authenticate an SSH-hosted session.
        if self.login_is_remote() {
            self.login.as_mut().unwrap().error = Some(
                "This session runs on another machine. Desktop login currently connects accounts on this computer only. Your remote credentials have not been changed.".into());
        } else {
            self.refresh_login_status(cx);
        }
        // Offline screenshot fixture: an OAuth sign-in waiting for the
        // browser, so the checklist can be reviewed without a network.
        if crate::harness::screenshot_mode()
            && std::env::var("JCODE_DESKTOP_SCREENSHOT_SIGN_IN_STEPS").as_deref() == Ok("1")
            && let Some(state) = self.login.as_mut()
        {
            state.provider = state
                .providers
                .iter()
                .find(|provider| provider.method == LoginMethod::OAuth)
                .copied();
            state.prompt = Some(AuthPrompt {
                auth_url: "https://example.invalid/authorize".into(),
                input_kind: AuthInputKind::AuthCodeOrCallbackUrl,
                user_code: None,
                expires_at_ms: i64::MAX,
            });
            state.browser_opened = true;
            state.callback_waiting = true;
        }
        cx.notify();
    }

    fn refresh_login_status(&mut self, cx: &mut Context<Self>) {
        let offline =
            self.preview_state.is_some() || cfg!(test) || crate::harness::screenshot_mode();
        let Some(state) = self.login.as_mut() else {
            return;
        };
        if state.status_loading {
            return;
        }
        state.status_loading = true;
        state.statuses = None;
        let accounts_task = cx.background_executor().spawn(async move {
            if offline {
                accounts::offline_data()
            } else {
                // A default chosen elsewhere (model picker, /account, CLI
                // login) before this sync existed still reorders the list.
                let default_route = jcode_base::config::Config::load().provider.default_provider;
                if let Some(route) = default_route.as_deref()
                    && let Err(error) =
                        jcode_base::auth::account_pool::sync_order_with_default_route(route)
                {
                    eprintln!("[accounts] could not sync order with default: {error}");
                }
                accounts::AccountsData {
                    accounts: crate::accounts::fetch(),
                    logins: jcode_base::auth::account_pool::oauth_logins(),
                    pool: jcode_base::auth::account_pool::AccountPool::load(),
                    default_route,
                    default_key: None,
                }
            }
        });
        state.accounts_task = Some(cx.spawn(async move |this, cx| {
            let data = accounts_task.await;
            let _ = this.update(cx, |panel, cx| {
                let mut data = data;
                let providers = panel
                    .login
                    .as_ref()
                    .map(|state| state.providers.clone())
                    .unwrap_or_default();
                data.resolve_default(&providers);
                if let Some(state) = panel.login.as_mut() {
                    state.accounts = data;
                }
                panel.rebuild_provider_entries(cx);
            });
        }));
        let provider_ids: Vec<_> = state
            .providers
            .iter()
            .map(|provider| provider.id.to_owned())
            .collect();
        let task = cx.background_executor().spawn(async move {
            if offline {
                let mut statuses = ConnectionStatuses::from([
                    ("openai".into(), ConnectionStatus::Connected),
                    ("openai-api".into(), ConnectionStatus::NotConnected),
                    ("claude".into(), ConnectionStatus::Expired),
                    ("gemini".into(), ConnectionStatus::Unverified),
                    ("copilot".into(), ConnectionStatus::Failed),
                ]);
                for id in provider_ids {
                    statuses.entry(id).or_insert(ConnectionStatus::NotConnected);
                }
                (Some(statuses), None)
            } else {
                (
                    connection::fetch_connection_statuses(),
                    jcode_base::model_usage::method_usage_counts().ok(),
                )
            }
        });
        state.status_task = Some(cx.spawn(async move |this, cx| {
            let (statuses, usage) = task.await;
            let _ = this.update(cx, |panel, cx| {
                if let Some(state) = panel.login.as_mut() {
                    state.statuses = statuses;
                    state.usage = usage;
                    state.status_loading = false;
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    /// Send one real request through `provider` (or every configured
    /// provider) and fold the results into the row statuses.
    pub(super) fn live_test_providers(&mut self, provider: Option<String>, cx: &mut Context<Self>) {
        let offline =
            self.preview_state.is_some() || cfg!(test) || crate::harness::screenshot_mode();
        let Some(state) = self.login.as_mut() else {
            return;
        };
        if state.live_test_all
            || provider
                .as_ref()
                .is_some_and(|id| state.live_testing.contains(id))
        {
            return;
        }
        match &provider {
            Some(id) => {
                state.live_testing.insert(id.clone());
                state.live_errors.remove(id);
            }
            None => {
                state.live_test_all = true;
                state.live_errors.clear();
            }
        }
        let arg = provider.clone();
        let task = cx.background_executor().spawn(async move {
            if offline {
                None
            } else {
                connection::run_live_test(arg.as_deref())
            }
        });
        state.live_tasks.push(cx.spawn(async move |this, cx| {
            let results = task.await;
            let _ = this.update(cx, |panel, cx| {
                let Some(state) = panel.login.as_mut() else {
                    return;
                };
                match &provider {
                    Some(id) => {
                        state.live_testing.remove(id);
                    }
                    None => state.live_test_all = false,
                }
                match results {
                    Some(results) => {
                        let statuses = state.statuses.get_or_insert_with(Default::default);
                        for (id, result) in results {
                            // A single-provider run only reports that provider.
                            if provider.as_ref().is_some_and(|p| *p != id) {
                                continue;
                            }
                            if let Some(error) = result.error {
                                state.live_errors.insert(id.clone(), error);
                            }
                            statuses.insert(id, result.status);
                        }
                    }
                    None if !offline => {
                        state.error = Some(
                            "The live test did not finish. Check your network and try again."
                                .into(),
                        );
                    }
                    None => {}
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn set_preview_login_error(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.login.as_mut() {
            state.error = Some("Sign-in failed: the authorization code has expired. Offline preview: no browser was opened and no credentials were changed.".into());
            state.busy = false;
        }
        cx.notify();
    }

    pub(super) fn close_login_picker(&mut self, cx: &mut Context<Self>) {
        self.login = None;
        cx.notify();
    }

    pub(crate) fn login_command(&mut self, content: &str, cx: &mut Context<Self>) -> bool {
        let mut words = content.split_whitespace();
        if words.next() != Some("/login") {
            return false;
        }
        self.open_login_picker(cx);
        if let Some(provider) = words.next() {
            if words.next().is_some() {
                self.login.as_mut().unwrap().error =
                    Some("Choose a provider below to sign in.".into());
            } else if let Some(provider) = {
                let state = self.login.as_ref().unwrap();
                if self.preview_state.is_some() {
                    state
                        .providers
                        .iter()
                        .find(|entry| entry.id == provider)
                        .copied()
                } else {
                    state.client.resolve_provider(provider)
                }
            } {
                self.select_login_provider(provider, cx);
            } else {
                self.login.as_mut().unwrap().error =
                    Some("That provider is not available here. Choose one below.".into());
            }
        }
        true
    }

    /// Runtime context that decides which provider actions are safe.
    fn inventory_context(&self) -> crate::providers::InventoryContext {
        crate::providers::InventoryContext::current(
            !self.login_is_remote(),
            crate::providers::provider_admin_supported(),
        )
    }

    /// Merge the fetched accounts with the last known runtime routes.
    fn provider_inventory(&self) -> Vec<crate::providers::ProviderEntry> {
        let Some(state) = self.login.as_ref() else {
            return Vec::new();
        };
        let Some(accounts) = state.accounts.accounts.as_ref() else {
            return Vec::new();
        };
        crate::providers::inventory(accounts, &self.routes, &self.inventory_context())
    }

    /// Recompute provider rows after accounts, routes, or credentials change.
    pub(super) fn rebuild_provider_entries(&mut self, cx: &mut Context<Self>) {
        let entries = self.provider_inventory();
        if let Some(state) = self.login.as_mut() {
            state.entries = entries;
        }
        cx.notify();
    }

    /// Switch to a provider from the runtime inventory (catalog entries and
    /// named profiles alike), using the runtime's routing policy.
    pub(super) fn select_provider_entry(
        &mut self,
        entry: &crate::providers::ProviderEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let spec = self.input.read(cx).provider_route_spec_for(
            &entry.id,
            &entry.display_name,
            self.model.as_deref(),
        );
        let Some(spec) = spec else {
            if let Some(state) = self.login.as_mut() {
                state.error = Some(format!(
                    "{} has no available model for this session.",
                    entry.display_name
                ));
            }
            cx.notify();
            return;
        };
        self.close_login_picker(cx);
        if self.preview_state.is_none() {
            self.bridge.send(Command::SetModel {
                session_id: self.session_id.clone(),
                model: spec,
            });
        }
        self.items.push(Item::Assistant(format!(
            "Switching provider to {}…",
            entry.display_name
        )));
        self.focus_input(window, cx);
        cx.notify();
    }

    fn select_login_provider(&mut self, provider: LoginProvider, cx: &mut Context<Self>) {
        self.select_login_provider_for(provider, None, cx)
    }

    /// Sign in to `provider`. `account` names an OAuth login to refresh, or an
    /// unknown label to add another login next to the existing ones.
    fn select_login_provider_for(
        &mut self,
        provider: LoginProvider,
        account: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.login_is_remote() {
            return;
        }
        // Keep the client and cached status. Cancel only the previous attempt.
        let Some(state) = self.login.as_mut() else {
            return;
        };
        state.task = None;
        state.callback_task = None;
        state.callback_waiting = false;
        state.authorized = false;
        state.manual_entry = false;
        let previous_flow = state.flow.take();
        state.provider = Some(provider);
        state.prompt = None;
        state.error = None;
        state.complete = false;
        state.busy = false;
        state.browser_opened = false;
        state.focus_pending = true;
        state.scroll.set_offset(point(px(0.), px(0.)));
        state.input = cx.new(|cx| LoginInput::new(cx, "Paste the code or callback URL"));
        if self.preview_state.is_some() {
            if provider.method == LoginMethod::ApiKey {
                state.input = cx.new(|cx| LoginInput::new(cx, "Paste your API key"));
            }
            self.set_preview_login_error(cx);
            return;
        }
        if provider.method == LoginMethod::ApiKey {
            if let Some(flow) = previous_flow {
                std::thread::spawn(move || {
                    let _ = flow.cancel();
                });
            }
            state.input = cx.new(|cx| LoginInput::new(cx, "Paste your API key"));
        } else {
            match state.client.begin(provider.id, account.as_deref()) {
                Ok(flow) => {
                    state.flow = Some(flow.clone());
                    self.run_login_task(
                        move || {
                            // Release the old callback port before binding the new
                            // attempt. Parallel cancellation made retries manual-only.
                            if let Some(previous) = previous_flow {
                                previous.cancel().map_err(|e| e.to_string())?;
                            }
                            flow.start()
                                .map(LoginUpdate::Prompt)
                                .map_err(|e| e.to_string())
                        },
                        cx,
                    );
                }
                Err(error) => state.error = Some(error.to_string()),
            }
        }
        cx.notify();
    }

    fn run_login_task(
        &mut self,
        work: impl FnOnce() -> Result<LoginUpdate, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.preview_state.is_some() {
            self.set_preview_login_error(cx);
            return;
        }
        let state = self.login.as_mut().unwrap();
        state.busy = true;
        state.error = None;
        let attempt = state.input.entity_id();
        let task = cx.background_executor().spawn(async move {
            if crate::harness::screenshot_mode() {
                return Err(
                    "Sign-in is disabled in the offline preview. No credentials were changed."
                        .into(),
                );
            }
            work()
        });
        state.task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |panel, cx| {
                let Some(state) = panel.login.as_mut() else {
                    return;
                };
                if state.input.entity_id() != attempt || state.complete {
                    return;
                }
                state.busy = false;
                match result {
                    Ok(LoginUpdate::Prompt(prompt)) => {
                        state.focus_pending = true;
                        state.prompt = Some(prompt);
                    }
                    Ok(LoginUpdate::Complete { validation_warning }) => {
                        panel.complete_login(validation_warning, cx);
                    }
                    Err(error) => state.error = Some(error),
                }
                // Device authorization finishes automatically. Keep its code and
                // browser controls visible while the background poll is running.
                let poll_device = panel.login.as_ref().is_some_and(|state| {
                    !state.complete
                        && state.error.is_none()
                        && state
                            .prompt
                            .as_ref()
                            .is_some_and(|prompt| prompt.input_kind == AuthInputKind::DeviceCode)
                });
                if poll_device {
                    panel.submit_login(cx);
                } else {
                    panel.wait_for_login_callback(cx);
                }
                cx.notify();
            });
        }));
    }

    fn complete_login(&mut self, validation_warning: bool, cx: &mut Context<Self>) {
        let Some(state) = self.login.as_mut() else {
            return;
        };
        if state.complete {
            return;
        }
        state.complete = true;
        state.busy = false;
        state.callback_waiting = false;
        state.flow = None;
        state.prompt = None;
        state.key_provider = None;
        state.input.update(cx, |input, cx| input.clear(cx));
        state.error = validation_warning.then(|| "Credentials were saved, but the provider could not be verified. If this session does not switch, choose another model. You do not need to reuse the sign-in code.".into());
        let provider = state.provider.map(|provider| provider.id.to_string());
        // The CLI and API-key paths notify the daemon on a throwaway
        // connection. That refreshes credentials everywhere, but the automatic
        // post-login model switch only applies to the notifying session, so
        // the session this login is for must notify on its own connection.
        if let Some(session_id) = self.login_target_session() {
            self.bridge.send(match provider {
                Some(provider) => Command::AuthChanged {
                    session_id,
                    provider,
                },
                None => Command::RefreshRuntime { session_id },
            });
        }
        crate::accounts::request_refresh();
        cx.notify();
    }

    /// The live session whose model should follow this login, if any. An
    /// Accounts panel signs in on behalf of the chat it was opened from.
    fn login_target_session(&self) -> Option<String> {
        if let Some(source) = self.session_id.strip_prefix("accounts://") {
            // Drafts, settings pages and remote sessions have no local runtime.
            return (!source.is_empty() && !source.contains("://")).then(|| source.to_string());
        }
        self.can_refresh_account_runtime()
            .then(|| self.session_id.clone())
    }

    fn wait_for_login_callback(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.login.as_mut() else {
            return;
        };
        if state.complete || state.callback_task.is_some() || state.error.is_some() {
            return;
        }
        let Some(flow) = state
            .flow
            .as_ref()
            .filter(|flow| flow.has_callback_listener())
            .cloned()
        else {
            return;
        };
        let attempt = state.input.entity_id();
        state.callback_waiting = true;
        let (sender, receiver) = async_channel::unbounded();
        let task = cx.background_executor().spawn(async move {
            let result = flow.wait_for_callback_with_progress(|| {
                let _ = sender.send_blocking(None);
            });
            let _ = sender.send_blocking(Some(result));
        });
        state.callback_task = Some(cx.spawn(async move |this, cx| {
            let _worker = task;
            let mut exchanging = false;
            while let Ok(update) = receiver.recv().await {
                let done = update.is_some();
                let keep_going = this
                    .update(cx, |panel, cx| {
                        let Some(state) = panel.login.as_mut() else {
                            return false;
                        };
                        if state.input.entity_id() != attempt || state.complete {
                            return false;
                        }
                        match update {
                            None => {
                                exchanging = true;
                                state.authorized = true;
                                state.callback_waiting = false;
                                state.busy = true;
                            }
                            Some(result) => {
                                state.callback_waiting = false;
                                match result {
                                    Ok(result) => {
                                        panel.complete_login(result.validation_warning, cx)
                                    }
                                    Err(_) if state.busy && !exchanging => {} // Manual submission owns completion.
                                    Err(error) => {
                                        state.busy = false;
                                        state.error = Some(error.to_string());
                                    }
                                }
                            }
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if done || !keep_going {
                    break;
                }
            }
        }));
    }

    fn submit_login(&mut self, cx: &mut Context<Self>) {
        if self.preview_state.is_some() {
            if let Some(state) = self.login.as_mut() {
                state.input.update(cx, |input, cx| input.clear(cx));
            }
            self.set_preview_login_error(cx);
            return;
        }
        let Some(state) = self.login.as_mut() else {
            return;
        };
        if state.busy || state.complete {
            return;
        }
        if state.key_provider.is_some() {
            self.submit_provider_key(cx);
            return;
        }
        let Some(provider) = state.provider.clone() else {
            return;
        };
        // Enter while the browser is expected to return must not fail the step.
        if provider.method != LoginMethod::ApiKey
            && state
                .prompt
                .as_ref()
                .is_some_and(|prompt| prompt.input_kind != AuthInputKind::DeviceCode)
            && !manual_input_visible(state)
        {
            return;
        }
        let secret = state.input.update(cx, |input, cx| input.take(cx));
        if provider.method == LoginMethod::ApiKey {
            if secret.trim().is_empty() {
                state.error = Some("Paste an API key first.".into());
                cx.notify();
                return;
            }
            self.run_login_task(
                move || {
                    let client = jcode_sdk::JcodeClient::connect(jcode_sdk::ConnectOptions {
                        client_name: "jcode-desktop-login".into(),
                        ensure_runtime: false,
                        ..Default::default()
                    })
                    .map_err(|_| {
                        "Could not connect to Jcode. Try again when the connection is restored."
                            .to_string()
                    })?;
                    client
                        .set_api_key(provider.id, secret.trim())
                        .map_err(|_| {
                            "Could not save this API key. Check the key and try again.".to_string()
                        })?;
                    Ok(LoginUpdate::Complete {
                        validation_warning: false,
                    })
                },
                cx,
            );
            return;
        }
        let (Some(flow), Some(prompt)) = (state.flow.clone(), state.prompt.as_ref()) else {
            return;
        };
        let kind = prompt.input_kind.clone();
        if kind != AuthInputKind::DeviceCode && secret.trim().is_empty() {
            state.error = Some("Paste the code or callback URL from your browser first.".into());
            cx.notify();
            return;
        }
        self.run_login_task(
            move || {
                let result = match kind {
                    AuthInputKind::DeviceCode => flow.complete_device(),
                    AuthInputKind::CallbackUrl => flow.submit_callback(secret.trim()),
                    AuthInputKind::AuthCode => flow.submit_code(secret.trim()),
                    AuthInputKind::AuthCodeOrCallbackUrl => {
                        if secret.trim().starts_with("http") {
                            flow.submit_callback(secret.trim())
                        } else {
                            flow.submit_code(secret.trim())
                        }
                    }
                };
                let result = result.map_err(|e| e.to_string())?;
                Ok(LoginUpdate::Complete {
                    validation_warning: result.validation_warning,
                })
            },
            cx,
        );
    }

    pub(super) fn render_login_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if self
            .login
            .as_ref()
            .is_some_and(|state| state.selection_only)
        {
            return None;
        }
        if let Some(state) = self.login.as_mut()
            && state.focus_pending
        {
            state.focus_pending = false;
            if state.provider.is_none() {
                state.search.focus_handle(cx).focus(window, cx);
            } else if state
                .provider
                .as_ref()
                .is_some_and(|provider| provider.method == LoginMethod::ApiKey)
            {
                let focus = state.input.read(cx).focus_handle.clone();
                focus.focus(window, cx);
            } else if manual_input_visible(state) {
                let focus = state.input.read(cx).focus_handle.clone();
                focus.focus(window, cx);
            } else {
                self.focus_handle.focus(window, cx);
            }
        }
        if let Some(state) = self.login.as_mut()
            && !state.browser_opened
            && let Some(prompt) = &state.prompt
        {
            state.browser_opened = true;
            if !cfg!(test) && self.preview_state.is_none() && !crate::harness::screenshot_mode() {
                cx.open_url(&prompt.auth_url);
            }
        }
        let state = self.login.as_ref()?;
        let theme = Theme::global();
        let remote = self.login_is_remote();
        let mut body = div().flex().flex_col().gap_3();
        // An in-progress sign-in shows its error inside the failed step.
        if let Some(error) = state
            .error
            .as_ref()
            .filter(|_| state.provider.is_none() || state.complete)
        {
            body = body.child(
                div()
                    .debug_selector(|| "login-error".into())
                    .text_color(theme.ERROR)
                    .child(error.clone()),
            );
        }
        if state.complete {
            if let Some(provider) = state.provider {
                body = body.child(self.render_login_steps(state, provider, cx));
            }
            body = body
                .child(
                    div()
                        .debug_selector(|| "login-complete".into())
                        .text_color(if state.error.is_some() {
                            theme.WARN
                        } else {
                            theme.OK
                        })
                        .child(if self.login_target_session().is_some() {
                            "Account connected. This session now uses it."
                        } else {
                            "Account connected."
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(login_button("login-done", "Done").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.close_login_picker(cx);
                                if this.is_accounts_panel() {
                                    cx.emit(AccountsPanelClosed);
                                } else {
                                    this.focus_input(window, cx);
                                }
                            },
                        )))
                        .child(
                            login_button("login-choose-model", "Pick a different model").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.close_login_picker(cx);
                                    if this.is_accounts_panel() {
                                        cx.emit(AccountsPanelChooseModel);
                                    } else {
                                        this.open_recovery_models(cx);
                                    }
                                }),
                            ),
                        ),
                );
        } else if let Some(provider) = state.provider {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(login_logo(&provider))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("Sign in to {}", provider.display_name)),
                    ),
            );
            let status =
                connection::status_for(state.statuses.as_ref(), provider.id, state.status_loading);
            if matches!(status, ConnectionStatus::Expired | ConnectionStatus::Failed) {
                body = body.child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.TEXT_DIM)
                        .child(status.detail()),
                );
            }
            body = body.child(self.render_login_steps(state, provider, cx));
        } else if remote {
            body = body.child(self.render_providers_section(cx));
        } else if !remote {
            let providers = catalog::filtered_providers(
                &state.providers,
                &state.search.read(cx).content,
                state.statuses.as_ref(),
                state.status_loading,
                state.usage.as_ref(),
            );
            if providers.is_empty() {
                body = body.child(
                    div()
                        .debug_selector(|| "login-search-empty".into())
                        .text_color(theme.TEXT_DIM)
                        .child("No accounts match your search."),
                );
            }
            body = body.child(self.render_providers_section(cx));
            let rows = self.render_account_catalog(&providers, cx);
            body = body.children(rows);
        }
        Some(
            div()
                .id("login-dialog")
                .debug_selector(|| "login-dialog".into())
                .absolute()
                .inset_0()
                .size_full()
                .bg(theme.PANEL_BG)
                .occlude()
                .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" && !event.is_held {
                        this.close_login_picker(cx);
                        if this.is_accounts_panel() {
                            cx.emit(AccountsPanelClosed);
                        } else {
                            this.focus_input(window, cx);
                        }
                        cx.stop_propagation();
                    }
                }))
                .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                    if event.keystroke.key == "enter" && !event.is_held {
                        this.submit_login(cx);
                        cx.stop_propagation();
                    }
                }))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .flex()
                .flex_col()
                .p_4()
                .gap_3()
                .text_size(px(14.))
                .text_color(theme.TEXT)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Accounts"),
                        )
                        .when(
                            state.provider.is_none() && !state.complete && !remote,
                            |el| {
                                let running = state.live_test_all;
                                el.child(
                                    login_button(
                                        "login-test-all",
                                        if running { "Testing all…" } else { "Test all" },
                                    )
                                    .when(running, |el| el.opacity(0.6))
                                    .tooltip(|_, cx| {
                                        cx.new(|_| {
                                            super::usage::MeterTooltip(
                                                "Send one small live request through every connected account to confirm each one works. Uses a little quota or credit.".into(),
                                            )
                                        })
                                        .into()
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.live_test_providers(None, cx)
                                    })),
                                )
                            },
                        )
                        .child(
                            login_button(
                                "login-close",
                                if state.busy { "Cancel" } else { "Close" },
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.close_login_picker(cx);
                                    if this.is_accounts_panel() {
                                        cx.emit(AccountsPanelClosed);
                                    } else {
                                        this.focus_input(window, cx);
                                    }
                                },
                            )),
                        ),
                )
                .when(
                    state.provider.is_none() && !state.complete && !remote,
                    |el| {
                        el.child(
                            div()
                                .debug_selector(|| "login-search".into())
                                .flex_none()
                                .child(state.search.clone()),
                        )
                    },
                )
                .child(
                    div()
                        .id("login-body")
                        .track_scroll(&state.scroll)
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(body),
                )
                .children(self.render_provider_dialogs(cx))
                .into_any_element(),
        )
    }

    #[cfg(test)]
    pub(super) fn test_set_provider_entries(
        &mut self,
        entries: Vec<crate::providers::ProviderEntry>,
    ) {
        if let Some(state) = self.login.as_mut() {
            state.entries = entries;
        }
    }

    #[cfg(test)]
    pub(super) fn test_select_provider_entry(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self
            .login
            .as_ref()
            .and_then(|state| state.entries.iter().find(|entry| entry.id == id))
            .cloned()
        else {
            return;
        };
        self.select_provider_entry(&entry, window, cx);
    }

    #[cfg(test)]
    pub(super) fn test_open_add_provider(&mut self, cx: &mut Context<Self>) {
        self.open_add_provider(cx);
    }

    #[cfg(test)]
    pub(super) fn test_open_provider_key(
        &mut self,
        provider: &str,
        display: &str,
        cx: &mut Context<Self>,
    ) {
        self.open_provider_key(provider, display, cx);
    }

    #[cfg(test)]
    pub(super) fn test_open_remove_provider(&mut self, provider: String, cx: &mut Context<Self>) {
        self.open_remove_provider(provider, cx);
    }

    fn paste_login(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(state) = &self.login {
            state.input.update(cx, |input, cx| input.paste(window, cx));
        }
    }

    // ---- Providers surface -------------------------------------------------

    fn set_add_provider_error(&mut self, message: String, cx: &mut Context<Self>) {
        if let Some(draft) = self
            .login
            .as_mut()
            .and_then(|state| state.add_provider.as_mut())
        {
            draft.busy = false;
            draft.error = Some(message);
        }
        cx.notify();
    }

    fn open_add_provider(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.login.as_mut() else {
            return;
        };
        let field = |cx: &mut Context<Self>, placeholder: &str, id: &str| {
            cx.new(|cx| LoginInput::new_plain(cx, placeholder).with_element_id(id))
        };
        state.add_provider = Some(AddProviderState {
            name: field(cx, "Provider name, e.g. my-gateway", "provider-add-name"),
            base_url: field(cx, "https://llm.example.com/v1", "provider-add-base"),
            model: field(cx, "Default model id", "provider-add-model"),
            context_window: field(cx, "Context window (optional)", "provider-add-context"),
            api_key_env: field(cx, "API key env var (optional)", "provider-add-env"),
            api_key: cx.new(|cx| {
                LoginInput::new(cx, "Paste your API key").with_element_id("provider-add-key")
            }),
            no_api_key: false,
            busy: false,
            error: None,
        });
        cx.notify();
    }

    fn add_provider_spec(&self, cx: &App) -> Result<jcode_sdk::ProviderProfileSpec, String> {
        let Some(state) = self.login.as_ref() else {
            return Err("The provider form is not open.".into());
        };
        let Some(draft) = state.add_provider.as_ref() else {
            return Err("The provider form is not open.".into());
        };
        let text = |input: &Entity<LoginInput>| input.read(cx).text().trim().to_string();
        let context_window = match text(&draft.context_window).as_str() {
            "" => None,
            raw => Some(
                raw.parse::<usize>()
                    .map_err(|_| "Context window must be a whole number of tokens.")?,
            ),
        };
        let api_key_env = text(&draft.api_key_env);
        let api_key = text(&draft.api_key);
        let spec = jcode_sdk::ProviderProfileSpec {
            name: text(&draft.name),
            base_url: text(&draft.base_url),
            model: text(&draft.model),
            context_window,
            api_key_env: (!api_key_env.is_empty()).then_some(api_key_env),
            api_key: (!api_key.is_empty()).then_some(api_key),
            env_file: None,
            no_api_key: draft.no_api_key,
            auth: None,
            auth_header: None,
            set_default: false,
            overwrite: false,
            provider_routing: false,
            model_catalog: false,
        };
        crate::providers::validate_profile_spec(&spec)
    }

    fn submit_add_provider(&mut self, cx: &mut Context<Self>) {
        let spec = match self.add_provider_spec(cx) {
            Ok(spec) => spec,
            Err(message) => {
                self.set_add_provider_error(message, cx);
                return;
            }
        };
        let offline = self.preview_state.is_some()
            || cfg!(test)
            || crate::harness::screenshot_mode();
        if offline {
            if let Some(state) = self.login.as_mut() {
                state.add_provider = None;
            }
            self.refresh_login_status(cx);
            return;
        }
        if let Some(draft) = self
            .login
            .as_mut()
            .and_then(|state| state.add_provider.as_mut())
        {
            draft.busy = true;
            draft.error = None;
        }
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { crate::providers::add_provider_profile(spec) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |panel, cx| match result {
                Ok(()) => {
                    if let Some(state) = panel.login.as_mut() {
                        state.add_provider = None;
                    }
                    panel.refresh_login_status(cx);
                }
                Err(error) => {
                    panel.set_add_provider_error(error.to_string(), cx);
                }
            });
        })
        .detach();
    }

    fn open_provider_key(&mut self, provider: &str, display_name: &str, cx: &mut Context<Self>) {
        let Some(state) = self.login.as_mut() else {
            return;
        };
        state.provider = None;
        state.prompt = None;
        state.complete = false;
        state.busy = false;
        state.error = None;
        state.key_provider = Some(ProviderKeyState {
            provider: provider.to_string(),
            display_name: display_name.to_string(),
            busy: false,
            error: None,
        });
        state.input = cx.new(|cx| LoginInput::new(cx, "Paste your API key"));
        state.focus_pending = true;
        cx.notify();
    }

    fn submit_provider_key(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.login.as_mut() else {
            return;
        };
        let Some((provider, display)) = state
            .key_provider
            .as_ref()
            .map(|key| (key.provider.clone(), key.display_name.clone()))
        else {
            return;
        };
        let secret = state.input.update(cx, |input, cx| input.take(cx));
        if secret.trim().is_empty() {
            state.error = Some(format!("Paste the API key for {display}."));
            cx.notify();
            return;
        }
        let offline = self.preview_state.is_some()
            || cfg!(test)
            || crate::harness::screenshot_mode();
        if offline {
            if let Some(state) = self.login.as_mut() {
                state.key_provider = None;
            }
            self.refresh_login_status(cx);
            return;
        }
        self.run_login_task(
            move || {
                crate::providers::set_provider_api_key(&provider, &secret).map_err(|error| {
                    format!("Could not save the API key for {display}: {error}")
                })?;
                Ok(LoginUpdate::Complete {
                    validation_warning: false,
                })
            },
            cx,
        );
    }

    fn open_remove_provider(&mut self, provider: String, cx: &mut Context<Self>) {
        if let Some(state) = self.login.as_mut() {
            state.remove_provider = Some(provider);
        }
        cx.notify();
    }

    fn submit_remove_provider(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self
            .login
            .as_mut()
            .and_then(|state| state.remove_provider.take())
        else {
            return;
        };
        let offline = self.preview_state.is_some()
            || cfg!(test)
            || crate::harness::screenshot_mode();
        if offline {
            self.refresh_login_status(cx);
            return;
        }
        let task = cx
            .background_executor()
            .spawn(async move { crate::providers::remove_provider_profile(&name) });
        cx.spawn(async move |this, cx| {
            let _ = task.await;
            let _ = this.update(cx, |panel, cx| {
                panel.refresh_login_status(cx);
            });
        })
        .detach();
    }

    fn provider_pill(
        &self,
        id: String,
        label: &'static str,
        color: gpui::Rgba,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::global();
        let selector = id.clone();
        div()
            .id(SharedString::from(id))
            .debug_selector(move || selector.clone())
            .flex_none()
            .px_3()
            .py_1()
            .rounded_full()
            .bg(color.opacity(0.16))
            .text_size(px(11.0))
            .text_color(color)
            .cursor_pointer()
            .hover(|el| el.bg(color.opacity(0.28)))
            .child(label)
            .when(color == theme.ACCENT, |el| el)
    }

    fn render_provider_admin_row(
        &self,
        entry: &crate::providers::ProviderEntry,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use crate::providers::ProviderAction;
        let theme = Theme::global();
        let ink = if entry.available() {
            theme.TEXT
        } else {
            theme.TEXT_DIM
        };
        let models = entry.available_model_count();
        let model_text = match models {
            0 => "No usable model".to_string(),
            1 => "1 model".to_string(),
            count => format!("{count} models"),
        };
        let logo: gpui::AnyElement = match crate::accounts::logo(&entry.id) {
            Some(bytes) => gpui::svg()
                .data(bytes)
                .size(px(16.0))
                .flex_none()
                .text_color(ink)
                .into_any_element(),
            None => div()
                .size(px(16.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .bg(theme.INLINE_CODE_BG)
                .text_size(px(10.0))
                .text_color(ink)
                .child(crate::accounts::lettermark(&entry.display_name))
                .into_any_element(),
        };
        let mut actions = div().flex_none().flex().items_center().gap_1p5();
        if entry.offers(ProviderAction::Use) {
            let target = entry.clone();
            actions = actions.child(self.provider_pill(
                format!("provider-use-{}", entry.id),
                "Use",
                theme.ACCENT,
            ).on_click(cx.listener(move |this, _, window, cx| {
                this.select_provider_entry(&target, window, cx);
            })));
        }
        if entry.offers(ProviderAction::SetApiKey) {
            let provider = entry.id.clone();
            let name = entry.display_name.clone();
            actions = actions.child(self.provider_pill(
                format!("provider-key-{}", entry.id),
                "Set key",
                theme.ACCENT,
            ).on_click(cx.listener(move |this, _, _, cx| {
                this.open_provider_key(&provider, &name, cx);
            })));
        }
        if entry.offers(ProviderAction::SignIn) {
            let provider = self
                .login
                .as_ref()
                .and_then(|state| state.providers.iter().find(|p| p.id == entry.id).copied());
            if let Some(provider) = provider {
                actions = actions.child(self.provider_pill(
                    format!("provider-signin-{}", entry.id),
                    "Sign in",
                    theme.ACCENT,
                ).on_click(cx.listener(move |this, _, _, cx| {
                    this.select_login_provider_for(provider, None, cx);
                })));
            }
        }
        if entry.offers(ProviderAction::Test) {
            let id = entry.id.clone();
            actions = actions.child(self.provider_pill(
                format!("provider-test-{}", entry.id),
                "Test",
                theme.TEXT_DIM,
            ).on_click(cx.listener(move |this, _, _, cx| {
                this.live_test_providers(Some(id.clone()), cx);
            })));
        }
        if entry.offers(ProviderAction::Remove) {
            let id = entry.id.clone();
            actions = actions.child(self.provider_pill(
                format!("provider-remove-{}", entry.id),
                "Remove",
                theme.ERROR,
            ).on_click(cx.listener(move |this, _, _, cx| {
                this.open_remove_provider(id.clone(), cx);
            })));
        }
        let selector = format!("provider-row-{}", entry.id);
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector({
                let selector = selector.clone();
                move || selector
            })
            .mx_2()
            .px_3()
            .py_2()
            .rounded_full()
            .bg(theme.HEADER_BG)
            .flex()
            .items_center()
            .gap_2()
            .child(logo)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(16.0))
                            .line_height(px(16.0))
                            .child(
                                div()
                                    .flex_none()
                                    .truncate()
                                    .text_size(px(13.0))
                                    .text_color(ink)
                                    .child(entry.display_name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(10.0))
                                    .text_color(theme.TEXT_DIM)
                                    .child(entry.auth_kind.clone()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .line_height(px(13.0))
                            .text_color(theme.TEXT_DIM)
                            .child(format!("{} · {model_text}", entry.status_label())),
                    ),
            )
            .child(actions)
            .into_any_element()
    }

    fn render_providers_section(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = Theme::global();
        let Some(state) = self.login.as_ref() else {
            return div().into_any_element();
        };
        let remote = self.login_is_remote();
        let query = state.search.read(cx).content.to_lowercase();
        let entries: Vec<_> = state
            .entries
            .iter()
            .filter(|entry| {
                format!(
                    "{} {} {} {}",
                    entry.display_name, entry.id, entry.auth_kind, entry.method
                )
                .to_lowercase()
                .contains(query.trim())
            })
            .cloned()
            .collect();
        let can_admin = !remote && crate::providers::provider_admin_supported();
        let mut section = div()
            .id("providers-section")
            .debug_selector(|| "providers-section".into())
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .pt_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .pb_1()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Providers"),
                    )
                    .when(can_admin, |el| {
                        el.child(
                            div()
                                .id("providers-add")
                                .debug_selector(|| "providers-add".into())
                                .flex_none()
                                .px_3()
                                .py_1()
                                .rounded_full()
                                .bg(theme.ACCENT.opacity(0.16))
                                .text_size(px(11.0))
                                .text_color(theme.ACCENT)
                                .cursor_pointer()
                                .hover(|el| el.bg(theme.ACCENT.opacity(0.28)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open_add_provider(cx);
                                }))
                                .child("Add provider"),
                        )
                    }),
            );
        if remote {
            section = section.child(
                div()
                    .debug_selector(|| "providers-remote-guidance".into())
                    .px_2()
                    .pb_1()
                    .text_size(px(11.0))
                    .text_color(theme.TEXT_DIM)
                    .child("Providers for this remote runtime are managed on its host. This list is read-only."),
            );
        }
        if entries.is_empty() {
            section = section.child(
                div()
                    .debug_selector(|| "providers-empty".into())
                    .px_2()
                    .pb_1()
                    .text_size(px(11.0))
                    .text_color(theme.TEXT_DIM)
                    .child(if query.trim().is_empty() {
                        "No providers reported by the runtime yet."
                    } else {
                        "No providers match your search."
                    }),
            );
        }
        for entry in &entries {
            section = section.child(self.render_provider_admin_row(entry, cx));
        }
        section.child(div().h(px(6.0))).into_any_element()
    }

    fn provider_dialog_frame(&self, id: &'static str, child: gpui::AnyElement) -> gpui::AnyElement {
        let theme = Theme::global();
        div()
            .id(id)
            .debug_selector(move || id.into())
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.BG.opacity(0.66))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .w(px(440.0))
                    .p_4()
                    .rounded_xl()
                    .bg(theme.PANEL_BG)
                    .border_1()
                    .border_color(theme.PANEL_BORDER)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(child),
            )
            .into_any_element()
    }

    fn render_provider_dialogs(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let theme = Theme::global();
        let mut dialogs = Vec::new();
        if let Some(draft) = self.login.as_ref().and_then(|state| state.add_provider.as_ref()) {
            let field = |label: &'static str, input: &Entity<LoginInput>| {
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.TEXT_DIM)
                            .child(label),
                    )
                    .child(input.clone())
            };
            let mut body = div().flex().flex_col().gap_2().child(
                div()
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Add OpenAI-compatible provider"),
            );
            body = body
                .child(field("Name", &draft.name))
                .child(field("Base URL", &draft.base_url))
                .child(field("Default model", &draft.model))
                .child(field("Context window", &draft.context_window))
                .child(field("API key env var", &draft.api_key_env))
                .child(field("API key", &draft.api_key));
            if draft.no_api_key {
                body = body.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.TEXT_DIM)
                        .child("Stored without a key. Add one later if the endpoint needs it."),
                );
            }
            if let Some(error) = &draft.error {
                body = body.child(div().text_size(px(11.0)).text_color(theme.ERROR).child(error.clone()));
            }
            let busy = draft.busy;
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pt_1()
                    .child(
                        div()
                            .id("provider-add-save")
                            .debug_selector(|| "provider-add-save".into())
                            .px_4()
                            .py_1p5()
                            .rounded_full()
                            .bg(theme.ACCENT_DIM)
                            .text_color(theme.TEXT)
                            .text_size(px(12.0))
                            .cursor_pointer()
                            .when(busy, |el| el.opacity(0.6))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.submit_add_provider(cx);
                            }))
                            .child(if busy { "Saving…" } else { "Save provider" }),
                    )
                    .child(
                        div()
                            .id("provider-add-cancel")
                            .debug_selector(|| "provider-add-cancel".into())
                            .px_4()
                            .py_1p5()
                            .rounded_full()
                            .bg(theme.PANEL_BORDER)
                            .text_color(theme.TEXT)
                            .text_size(px(12.0))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(state) = this.login.as_mut() {
                                    state.add_provider = None;
                                }
                                cx.notify();
                            }))
                            .child("Cancel"),
                    ),
            );
            dialogs.push(self.provider_dialog_frame("provider-add-dialog", body.into_any_element()));
        }

        if let Some(key) = self.login.as_ref().and_then(|state| state.key_provider.as_ref()) {
            let mut body = div().flex().flex_col().gap_2().child(
                div()
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("Set API key for {}", key.display_name)),
            );
            if let Some(state) = self.login.as_ref() {
                body = body.child(state.input.clone());
            }
            if let Some(error) = &key.error {
                body = body
                    .child(div().text_size(px(11.0)).text_color(theme.ERROR).child(error.clone()));
            }
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pt_1()
                    .child(
                        div()
                            .id("provider-key-save")
                            .debug_selector(|| "provider-key-save".into())
                            .px_4()
                            .py_1p5()
                            .rounded_full()
                            .bg(theme.ACCENT_DIM)
                            .text_color(theme.TEXT)
                            .text_size(px(12.0))
                            .cursor_pointer()
                            .when(key.busy, |el| el.opacity(0.6))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.submit_provider_key(cx);
                            }))
                            .child(if key.busy { "Saving…" } else { "Save key" }),
                    )
                    .child(
                        div()
                            .id("provider-key-cancel")
                            .debug_selector(|| "provider-key-cancel".into())
                            .px_4()
                            .py_1p5()
                            .rounded_full()
                            .bg(theme.PANEL_BORDER)
                            .text_color(theme.TEXT)
                            .text_size(px(12.0))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(state) = this.login.as_mut() {
                                    state.key_provider = None;
                                    state.input = cx.new(|cx| LoginInput::new(cx, "Paste the code or callback URL"));
                                }
                                cx.notify();
                            }))
                            .child("Cancel"),
                    ),
            );
            dialogs.push(self.provider_dialog_frame("provider-key-dialog", body.into_any_element()));
        }

        if let Some(name) = self.login.as_ref().and_then(|state| state.remove_provider.as_ref()) {
            let body = div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("Remove provider '{name}'?")),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme.TEXT_DIM)
                        .child("This deletes the profile and its stored API key. Sessions already using another provider are unaffected."),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .pt_1()
                        .child(
                            div()
                                .id("provider-remove-confirm")
                                .debug_selector(|| "provider-remove-confirm".into())
                                .px_4()
                                .py_1p5()
                                .rounded_full()
                                .bg(theme.ERROR.opacity(0.2))
                                .text_color(theme.ERROR)
                                .text_size(px(12.0))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.submit_remove_provider(cx);
                                }))
                                .child("Remove"),
                        )
                        .child(
                            div()
                                .id("provider-remove-cancel")
                                .debug_selector(|| "provider-remove-cancel".into())
                                .px_4()
                                .py_1p5()
                                .rounded_full()
                                .bg(theme.PANEL_BORDER)
                                .text_color(theme.TEXT)
                                .text_size(px(12.0))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(state) = this.login.as_mut() {
                                        state.remove_provider = None;
                                    }
                                    cx.notify();
                                }))
                                .child("Cancel"),
                        ),
                );
            dialogs.push(self.provider_dialog_frame("provider-remove-dialog", body.into_any_element()));
        }
        dialogs
    }
}

fn login_method_icon(method: LoginMethod) -> gpui::AnyElement {
    let (icon, label) = match method {
        LoginMethod::ApiKey => ("key", catalog::method_label(method)),
        LoginMethod::OAuth => ("browser", catalog::method_label(method)),
        LoginMethod::DeviceCode => ("computer", catalog::method_label(method)),
    };
    div()
        .debug_selector(move || format!("login-method-{icon}"))
        .flex_none()
        .flex()
        .items_center()
        .gap_1p5()
        .text_size(px(11.))
        .text_color(Theme::global().TEXT_DIM)
        .child(if method == LoginMethod::ApiKey {
            gpui::svg()
                .data(include_bytes!("../../../assets/icons/key.svg") as &'static [u8])
                .size(px(14.))
                .text_color(Theme::global().TEXT_DIM)
                .into_any_element()
        } else {
            crate::tool_icon::render(icon).into_any_element()
        })
        .child(label)
        .into_any_element()
}

fn login_logo(provider: &LoginProvider) -> gpui::AnyElement {
    let id = format!("login-logo-{}", provider.id);
    div()
        .debug_selector(move || id.clone())
        .size(px(22.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(match crate::accounts::logo(provider.id) {
            Some(bytes) => gpui::svg()
                .data(bytes)
                .size(px(20.))
                .text_color(Theme::global().TEXT)
                .into_any_element(),
            None => div()
                .child(crate::accounts::lettermark(provider.display_name))
                .into_any_element(),
        })
        .into_any_element()
}

fn login_button(id: &'static str, label: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .px_4()
        .py_1p5()
        .rounded_full()
        .bg(Theme::global().ACCENT_DIM)
        .text_color(Theme::global().TEXT)
        .cursor_pointer()
        .hover(|el| el.bg(Theme::global().PANEL_BORDER))
        .child(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn login_status_badges_and_search_are_visible(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new_accounts("login-test", None, crate::harness::spawn_inert(), cx)
        });
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("accounts-panel").is_some());
        assert!(vcx.debug_bounds("login-status-openai").is_some());
        assert!(vcx.debug_bounds("login-refresh-status").is_none());
        assert!(vcx.debug_bounds("login-search").is_some());
        panel.read_with(vcx, |panel, _| {
            let state = panel.login.as_ref().unwrap();
            assert!(!state.status_loading);
            assert_eq!(
                state.statuses.as_ref().unwrap()["openai"],
                ConnectionStatus::Connected
            );
        });
        vcx.simulate_input("no-such-provider");
        assert!(vcx.debug_bounds("login-search-empty").is_some());
        assert!(vcx.debug_bounds("login-provider-openai").is_none());
        panel.update(vcx, |panel, cx| {
            panel
                .login
                .as_ref()
                .unwrap()
                .search
                .update(cx, |input, cx| input.set_content("OPENAI".into(), cx));
            cx.notify();
        });
        assert!(vcx.debug_bounds("login-provider-openai").is_some());
    }

    #[gpui::test]
    fn login_accounts_back_keeps_dedicated_panel_and_remote_does_not_probe(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new_accounts("login-test", None, crate::harness::spawn_inert(), cx)
        });
        panel.update(vcx, |panel, cx| {
            panel.login_command("/login openai-api", cx);
        });
        let back = vcx.debug_bounds("login-back").unwrap();
        vcx.simulate_click(back.center(), gpui::Modifiers::default());
        assert!(vcx.debug_bounds("accounts-panel").is_some());
        assert!(vcx.debug_bounds("login-provider-openai").is_some());
        panel.update(vcx, |panel, cx| {
            panel.close_login_picker(cx);
            panel.session_id = "accounts://ssh://test-host/session".into();
            panel.open_login_picker(cx);
            let state = panel.login.as_ref().unwrap();
            assert!(state.error.as_ref().unwrap().contains("another machine"));
            assert!(state.status_task.is_none());
        });
        assert!(vcx.debug_bounds("login-provider-openai").is_none());
    }

    #[gpui::test]
    fn native_login_is_clickable_private_and_preserves_draft(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) =
            cx.add_window_view(|_, cx| Panel::new("login-test".into(), None, None, bridge, cx));
        panel.update(vcx, |panel, cx| {
            panel.input.update(cx, |input, cx| {
                input.set_content("keep my draft".into(), cx)
            });
        });
        assert!(vcx.debug_bounds("panel-login").is_some());
        // Workspace tests exercise the footer opening a separate adjacent panel.
        panel.update(vcx, |panel, cx| panel.open_login_picker(cx));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("login-dialog").is_some());
        for _ in 0..20 {
            let dialog = vcx.debug_bounds("login-dialog").unwrap();
            let provider = vcx.debug_bounds("login-provider-openai-api").unwrap();
            if provider.bottom() < dialog.bottom() - px(20.) {
                break;
            }
            vcx.simulate_event(gpui::ScrollWheelEvent {
                position: dialog.center(),
                delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-100.))),
                modifiers: gpui::Modifiers::default(),
                touch_phase: gpui::TouchPhase::Moved,
            });
            vcx.run_until_parked();
        }
        let button = vcx
            .debug_bounds("login-provider-openai-api")
            .expect("API key choice");
        vcx.simulate_click(button.center(), gpui::Modifiers::default());
        vcx.run_until_parked();
        assert!(
            vcx.debug_bounds("login-submit").is_some(),
            "provider={:?}, dialog={:?}, choice={button:?}",
            panel.read_with(vcx, |p, _| p.login.as_ref().unwrap().provider.map(|p| p.id)),
            vcx.debug_bounds("login-dialog")
        );
        vcx.simulate_input("typed-private-value");
        panel.read_with(vcx, |panel, cx| {
            assert!(!panel.login.as_ref().unwrap().input.read(cx).content_empty());
            assert_eq!(panel.input.read(cx).content.as_ref(), "keep my draft");
        });
        vcx.update(|_, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                "dummy-secret-not-for-chat".into(),
            ))
        });
        let paste = vcx.debug_bounds("login-paste").expect("mouse paste action");
        vcx.simulate_click(paste.center(), gpui::Modifiers::default());
        panel.read_with(vcx, |panel, cx| {
            let state = panel.login.as_ref().unwrap();
            assert!(!state.input.read(cx).content_empty());
            let snapshot = serde_json::to_string(&panel.snapshot(cx)).unwrap();
            assert!(!snapshot.contains("dummy-secret"));
            assert!(snapshot.contains("keep my draft"));
            assert!(panel.items.is_empty());
        });
        let close = vcx.debug_bounds("login-close").unwrap();
        vcx.simulate_click(close.center(), gpui::Modifiers::default());
        panel.read_with(vcx, |panel, cx| {
            assert!(panel.login.is_none());
            assert_eq!(panel.input.read(cx).content.as_ref(), "keep my draft");
        });
        assert!(
            commands.try_recv().is_err(),
            "login secrets must never be sent as messages"
        );
    }

    #[gpui::test]
    fn login_completion_refreshes_routes_and_offers_model_selection(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) =
            cx.add_window_view(|_, cx| Panel::new("login-test".into(), None, None, bridge, cx));
        panel.update(vcx, |panel, cx| {
            panel.open_login_picker(cx);
            panel.run_login_task(
                || {
                    Ok(LoginUpdate::Complete {
                        validation_warning: true,
                    })
                },
                cx,
            );
        });
        vcx.run_until_parked();
        assert!(
            matches!(commands.try_recv(), Ok(Command::RefreshRuntime { session_id }) if session_id == "login-test")
        );
        panel.read_with(vcx, |panel, _| {
            let state = panel.login.as_ref().unwrap();
            assert!(state.complete);
            assert!(
                state
                    .error
                    .as_deref()
                    .unwrap()
                    .contains("Credentials were saved")
            );
            assert!(panel.items.is_empty());
        });
        let choose = vcx
            .debug_bounds("login-choose-model")
            .expect("native post-login action");
        vcx.simulate_click(choose.center(), gpui::Modifiers::default());
        assert!(vcx.debug_bounds("recovery-model-picker").is_some());
        assert!(vcx.debug_bounds("login-dialog").is_none());
        assert!(
            commands.try_recv().is_err(),
            "login must not replay user prompts"
        );
    }

    #[gpui::test]
    fn accounts_login_switches_the_source_session_not_a_throwaway(cx: &mut gpui::TestAppContext) {
        let (bridge, commands) = crate::harness::spawn_recording();
        let (panel, vcx) =
            cx.add_window_view(|_, cx| Panel::new_accounts("chat-session", None, bridge, cx));
        panel.update(vcx, |panel, cx| {
            let state = panel.login.as_mut().unwrap();
            state.provider = state.client.resolve_provider("claude");
            panel.complete_login(false, cx);
        });
        assert!(matches!(
            commands.try_recv(),
            Ok(Command::AuthChanged { session_id, provider })
                if session_id == "chat-session" && provider == "claude"
        ));
        vcx.run_until_parked();
        assert!(vcx.debug_bounds("login-done").is_some());
        assert!(vcx.debug_bounds("login-choose-model").is_some());
        // Nothing live behind a draft, settings page or remote host.
        for source in ["startup://draft", "ssh://host/session", ""] {
            panel.update(vcx, |panel, _| {
                panel.session_id = format!("accounts://{source}");
                assert_eq!(panel.login_target_session(), None, "{source}");
            });
        }
    }

    #[gpui::test]
    fn login_slash_command_opens_ui_and_remote_login_never_touches_local_credentials(
        cx: &mut gpui::TestAppContext,
    ) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "ssh://test-host/session".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(vcx, |panel, cx| {
            assert!(panel.handle_slash_command("/login openai", cx));
            let state = panel.login.as_ref().unwrap();
            assert!(state.error.as_ref().unwrap().contains("another machine"));
            assert!(state.provider.is_none());
            assert!(state.flow.is_none());
            assert!(state.task.is_none());
            assert!(!panel.login_command("/login-custom", cx));
        });
        assert!(vcx.debug_bounds("login-dialog").is_some());
        assert!(vcx.debug_bounds("login-provider-openai").is_none());
    }

    #[gpui::test]
    fn login_unknown_provider_and_empty_key_have_native_recovery(cx: &mut gpui::TestAppContext) {
        let (panel, vcx) = cx.add_window_view(|_, cx| {
            Panel::new(
                "login-test".into(),
                None,
                None,
                crate::harness::spawn_inert(),
                cx,
            )
        });
        panel.update(vcx, |panel, cx| {
            assert!(panel.handle_slash_command("/login does-not-exist", cx));
            assert!(panel.login.as_ref().unwrap().error.is_some());
            assert!(panel.handle_slash_command("/login openai-api", cx));
            panel.submit_login(cx);
            assert_eq!(
                panel.login.as_ref().unwrap().error.as_deref(),
                Some("Paste an API key first.")
            );
            assert!(panel.login.as_ref().unwrap().task.is_none());
            assert!(panel.items.is_empty());
        });
        assert!(vcx.debug_bounds("login-paste").is_some());
    }
}

#[cfg(test)]
#[path = "panel_preview_login_tests.rs"]
mod preview_tests;

#[cfg(all(test, unix))]
#[path = "panel_login_flow_tests.rs"]
mod flow_tests;
