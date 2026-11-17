#[test]
fn interrupted_remote_session_waits_for_an_explicit_prompt_after_transport_replacement() {
    let ui_id = remote::namespace("desktop", "same-id");
    let (commands, rx) = channel();
    let (tx, updates) = update_channel();
    let (connections_tx, connections) = channel();
    let worker_id = ui_id.clone();
    let worker = std::thread::spawn(move || {
        session_worker_with_connector(worker_id, rx, tx, None, |address| {
            assert_eq!(address.host.as_deref(), Some("desktop"));
            let (client, server) = Server::new();
            connections_tx.send(server).unwrap();
            Ok(client)
        })
    });
    for attempt in 0..2 {
        let server = connections.recv_timeout(Duration::from_secs(4)).unwrap();
        assert!(
            matches!(server.next(), ApiRequest::AttachSession { session_id } if session_id == "same-id")
        );
        // The daemon can volunteer recovery history before attach's state
        // reply, and before Desktop asks for its displayed transcript.
        server.event(ApiEvent::SessionRecovery {
            session_id: "same-id".into(),
            continuation_message: format!("continue interrupted attempt {attempt}"),
            reconnect_notice: None,
        });
        assert!(matches!(server.next(), ApiRequest::GetHistory { .. }));
        assert!(matches!(server.next(), ApiRequest::GetRuntimeInfo { .. }));

        // Desktop must stay idle after an interruption, both on the first
        // attachment and after the transport is replaced. Drain a short window
        // so unrelated bootstrap requests do not hide a continuation send.
        let deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < deadline {
            match server.requests.try_recv() {
                Ok(request) => assert!(
                    !matches!(request, ApiRequest::SendMessage { .. }),
                    "recovery must not auto-continue after a transport replacement"
                ),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
        }

        if attempt == 0 {
            // No TurnDone: the old worker activity flag is still true here.
            server.disconnect();
            wait_update(&updates, |update| {
                matches!(update, Update::SessionLost { .. })
            });
        } else {
            commands.send(SessionCommand::Stop).unwrap();
            worker.join().unwrap();
            server.disconnect();
            return;
        }
    }
}
