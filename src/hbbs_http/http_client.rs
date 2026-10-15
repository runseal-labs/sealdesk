use hbb_common::{bail, ResultType};
use reqwest::{blocking::Client as SyncClient, Client as AsyncClient};

// Plain HTTPS to the update and download hosts. No proxy: the app has no proxy setting.
macro_rules! configure_http_client {
    ($builder:expr, $Client: ty) => {{
        #[allow(unused_mut)]
        let mut builder = $builder.no_proxy();
        #[cfg(any(target_os = "android", target_os = "ios"))]
        match hbb_common::verifier::client_config(false) {
            Ok(client_config) => {
                builder = builder.use_preconfigured_tls(client_config);
            }
            Err(e) => {
                hbb_common::log::error!("Failed to get client config: {}", e);
            }
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            builder = builder.use_rustls_tls();
        }
        builder.build().unwrap_or_else(|e| {
            hbb_common::log::info!("Failed to create a client: {}", e);
            <$Client>::new()
        })
    }};
}

fn require_https(url: &str) -> ResultType<()> {
    let parsed_url = url::Url::parse(url)?;
    if parsed_url.scheme() != "https" {
        bail!("Strict HTTP client requires HTTPS: {}", url);
    }
    Ok(())
}

pub fn create_http_client_async() -> AsyncClient {
    configure_http_client!(AsyncClient::builder(), AsyncClient)
}

pub fn create_http_client_with_url_strict(url: &str) -> ResultType<SyncClient> {
    require_https(url)?;
    Ok(configure_http_client!(SyncClient::builder(), SyncClient))
}

pub async fn create_http_client_async_with_url_strict(url: &str) -> ResultType<AsyncClient> {
    require_https(url)?;
    Ok(configure_http_client!(AsyncClient::builder(), AsyncClient))
}
