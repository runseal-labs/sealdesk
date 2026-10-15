use super::*;
use crate::client::{Client, Interface};
use base::{
    config::keys,
    message_proto::{login_response, message, Message},
};
use hbb_common::{
    config::Config,
    protobuf::Message as ProtoMessage,
    rendezvous_proto::ConnType,
    tokio::{
        self,
        time::{timeout, Duration, Instant},
    },
};

#[test]
#[ignore]
fn live_controller_does_not_start_host() {
    assert!(std::env::var("SEALDESK_CONTROLLER_PROBE").is_ok());
    *hbb_common::config::APP_NAME.write().unwrap() =
        format!("SealDeskCliProbe-{}", std::process::id());
    assert!(!std::path::Path::new(&Config::ipc_path("")).exists());
    assert!(!crate::common::is_server_running());
    let no_server = super::launch::controller_only("", false);
    host::request_mode("control").unwrap();
    assert!(hbb_common::config::LocalConfig::get_option(keys::OPTION_SEALDESK_MODE).is_empty());
    crate::start_server(false, no_server);
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert!(!crate::common::is_server_running());
    assert!(!std::path::Path::new(&Config::ipc_path("")).exists());
    assert!(!std::path::Path::new(&server_key_path()).exists());
    println!("LIVE_CONTROLLER_ONLY_OK no host IPC, listener, or tailcat server key created");
}

#[test]
#[ignore]
#[cfg(target_os = "linux")]
fn live_host_role_switch() {
    use hbb_common::config::LocalConfig;
    use std::io::{Read, Write};
    use std::{
        collections::HashSet,
        time::{Duration, Instant},
    };

    assert!(std::env::var("SEALDESK_CONTROLLER_PROBE").is_ok());
    *hbb_common::config::APP_NAME.write().unwrap() =
        format!("SealDeskRoleProbe-{}", std::process::id());
    let listening_ports = || -> Vec<u16> {
        let inodes: HashSet<String> = std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(|entry| std::fs::read_link(entry.ok()?.path()).ok())
            .filter_map(|link| {
                link.to_str()?
                    .strip_prefix("socket:[")?
                    .strip_suffix(']')
                    .map(str::to_owned)
            })
            .collect();
        std::fs::read_to_string("/proc/net/tcp")
            .unwrap()
            .lines()
            .skip(1)
            .filter_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                if fields[3] != "0A" || !inodes.contains(fields[9]) {
                    return None;
                }
                let (address, port) = fields[1].split_once(':').unwrap();
                assert_eq!(address, "0100007F");
                Some(u16::from_str_radix(port, 16).unwrap())
            })
            .collect()
    };
    let select = |mode: &str| {
        crate::ui_interface::set_local_option(
            keys::OPTION_SEALDESK_MODE.to_owned(),
            mode.to_owned(),
        );
        assert_eq!(LocalConfig::get_option(keys::OPTION_SEALDESK_MODE), mode);
        assert_eq!(host::query_mode().unwrap(), mode);
    };
    assert!(LocalConfig::get_option(keys::OPTION_SEALDESK_MODE).is_empty());
    assert!(super::launch::controller_only("", false));
    select("control");
    assert!(!std::path::Path::new(&Config::ipc_path("")).exists());
    assert!(!std::path::Path::new(&server_key_path()).exists());
    assert!(listening_ports().is_empty());

    let mut previous_key = None;
    let mut previous_address = None;
    for round in 0..3 {
        Config::set_option(keys::OPTION_STOP_SERVICE.to_owned(), "Y".to_owned());
        select("host");
        let deadline = Instant::now() + Duration::from_secs(30);
        while !is_addr(&Config::get_option(OPTION_ADDR)) {
            assert!(Instant::now() < deadline, "host did not publish an address");
            std::thread::sleep(Duration::from_millis(100));
        }
        let address = Config::get_option(OPTION_ADDR);
        if let Some(previous_address) = &previous_address {
            assert_eq!(previous_address, &address);
        }
        previous_address = Some(address.clone());
        let key = std::fs::read(server_key_path()).unwrap();
        if let Some(previous_key) = &previous_key {
            assert_eq!(previous_key, &key);
        }
        previous_key = Some(key);
        let ports = listening_ports();
        assert_eq!(ports.len(), 1);
        let token = super::transport::local_token();
        let started = Instant::now();
        let forward = super::forward(&address, 10_000, &token).unwrap();
        assert!(started.elapsed() < Duration::from_secs(10));
        println!(
            "LIVE_ROLE_RECONNECT_OK round={} elapsed_ms={}",
            round,
            started.elapsed().as_millis()
        );
        let mut tunnel = std::net::TcpStream::connect(forward.0.to_str().unwrap()).unwrap();
        tunnel
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        tunnel.write_all(&token).unwrap();
        let mut ack = [0u8; 1];
        tunnel.read_exact(&mut ack).unwrap();
        assert_eq!(ack, [1]);
        let mut message = [0u8; 65536];
        assert!(tunnel.read(&mut message).unwrap() > 0);
        let stale_options = Config::get_options();
        let stale_config = (Config::get(), hbb_common::config::Config2::get());
        select("control");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match tunnel.read(&mut message) {
                Ok(0) => break,
                Ok(_) => assert!(Instant::now() < deadline, "incoming tunnel stayed open"),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                    ) =>
                {
                    break
                }
                Err(error) => panic!("incoming tunnel did not close: {error}"),
            }
        }
        drop(tunnel);
        drop(forward);
        crate::ipc::set_options(stale_options).unwrap();
        crate::ipc::set_data(&crate::ipc::Data::SyncConfig(Some(stale_config.into()))).unwrap();
        assert_eq!(host::query_mode().unwrap(), "control");
        assert!(Config::get_option(OPTION_ADDR).is_empty());
        assert!(listening_ports().is_empty());
        assert_eq!(
            std::fs::read(server_key_path()).unwrap(),
            previous_key.clone().unwrap()
        );
        println!(
            "LIVE_ROLE_SWITCH_OK host -> control closed loopback ingress; address_len={}",
            address.len()
        );
    }
    crate::ui_interface::set_local_option(
        keys::OPTION_SEALDESK_MODE.to_owned(),
        "invalid".to_owned(),
    );
    assert_eq!(
        LocalConfig::get_option(keys::OPTION_SEALDESK_MODE),
        "control"
    );
    println!(
        "LIVE_ROLE_LIFECYCLE_OK initial control, host start, stop, and restart keep the same key"
    );
}

