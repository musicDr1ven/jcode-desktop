use super::*;

#[gpui::test]
fn method_pill_selects_existing_provider_inline(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| crate::input::bind_keys(cx));
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        let route = |model: &str, method: &str| jcode_sdk::ModelRouteInfo {
            model: model.into(),
            provider: String::new(),
            api_method: method.into(),
            available: true,
            detail: String::new(),
            usage: None,
        };
        panel.apply(
            &ApiEvent::RuntimeInfo {
                session_id: "session-a".into(),
                provider: Some("openai".into()),
                model: Some("gpt-5.6-sol".into()),
                reasoning_effort: None,
                auth_method: Some("oauth".into()),
                account_label: None,
                routes: vec![
                    route("gpt-5.6-sol", "openai-oauth"),
                    route("claude-fable-5", "claude-oauth"),
                ],
            },
            cx,
        );
    });
    vcx.run_until_parked();
    let pill = vcx.debug_bounds("panel-login").unwrap();
    vcx.simulate_click(pill.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    let picker = vcx
        .debug_bounds("composer-provider-picker")
        .expect("inline picker");
    let input = vcx.debug_bounds("prompt-input").unwrap();
    assert!(picker.bottom() <= input.top());
    assert!(vcx.debug_bounds("accounts-panel").is_none());
    assert!(
        vcx.debug_bounds("login-close").is_none(),
        "no Accounts sign-in UI"
    );
    let row = vcx
        .debug_bounds("login-provider-claude")
        .expect("connected provider row is shown");
    let spec = panel.update(vcx, |panel, cx| {
        panel
            .input
            .read(cx)
            .provider_route_spec_for("neuralwatt", "Neuralwatt", Some("nw-model"))
    });
    eprintln!("DEBUG spec={spec:?}");
    vcx.simulate_click(row.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_millis(100)),
        Ok(Command::SetModel { session_id, model })
            if session_id == "session-a" && model == "claude-oauth:claude-fable-5"
    ));
    assert!(vcx.debug_bounds("composer-provider-picker").is_none());
    assert!(vcx.debug_bounds("login-busy").is_none());
}


fn fixture_provider(
    id: &str,
    display_name: &str,
    actions: Vec<jcode_desktop_harness::providers::ProviderAction>,
    removable: bool,
) -> jcode_desktop_harness::providers::ProviderEntry {
    jcode_desktop_harness::providers::ProviderEntry {
        id: id.into(),
        display_name: display_name.into(),
        auth_kind: "API key".into(),
        method: "API key".into(),
        status: "available".into(),
        credential_source: None,
        models: vec![jcode_desktop_harness::providers::ProviderModel {
            model: "nw-model".into(),
            provider: display_name.into(),
            api_method: format!("openai-compatible:{id}"),
            available: true,
        }],
        actions,
        removable,
    }
}

fn accounts_panel<'a>(
    cx: &'a mut gpui::TestAppContext,
    session_id: &str,
) -> (gpui::Entity<crate::panel::Panel>, &'a mut gpui::VisualTestContext) {
    let (bridge, _commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel(session_id, cx);
        workspace
    });
    let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| panel.open_login_picker(cx));
    // Let the offline account snapshot land before callers inject fixtures;
    // a later refresh would otherwise recompute and drop them.
    vcx.run_until_parked();
    (panel, vcx)
}

