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
        // Keep the failure visible until the user stops sharing to retry.
        while crate::tailcat::host::enabled() && !crate::tailcat::host::start_error().is_empty() {
            sleep(0.1).await;
        }
    }
}

async fn accept_tailcat(server: ServerPtr) {
    if !crate::tailcat::host::enabled() {
        return;
    }
    let listener = match hbb_common::tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(l) => l,
        Err(err) => {
            log::error!("tailcat: loopback listen failed: {err}");
            crate::tailcat::host::set_start_error(
                "Local listener: unable to bind loopback port".to_owned(),
            );
            return;
        }
    };
    let port = match listener.local_addr() {
        Ok(a) => a.port(),
        Err(err) => {
            log::error!("tailcat: {err}");
            crate::tailcat::host::set_start_error(
                "Local listener: unable to read loopback address".to_owned(),
            );
            return;
        }
    };
    let local_token = crate::tailcat::transport::local_token();
    if !crate::tailcat::host::enabled() {
        return;
    }
    match tokio::task::spawn_blocking(move || crate::tailcat::serve(port, &local_token)).await {
        Ok(Ok(addr)) => {
            if !crate::tailcat::host::enabled() {
                return;
            }
            crate::tailcat::host::set_address(addr.clone());
            log::info!("Tailcat sharing ready");
        }
        Ok(Err(err)) => {
            crate::tailcat::host::set_start_error(err.to_string());
            log::error!("Tailcat initialization failed; waiting for retry");
            return;
        }
        Err(_) => {
            crate::tailcat::host::set_start_error(
                "Initialization: transport task failed".to_owned(),
            );
            return;
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