#[test]
#[ignore]
fn live_reset_address() {
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    assert!(std::env::var("SEALDESK_CONTROLLER_PROBE").is_ok());
    *hbb_common::config::APP_NAME.write().unwrap() =
        format!("SealDeskResetProbe-{}", std::process::id());
    assert!(host::reset_address().is_err());
    host::request_mode("host").unwrap();
    let address = || {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let address = Config::get_option(OPTION_ADDR);
            if is_addr(&address) {
                return address;
            }
            assert!(Instant::now() < deadline, "host did not publish an address");
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    let original_address = address();
    let original_key = std::fs::read(server_key_path()).unwrap();
    assert!(crate::ipc::set_config(keys::OPTION_SEALDESK_RESET_ADDRESS, "Y".to_owned()).is_err());
    assert_eq!(std::fs::read(server_key_path()).unwrap(), original_key);
    let token = transport::local_token();
    let old_forward = forward(&original_address, 10_000, &token).unwrap();
    let client_key = std::fs::read(client_key_path()).unwrap();
    let mut tunnel = std::net::TcpStream::connect(old_forward.0.to_str().unwrap()).unwrap();
    tunnel
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    tunnel.write_all(&token).unwrap();
    let mut ack = [0u8; 1];
    tunnel.read_exact(&mut ack).unwrap();
    assert_eq!(ack, [1]);
    let mut message = [0u8; 65536];
    assert!(tunnel.read(&mut message).unwrap() > 0);
    host::reset_address().unwrap();
    let new_address = address();
    assert_ne!(original_address, new_address);
    assert_ne!(std::fs::read(server_key_path()).unwrap(), original_key);
    assert_eq!(std::fs::read(client_key_path()).unwrap(), client_key);
    loop {
        match tunnel.read(&mut message) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                ) =>
            {
                break
            }
            Err(error) => panic!("old session did not close: {error}"),
        }
    }
    drop(tunnel);
    drop(old_forward);
    assert!(forward(&original_address, 2_000, &token).is_err());
    let new_forward = forward(&new_address, 10_000, &token).unwrap();
    drop(new_forward);
    println!("LIVE_RESET_ADDRESS_OK old address rejected, old tunnel closed, new address connects, controller key preserved");
    Config::set_option(keys::OPTION_STOP_SERVICE.to_owned(), "Y".to_owned());
    host::reset_address().unwrap();
    assert_eq!(host::query_mode().unwrap(), "control");
    assert_eq!(Config::get_option(keys::OPTION_STOP_SERVICE), "Y");
    println!("LIVE_RESET_STOPPED_OK reset does not restart a disabled host");
    host::reset_address().unwrap();
    assert_eq!(host::query_mode().unwrap(), "control");
    assert!(!std::path::Path::new(&server_key_path()).exists());
    println!("LIVE_RESET_CONTROL_OK reset keeps controller mode stopped");
}