#[gpui::test]
fn providers_section_lists_runtime_only_providers(cx: &mut gpui::TestAppContext) {
    use jcode_desktop_harness::providers::ProviderAction;
    cx.update(|cx| crate::input::bind_keys(cx));
    let (panel, vcx) = accounts_panel(cx, "session-a");
    panel.update(vcx, |panel, cx| {
        panel.test_set_provider_entries(vec![fixture_provider(
            "neuralwatt",
            "Neuralwatt",
            vec![ProviderAction::Use, ProviderAction::SetApiKey, ProviderAction::Remove],
            true,
        )]);
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(
        vcx.debug_bounds("providers-section").is_some(),
        "providers section renders"
    );
    assert!(
        vcx.debug_bounds("provider-row-neuralwatt").is_some(),
        "named profile outside the sign-in catalog is listed"
    );
    assert!(vcx.debug_bounds("provider-use-neuralwatt").is_some());
}

#[gpui::test]
fn provider_dialogs_render_for_add_key_and_remove(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| crate::input::bind_keys(cx));
    let (panel, vcx) = accounts_panel(cx, "session-a");
    panel.update(vcx, |panel, cx| panel.test_open_add_provider(cx));
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("provider-add-dialog").is_some());
    panel.update(vcx, |panel, cx| {
        if let Some(state) = panel.login.as_mut() {
            state.add_provider = None;
        }
        panel.test_open_provider_key("neuralwatt", "Neuralwatt", cx);
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("provider-key-dialog").is_some());
}

#[gpui::test]
fn remote_runtime_renders_read_only_provider_guidance(cx: &mut gpui::TestAppContext) {
    use jcode_desktop_harness::providers::ProviderAction;
    cx.update(|cx| crate::input::bind_keys(cx));
    let (panel, vcx) = accounts_panel(cx, "ssh://remote.example/session-a");
    panel.update(vcx, |panel, cx| {
        panel.test_set_provider_entries(vec![fixture_provider(
            "neuralwatt",
            "Neuralwatt",
            vec![ProviderAction::Use],
            false,
        )]);
        cx.notify();
    });
    vcx.run_until_parked();
    assert!(vcx.debug_bounds("providers-remote-guidance").is_some());
    assert!(vcx.debug_bounds("providers-add").is_none());
}

#[gpui::test]
fn switcher_offers_provider_outside_the_sign_in_catalog(cx: &mut gpui::TestAppContext) {
    use jcode_desktop_harness::providers::ProviderAction;
    cx.update(|cx| crate::input::bind_keys(cx));
    let (bridge, commands) = crate::harness::spawn_recording();
    let (workspace, vcx) = cx.add_window_view(|_, cx| {
        let mut workspace =
            crate::workspace::Workspace::for_test(crate::learning::Coach::new(), cx);
        workspace.set_test_bridge(bridge);
        workspace.push_test_panel("session-a", cx);
        workspace
    });
    let panel = workspace.update(vcx, |workspace, _| workspace.test_panel(0).unwrap());
    panel.update(vcx, |panel, cx| {
        panel.apply(
            &ApiEvent::RuntimeInfo {
                session_id: "session-a".into(),
                provider: Some("neuralwatt".into()),
                model: Some("nw-model".into()),
                reasoning_effort: None,
                auth_method: Some("api key".into()),
                account_label: None,
                routes: vec![jcode_sdk::ModelRouteInfo {
                    model: "nw-model".into(),
                    provider: "Neuralwatt".into(),
                    api_method: "openai-compatible:neuralwatt".into(),
                    available: true,
                    detail: String::new(),
                    usage: None,
                }],
            },
            cx,
        );
    });
    vcx.run_until_parked();
    let pill = vcx.debug_bounds("panel-login").unwrap();
    vcx.simulate_click(pill.center(), gpui::Modifiers::default());
    vcx.run_until_parked();
    panel.update(vcx, |panel, cx| {
        panel.test_set_provider_entries(vec![fixture_provider(
            "neuralwatt",
            "Neuralwatt",
            vec![ProviderAction::Use],
            true,
        )]);
        cx.notify();
    });
    vcx.run_until_parked();
    let _row = vcx
        .debug_bounds("provider-entry-neuralwatt")
        .expect("runtime-only provider appears in the switcher");
    let spec = panel.read_with(vcx, |panel, cx| {
        panel
            .input
            .read(cx)
            .provider_route_spec_for("neuralwatt", "Neuralwatt", Some("nw-model"))
    });
    assert!(
        spec.as_deref().is_some_and(|spec| spec.contains("nw-model")),
        "runtime routing resolves a spec for the named profile: {spec:?}"
    );
    panel.update_in(vcx, |panel, window, cx| {
        panel.test_select_provider_entry("neuralwatt", window, cx);
    });
    vcx.run_until_parked();
    assert!(matches!(
        commands.recv_timeout(std::time::Duration::from_millis(100)),
        Ok(Command::SetModel { session_id, model })
            if session_id == "session-a" && model.contains("nw-model")
    ));
    assert!(
        vcx.debug_bounds("composer-provider-picker").is_none(),
        "selecting closes the switcher"
    );
}
