//! The controlled side's way in. Upstream RustDesk registers with a rendezvous server here and
//! answers its punch-hole / relay requests; this build has no rendezvous or relay server at all.
//! Sessions arrive only through the tailcat tunnel (see `crate::tailcat`).

use std::sync::atomic::AtomicBool;

use hbb_common::{
    allow_err,
    config::{self, Config},
    log, sleep, tokio,
};

use crate::server::{check_zombie, new as new_server, ConnectionMeta, ServerPtr};

/// Kept for the IPC / UI callers of the upstream deploy flow; nothing sets it without a
/// rendezvous server.
pub(crate) static NEEDS_DEPLOY: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "android")]
pub(crate) fn reset_needs_deploy_notification() {
    NEEDS_DEPLOY.store(false, std::sync::atomic::Ordering::SeqCst);
}

pub struct RendezvousMediator;

impl RendezvousMediator {
    /// Upstream re-registers with the rendezvous servers. The tunnel does not depend on any
    /// option that can change at runtime, so there is nothing to redo.
    pub fn restart() {
        log::info!("server restart (no-op without rendezvous server)");
    }

    pub async fn start_all() {
        if config::is_outgoing_only() {
            loop {
                sleep(1.).await;
            }
        }
        crate::hbbs_http::sync::start();
        #[cfg(target_os = "windows")]
        if crate::platform::is_installed() && crate::is_server() {
            crate::updater::start_auto_update();
        }
        check_zombie();
        let server = new_server();
        #[cfg(target_os = "android")]
        let start_lan_listening = true;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let start_lan_listening = crate::platform::is_installed();
        if start_lan_listening {
            std::thread::spawn(move || {
                allow_err!(super::lan::start_listening());
            });
        }
        scrap::codec::test_av1();
        tailcat_server(server).await;
    }
}

/// Accepts RustDesk sessions arriving through the tailcat tunnel. The tunnel delivers them to a
/// loopback-only listener, so nothing is exposed on the LAN or the internet.
async fn tailcat_server(server: ServerPtr) {
    crate::tailcat::host::set_address(String::new());
    loop {
        while !crate::tailcat::host::enabled() {
            sleep(0.1).await;
        }
        if !crate::tailcat::host::begin_start() {
            continue;
        }
        accept_tailcat(server.clone()).await;
        crate::tailcat::host::set_address(String::new());
        loop {
            match tokio::task::spawn_blocking(crate::tailcat::stop_serving).await {
                Ok(Ok(())) => break,
                Ok(Err(error)) => log::error!("tailcat stop failed: {error}"),
                Err(error) => log::error!("tailcat stop task failed: {error}"),
            }
            sleep(1.).await;
        }
        crate::tailcat::host::set_running(false);
    }
}

async fn accept_tailcat(server: ServerPtr) {
    let listener = loop {
        if !crate::tailcat::host::enabled() {
            return;
        }
        match hbb_common::tokio::net::TcpListener::bind("127.0.0.1:0").await {
            Ok(l) => break l,
            Err(err) => {
                log::error!("tailcat: loopback listen failed: {err}");
                sleep(1.).await;
            }
        }
    };
    let port = match listener.local_addr() {
        Ok(a) => a.port(),
        Err(err) => {
            log::error!("tailcat: {err}");
            return;
        }
    };
    let local_token = crate::tailcat::transport::local_token();
    loop {
        if !crate::tailcat::host::enabled() {
            return;
        }
        match tokio::task::spawn_blocking(move || crate::tailcat::serve(port, &local_token)).await {
            Ok(Ok(addr)) => {
                if !crate::tailcat::host::enabled() {
                    return;
                }
                crate::tailcat::host::set_address(addr.clone());
                log::info!("tailcat address: {addr}");
                if let Ok(qr) = crate::tailcat::qr_text(&addr) {
                    log::info!("scan to connect:\n{qr}");
                }
                break;
            }
            Ok(Err(err)) => log::error!("tailcat serve failed, retrying: {err}"),
            Err(err) => log::error!("tailcat serve task: {err}"),
        }
        for _ in 0..50 {
            if !crate::tailcat::host::enabled() {
                return;
            }
            sleep(0.1).await;
        }
    }
    let handshakes = std::sync::Arc::new(tokio::sync::Semaphore::new(64));
    loop {
        if !crate::tailcat::host::enabled() {
            return;
        }
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            _ = sleep(0.1) => continue,
        };
        let (mut stream, _) = match accepted {
            Ok(x) => x,
            Err(err) => {
                log::error!("tailcat accept: {err}");
                sleep(0.1).await;
                continue;
            }
        };
        let Ok(permit) = handshakes.clone().try_acquire_owned() else {
            continue;
        };
        let local_addr = stream
            .local_addr()
            .unwrap_or(Config::get_any_listen_addr(true));
        let server = server.clone();
        tokio::spawn(async move {
            let addr = match crate::tailcat::transport::authenticate_local(&mut stream, &local_token)
                .await
            {
                Ok(addr) => addr,
                Err(_) => return,
            };
            if !crate::tailcat::host::enabled() {
                return;
            }
            drop(permit);
            if let Err(err) = stream.set_nodelay(true) {
                log::trace!("tailcat local socket: {err}");
                return;
            }
            allow_err!(
                crate::server::create_tcp_connection(
                    server,
                    hbb_common::Stream::from(stream, local_addr),
                    addr,
                    false,
                    ConnectionMeta {
                        via_tailcat: true,
                        ..Default::default()
                    },
                )
                .await
            );
        });
    }
}
