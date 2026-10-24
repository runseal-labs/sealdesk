use crate::{
    common::{display_version_from_release_id, release_id_from_update_url},
    hbbs_http::create_http_client_with_url_strict,
    update_metadata::{UpdateArtifactQuery, UpdateMetadataRequirements, VerifiedUpdateArtifact},
};
use hbb_common::{bail, ResultType};
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

const UPDATE_SIDECAR_HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const UPDATE_METADATA_SIDECAR_MAX_BYTES: u64 = 1024 * 1024;

lazy_static::lazy_static! {
    static ref VERIFIED_UPDATE_ARTIFACT: Mutex<Option<VerifiedUpdateArtifact>> = Mutex::new(None);
}

pub(crate) fn current_update_platform() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::consts::OS
    }
}

pub(crate) fn current_update_arch() -> &'static str {
    #[cfg(all(target_os = "windows", not(feature = "flutter")))]
    {
        "x86"
    }
    #[cfg(not(all(target_os = "windows", not(feature = "flutter"))))]
    {
        std::env::consts::ARCH
    }
}

pub(crate) fn current_update_format(update_msi: bool) -> &'static str {
    #[cfg(any(
        not(target_os = "windows"),
        all(target_os = "windows", not(feature = "flutter"))
    ))]
    let _ = update_msi;
    #[cfg(all(target_os = "windows", feature = "flutter"))]
    {
        if update_msi {
            return "msi";
        }
        "exe"
    }
    #[cfg(all(target_os = "windows", not(feature = "flutter")))]
    {
        "exe"
    }
    #[cfg(target_os = "macos")]
    {
        "dmg"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "unknown"
    }
}

pub fn current_update_artifact_query(update_msi: bool) -> UpdateArtifactQuery<'static> {
    UpdateArtifactQuery {
        platform: current_update_platform(),
        arch: current_update_arch(),
        format: current_update_format(update_msi),
    }
}

/// Rust `std::env::consts::ARCH` on 32-bit ARM is `arm`. The signed manifest uses `armv7`.
pub(crate) fn android_update_arch(rust_arch: &str) -> Option<&'static str> {
    match rust_arch {
        "aarch64" => Some("aarch64"),
        "arm" => Some("armv7"),
        "x86_64" => Some("x86_64"),
        _ => None,
    }
}

pub(crate) fn android_update_query(rust_arch: &str) -> ResultType<UpdateArtifactQuery<'static>> {
    let Some(arch) = android_update_arch(rust_arch) else {
        bail!("unsupported Android ABI: {}", rust_arch);
    };
    Ok(UpdateArtifactQuery {
        platform: "android",
        arch,
        format: "apk",
    })
}

pub(crate) fn android_update_apk_path(cache_dir: &Path, download_url: &str) -> ResultType<PathBuf> {
    if !cache_dir.is_absolute() {
        bail!("Android update directory must be absolute");
    }
    let Some(parsed) = get_update_download_file_from_url(download_url) else {
        bail!("Failed to get the file path from the URL: {}", download_url);
    };
    let Some(file_name) = parsed.file_name().and_then(|name| name.to_str()) else {
        bail!("Update artifact has no file name");
    };
    if !file_name.ends_with(".apk") {
        bail!("update artifact is not an apk");
    }
    let updates_dir = cache_dir.join("updates");
    std::fs::create_dir_all(&updates_dir)?;
    let file_path = updates_dir.join(file_name);
    if file_path.parent() != Some(updates_dir.as_path()) {
        bail!("update artifact path escaped the download directory");
    }
    Ok(file_path)
}

pub fn verified_update_artifact_for_release_page_url(
    release_page_url: &str,
    query: UpdateArtifactQuery<'_>,
) -> ResultType<VerifiedUpdateArtifact> {
    let artifact = verified_update_artifact_from_release_page_url(release_page_url, &query)?;
    *VERIFIED_UPDATE_ARTIFACT.lock().unwrap() = Some(artifact.clone());
    Ok(artifact)
}

pub fn verified_update_artifact_for_download_url(
    download_url: &str,
) -> ResultType<VerifiedUpdateArtifact> {
    VERIFIED_UPDATE_ARTIFACT
        .lock()
        .unwrap()
        .as_ref()
        .filter(|artifact| artifact.url == download_url)
        .cloned()
        .ok_or_else(|| hbb_common::anyhow::anyhow!("update artifact was not verified"))
}

pub(super) fn verified_update_artifact_from_release_page_url(
    update_url: &str,
    query: &UpdateArtifactQuery<'_>,
) -> ResultType<VerifiedUpdateArtifact> {
    let release_id = release_id_from_update_url(update_url)?;
    let display_version = display_version_from_release_id(&release_id)?;
    let expected_artifact_url_prefix =
        format!("https://github.com/runseal-labs/sealdesk/releases/download/{release_id}/");
    let metadata_url = format!("{expected_artifact_url_prefix}sealdesk-update.json");
    let signature_url = format!("{metadata_url}.sig");
    let metadata_bytes = fetch_update_sidecar_bytes(&metadata_url)?;
    let signature_bytes = fetch_update_sidecar_bytes(&signature_url)?;
    let requirements = UpdateMetadataRequirements {
        expected_version: display_version.as_str(),
        expected_release_id: release_id.as_str(),
        expected_artifact_url_prefix: expected_artifact_url_prefix.as_str(),
        artifact: *query,
    };
    crate::update_metadata::verify_update_metadata(&metadata_bytes, &signature_bytes, requirements)
}

