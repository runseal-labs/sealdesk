//! The controlled side's way in. Upstream RustDesk registers with a rendezvous server here and
//! answers its punch-hole / relay requests; this build has no rendezvous or relay server at all.
//! Sessions arrive through Tailcat or the sharing-gated direct listener in `crate::lan`.

use std::sync::atomic::AtomicBool;

use hbb_common::{
    allow_err,
    config::{self, Config},
    log, sleep, tokio,
};

use crate::server::{check_zombie, new as new_server, ConnectionMeta, ServerPtr};

pub struct RendezvousMediator;

impl RendezvousMediator {
    pub async fn start_all() {
        if config::is_outgoing_only() {
            loop {
                sleep(1.).await;
            }
        }
        #[cfg(target_os = "windows")]
        if crate::platform::is_installed() && crate::is_server() {
            crate::updater::start_auto_update();
        }
        check_zombie();
        let server = new_server();
        tokio::spawn(crate::lan::serve(server.clone()));
        scrap::codec::test_av1();
        tailcat_server(server).await;
    }
}

/// Accepts RustDesk sessions arriving through the tailcat tunnel. The tunnel delivers them to a
/// loopback-only listener. The separate sharing-gated LAN listener is managed by `crate::lan`.
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
                crate::tailcat::host::clear_init_error();
                if !crate::tailcat::host::enabled() {
                    return;
                }
                crate::tailcat::host::set_address(addr.clone());
                log::info!("Tailcat sharing ready");
                break;
            }
            Ok(Err(err)) => {
                let err = crate::tailcat::host::set_init_error("serve", &err.to_string());
                log::error!("tailcat serve failed, retrying: {err}");
            }
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