#[test]
#[ignore]
fn live_pairing_password_policies() {
    assert!(std::env::var("SEALDESK_LIVE_CODE_FILE").is_ok());
    *hbb_common::config::APP_NAME.write().unwrap() = APP_NAME.to_owned();
    let original = crate::ipc::get_options();
    assert!(original.contains_key(OPTION_ADDR));
    for (approval, verification, includes_password) in [
        ("password", "use-both-passwords", true),
        ("click", "use-both-passwords", false),
        ("password", "use-permanent-password", false),
    ] {
        let mut options = original.clone();
        options.insert(keys::OPTION_APPROVE_MODE.to_owned(), approval.to_owned());
        options.insert(
            keys::OPTION_VERIFICATION_METHOD.to_owned(),
            verification.to_owned(),
        );
        crate::ipc::set_options(options).unwrap();
        let pairing = pairing::PairingCode::parse(&current_pairing_code(600).unwrap()).unwrap();
        assert_eq!(pairing.password.is_some(), includes_password);
        if includes_password {
            assert_eq!(
                pairing.password,
                crate::ipc::get_config("temporary-password").unwrap()
            );
        }
    }
    crate::ipc::set_options(original).unwrap();
    println!("LIVE_PAIRING_POLICY_OK click-only and permanent-only omit password");
}

#[tokio::test]
#[ignore]
async fn live_tunnel_password_login() {
    *hbb_common::config::APP_NAME.write().unwrap() = APP_NAME.to_owned();
    let code_path = std::env::var("SEALDESK_LIVE_CODE_FILE").unwrap();
    let code = std::fs::read_to_string(code_path).unwrap();
    let pairing = pairing::PairingCode::parse(code.lines().last().unwrap()).unwrap();
    let expect_rejection = std::env::var("SEALDESK_LIVE_REJECT").is_ok();
    let password = if expect_rejection {
        "deliberately-wrong-password".to_owned()
    } else {
        pairing.password.clone().unwrap()
    };
    let handler = crate::ui::remote::SciterSession::new(
        "--connect".to_owned(),
        pairing.address.clone(),
        password.clone(),
        Vec::new(),
    )
    .inner();
    let started = Instant::now();
    let ((mut peer, _, _, _, _), _) = Client::start(
        &pairing.address,
        "",
        "",
        ConnType::DEFAULT_CONN,
        handler.clone(),
    )
    .await
    .unwrap();
    assert!(peer.is_secured() || is_addr(&handler.get_id()));
    let mut sent_login = false;
    loop {
        let frame = timeout(Duration::from_secs(20), peer.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let message = Message::parse_from_bytes(&frame).unwrap();
        match message.union {
            Some(message::Union::Hash(hash)) => {
                assert!(handler.handle_hash(&password, hash, &mut peer).await);
                sent_login = true;
            }
            Some(message::Union::LoginResponse(response)) => {
                assert!(sent_login);
                match response.union {
                    Some(login_response::Union::PeerInfo(_)) => {
                        assert!(!expect_rejection);
                        assert!(started.elapsed() < Duration::from_secs(10));
                        let elapsed_ms = started.elapsed().as_millis();
                        let (sender, receiver) = tokio::sync::oneshot::channel();
                        let mut sender = Some(sender);
                        let monitor = watch_path(&pairing.address, move |direct| {
                            if let Some(sender) = sender.take() {
                                let _ = sender.send(direct);
                            }
                        });
                        let direct = timeout(Duration::from_secs(10), async {
                            tokio::select! {
                                result = receiver => result.unwrap(),
                                _ = monitor => panic!("path monitor stopped before an update"),
                            }
                        })
                        .await
                        .unwrap();
                        if std::env::var("TS_DEBUG_ALWAYS_USE_DERP").as_deref() == Ok("true") {
                            assert!(!direct);
                        }
                        let route = if direct { "p2p" } else { "derp" };
                        println!("LIVE_LOGIN_OK secure=true route={route} elapsed_ms={elapsed_ms}");
                    }
                    Some(login_response::Union::Error(error)) => {
                        assert!(expect_rejection, "login rejected: {error}");
                        println!("LIVE_LOGIN_REJECTED {error}");
                    }
                    _ => panic!("missing login result"),
                }
                break;
            }
            _ => {}
        }
    }
    assert!(Config::get_option(keys::OPTION_CUSTOM_RENDEZVOUS_SERVER).is_empty());
}
