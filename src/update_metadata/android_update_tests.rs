use super::*;
use hbb_common::{
    base64::engine::general_purpose::STANDARD,
    sodiumoxide::{self, crypto::sign},
};
use serde_json::{json, Value};

const SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PREFIX: &str = "https://github.com/runseal-labs/sealdesk/releases/download/v1.4.6/";

fn apk(arch: &str) -> Value {
    let file_name = format!("sealdesk-1.4.6-{arch}.apk");
    json!({
        "platform": "android",
        "arch": arch,
        "format": "apk",
        "url": format!("{PREFIX}{file_name}"),
        "file_name": file_name,
        "size": 100,
        "sha256": SHA256,
    })
}

fn metadata(artifacts: Vec<Value>) -> Value {
    json!({
        "schema_version": 1,
        "app": "sealdesk",
        "package_id": "sealdesk",
        "version": "1.4.6",
        "release_id": "v1.4.6",
        "published_at": "2026-05-14T00:00:00Z",
        "signature_key_id": "sealdesk-2026-ed25519",
        "artifacts": artifacts,
    })
}

struct Signed {
    metadata: Vec<u8>,
    signature: Vec<u8>,
    public_key: [u8; 32],
}

fn sign(metadata: Value, key_id: &str) -> Signed {
    sodiumoxide::init().expect("sodiumoxide");
    let metadata = serde_json::to_vec(&metadata).expect("metadata");
    let (public_key, secret_key) = sign::gen_keypair();
    let mut signed = UPDATE_METADATA_SIGNATURE_CONTEXT.to_vec();
    signed.extend_from_slice(&metadata);
    let sig = sign::sign_detached(&signed, &secret_key);
    let signature = serde_json::to_vec(&json!({
        "schema_version": 1,
        "algorithm": "ed25519",
        "key_id": key_id,
        "signature": STANDARD.encode(sig.to_bytes()),
    }))
    .expect("signature");
    Signed {
        metadata,
        signature,
        public_key: public_key.0,
    }
}

fn query(arch: &str) -> UpdateArtifactQuery<'_> {
    UpdateArtifactQuery {
        platform: "android",
        arch,
        format: "apk",
    }
}

fn verify_signed(signed: &Signed, arch: &str) -> ResultType<VerifiedUpdateArtifact> {
    let trusted_keys = [TrustedUpdateKey {
        key_id: "sealdesk-2026-ed25519",
        algorithm: "ed25519",
        public_key: signed.public_key,
    }];
    verify_update_metadata_with_options(
        &signed.metadata,
        &signed.signature,
        VerificationOptions {
            requirements: MetadataRequirements {
                expected_version: "1.4.6",
                expected_release_id: "v1.4.6",
                artifact: query(arch),
            },
            expected_package_id: PUBLIC_UPDATE_PACKAGE_ID,
            artifact_url_policy: ArtifactUrlPolicy::ExactPrefix(PREFIX),
            trusted_keys: &trusted_keys,
        },
    )
}

#[test]
fn selects_the_android_abi_entry() {
    let signed = sign(
        metadata(vec![apk("aarch64"), apk("armv7"), apk("x86_64")]),
        "sealdesk-2026-ed25519",
    );
    for arch in ["aarch64", "armv7", "x86_64"] {
        let artifact = verify_signed(&signed, arch).expect(arch);
        assert_eq!(artifact.version, "1.4.6");
        assert_eq!(artifact.size, 100);
        assert_eq!(artifact.sha256, SHA256);
        assert!(artifact
            .url
            .ends_with(&format!("sealdesk-1.4.6-{arch}.apk")));
    }
    assert!(verify_signed(&signed, "x86").is_err());
}

#[test]
fn refuses_bad_signature_duplicate_or_mismatched_android_entry() {
    let artifacts = metadata(vec![apk("aarch64"), apk("armv7"), apk("x86_64")]);
    let mut tampered = sign(artifacts.clone(), "sealdesk-2026-ed25519");
    tampered.metadata.push(b' ');
    assert!(verify_signed(&tampered, "aarch64").is_err());

    let mut bad_sha = artifacts.clone();
    bad_sha["artifacts"][0]["sha256"] = json!("not-a-sha256");
    assert!(verify_signed(&sign(bad_sha, "sealdesk-2026-ed25519"), "aarch64").is_err());

    let mut bad_url = artifacts.clone();
    bad_url["artifacts"][0]["url"] = json!("https://example.com/sealdesk-1.4.6-aarch64.apk");
    assert!(verify_signed(&sign(bad_url, "sealdesk-2026-ed25519"), "aarch64").is_err());

    let mut duplicate = artifacts;
    duplicate["artifacts"]
        .as_array_mut()
        .expect("artifacts")
        .push(apk("aarch64"));
    match verify_signed(&sign(duplicate, "sealdesk-2026-ed25519"), "aarch64") {
        Err(error) => {
            assert_eq!(
                "multiple matching update artifacts found",
                error.to_string()
            )
        }
        Ok(_) => panic!("duplicate android entry was accepted"),
    }
}

#[test]
fn refuses_android_metadata_not_signed_by_the_builtin_key() {
    let signed = sign(metadata(vec![apk("aarch64")]), "sealdesk-2026-ed25519");
    match verify_update_metadata(
        &signed.metadata,
        &signed.signature,
        UpdateMetadataRequirements {
            expected_version: "1.4.6",
            expected_release_id: "v1.4.6",
            expected_artifact_url_prefix: PREFIX,
            artifact: query("aarch64"),
        },
    ) {
        Err(error) => assert_eq!("invalid update metadata signature", error.to_string()),
        Ok(_) => panic!("builtin key accepted a different signature"),
    }
}
