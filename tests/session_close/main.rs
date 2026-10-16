#![cfg(feature = "flutter")]

fn main() {}

#[test]
fn late_reconnect_does_not_restart_closed_session() {
    let session: sealdesk::flutter::FlutterSession = Default::default();
    let pending_retry = session.clone();

    session.close();
    pending_retry.reconnect();

    assert!(
        session.thread.lock().unwrap().is_none(),
        "a retry arriving after user close must not start a connection thread"
    );
}
