fn recovery_event(session: &str) -> ApiEvent {
    ApiEvent::SessionRecovery {
        session_id: session.into(),
        continuation_message: "Continue interrupted work.".into(),
        reconnect_notice: None,
    }
}

fn recovery_fence(worker: &Worker) {
    worker.event(ApiEvent::RuntimeInfo {
        session_id: "s1".into(),
        provider: None,
        model: None,
        routes: vec![],
        reasoning_effort: None,
        auth_method: None,
        account_label: None,
    });
    worker.wait_for(|update| {
        matches!(
            update,
            Update::Event {
                event: ApiEvent::RuntimeInfo { .. },
                ..
            }
        )
    });
}

#[test]
fn interrupted_session_stays_idle_until_the_user_prompts() {
    let worker = Worker::new(|_| {
        vec![ApiEvent::MessageAccepted {
            session_id: "s1".into(),
        }]
    });
    worker.event(recovery_event("s1"));
    recovery_fence(&worker);
    assert!(
        worker.requests.try_recv().is_err(),
        "an interrupted restored session must not submit work on its own"
    );

    // Continuation is always an explicit act by the user.
    worker.send("please continue", vec![]);
    assert!(
        matches!(worker.request(), ApiRequest::SendMessage { session_id, content, .. }
        if session_id == "s1" && content == "please continue")
    );
}

#[test]
fn completed_session_and_other_sessions_recovery_remain_idle() {
    let worker = Worker::new(|_| panic!("idle session must not send"));
    worker.event(ApiEvent::SessionStatus {
        session_id: "s1".into(),
        status: "idle".into(),
        pending_soft_interrupts: None,
    });
    worker.event(recovery_event("other-session"));
    recovery_fence(&worker);
    assert!(worker.requests.try_recv().is_err());
}

#[test]
fn already_running_or_completed_turn_recovery_remains_idle() {
    for event in [
        ApiEvent::SessionStatus {
            session_id: "s1".into(),
            status: "running".into(),
            pending_soft_interrupts: None,
        },
        ApiEvent::TurnDone {
            session_id: "s1".into(),
            pending_soft_interrupts: None,
        },
        ApiEvent::SessionStatus {
            session_id: "s1".into(),
            status: "cancelled".into(),
            pending_soft_interrupts: None,
        },
    ] {
        let worker = Worker::new(|_| panic!("stale recovery must not send"));
        worker.event(event);
        worker.event(recovery_event("s1"));
        recovery_fence(&worker);
        assert!(worker.requests.try_recv().is_err());
    }
}

#[test]
fn interruption_directives_never_replay_as_steering() {
    let worker = Worker::new(|_| vec![busy_error()]);
    worker.event(recovery_event("s1"));
    recovery_fence(&worker);
    assert!(
        worker.requests.try_recv().is_err(),
        "recovery must not be replayed through steering either"
    );
}

#[test]
fn explicit_cancel_or_clear_keeps_a_late_recovery_idle() {
    for command in [
        SessionCommand::Cancel,
        SessionCommand::Operation(SessionOperation::Clear),
    ] {
        let worker = Worker::new(|_| panic!("explicit user stop must win"));
        worker.commands.send(command).unwrap();
        worker
            .commands
            .send(SessionCommand::RefreshRuntime)
            .unwrap();
        // The synchronous runtime reply fences the preceding user command.
        worker.wait_for(|update| {
            matches!(
                update,
                Update::Event {
                    event: ApiEvent::RuntimeInfo { .. },
                    ..
                }
            )
        });
        worker.event(recovery_event("s1"));
        recovery_fence(&worker);
        assert!(worker.requests.try_recv().is_err());
    }
}