fn fetch_update_sidecar_bytes(url: &str) -> ResultType<Vec<u8>> {
    let client = create_http_client_with_url_strict(url)?;
    let mut response = client
        .get(url)
        .timeout(UPDATE_SIDECAR_HTTP_REQUEST_TIMEOUT)
        .send()?;
    if !response.status().is_success() {
        bail!(
            "Failed to download update metadata sidecar: {}",
            response.status()
        );
    }
    read_limited_response_bytes(
        &mut response,
        UPDATE_METADATA_SIDECAR_MAX_BYTES,
        "Update metadata sidecar",
    )
}

fn read_limited_response_bytes<R: Read>(
    reader: &mut R,
    limit: u64,
    what: &str,
) -> ResultType<Vec<u8>> {
    let mut limited_reader = reader.take(limit.saturating_add(1));
    let mut bytes = Vec::new();
    limited_reader.read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("{what} exceeds maximum allowed size of {limit} bytes");
    }
    Ok(bytes)
}

pub fn get_update_download_file_from_url(url: &str) -> Option<PathBuf> {
    let parsed = url::Url::parse(url).ok()?;
    // Check the raw prefix before Url normalizes default ports.
    if !url.starts_with("https://github.com/")
        || parsed.scheme() != "https"
        || parsed.host_str() != Some("github.com")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }

    let mut segments = parsed.path_segments()?;
    let owner = segments.next()?;
    let repo = segments.next()?;
    let releases = segments.next()?;
    let download = segments.next()?;
    let tag = segments.next()?;
    let filename = segments.next()?;

    if owner != "runseal-labs"
        || repo != "sealdesk"
        || releases != "releases"
        || download != "download"
        || tag.is_empty()
        || segments.next().is_some()
        || !is_plain_update_filename(filename)
    {
        return None;
    }

    Some(std::env::temp_dir().join(filename))
}

fn is_plain_update_filename(filename: &str) -> bool {
    if filename.is_empty()
        || filename.contains('/')
        || filename.contains('\\')
        || filename.contains(':')
    {
        return false;
    }

    let mut components = Path::new(filename).components();
    matches!(
        components.next(),
        Some(Component::Normal(name)) if name.to_str() == Some(filename)
    ) && components.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_download_file_accepts_expected_github_asset_urls() {
        let file = get_update_download_file_from_url(
            "https://github.com/runseal-labs/sealdesk/releases/download/1.4.0/rustdesk-1.4.0-x86_64.dmg",
        )
        .expect("valid GitHub release asset URL");

        assert_eq!(
            file.file_name().and_then(|name| name.to_str()),
            Some("rustdesk-1.4.0-x86_64.dmg")
        );
    }

    #[test]
    fn update_download_file_rejects_untrusted_or_malformed_urls() {
        for url in [
            "https://github.com/rustdesk/rustdesk/releases/download/1/rustdesk.exe",
            "http://github.com/runseal-labs/sealdesk/releases/download/1/rustdesk.exe",
            "https://example.com/rustdesk.exe",
            "https://github.com/other/project/releases/download/1/rustdesk.exe",
            "https://github.com/runseal-labs/sealdesk/releases/download/1/",
            "https://github.com/runseal-labs/sealdesk/releases/download/1/nested/rustdesk.exe",
            "https://github.com/runseal-labs/sealdesk/releases/download/1/C:rustdesk.exe",
            "https://user@github.com/runseal-labs/sealdesk/releases/download/1/rustdesk.exe",
            "https://github.com:443/runseal-labs/sealdesk/releases/download/1/rustdesk.exe",
            "https://github.com/runseal-labs/sealdesk/releases/download/1/rustdesk.exe?download=1",
            "https://github.com/runseal-labs/sealdesk/releases/download/1/rustdesk.exe#download",
            "not a url",
        ] {
            assert!(get_update_download_file_from_url(url).is_none(), "{url}");
        }
    }

    #[test]
    fn android_update_arch_maps_device_abi_and_rejects_unknown() {
        assert_eq!(android_update_arch("aarch64"), Some("aarch64"));
        assert_eq!(android_update_arch("arm"), Some("armv7"));
        assert_eq!(android_update_arch("x86_64"), Some("x86_64"));
        assert_eq!(android_update_arch("x86"), None);
        assert!(android_update_query("x86").is_err());
        let query = android_update_query("arm").expect("armv7");
        assert_eq!(
            (query.platform, query.arch, query.format),
            ("android", "armv7", "apk")
        );
    }

    #[test]
    fn android_update_apk_path_stays_inside_the_cache_updates_dir() {
        let cache_dir = std::env::temp_dir().join(format!(
            "sealdesk-android-update-path-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&cache_dir);
        std::fs::create_dir_all(&cache_dir).unwrap();
        let path = android_update_apk_path(
            &cache_dir,
            "https://github.com/runseal-labs/sealdesk/releases/download/v1.4.6/sealdesk-1.4.6-aarch64.apk",
        )
        .expect("apk path");
        assert_eq!(
            path,
            cache_dir.join("updates").join("sealdesk-1.4.6-aarch64.apk")
        );
        assert!(android_update_apk_path(
            &cache_dir,
            "https://github.com/runseal-labs/sealdesk/releases/download/v1.4.6/sealdesk-1.4.6-x86_64.exe",
        )
        .is_err());
        assert!(android_update_apk_path(
            Path::new("relative"),
            "https://github.com/runseal-labs/sealdesk/releases/download/v1.4.6/sealdesk-1.4.6-aarch64.apk",
        )
        .is_err());
        std::fs::remove_dir_all(&cache_dir).unwrap();
    }
}
