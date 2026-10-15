use hbb_common::{
    bail,
    sodiumoxide::{randombytes, utils::memcmp},
    tokio::{
        io::{AsyncRead, AsyncReadExt},
        time::{timeout, Duration},
    },
    ResultType,
};
use std::net::{Ipv6Addr, SocketAddr};

pub fn local_token() -> [u8; 32] {
    let mut token = [0; 32];
    randombytes::randombytes_into(&mut token);
    token
}

pub async fn authenticate_local(
    stream: &mut (impl AsyncRead + Unpin),
    token: &[u8; 32],
) -> ResultType<SocketAddr> {
    let mut header = [0; 48];
    timeout(Duration::from_secs(1), stream.read_exact(&mut header)).await??;
    if !memcmp(&header[..32], token) || header[32] != 0xfd {
        bail!("Unauthenticated local tailcat connection");
    }
    let mut identity = [0; 16];
    identity.copy_from_slice(&header[32..]);
    Ok(SocketAddr::from((Ipv6Addr::from(identity), 0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbb_common::tokio::{self, io::AsyncWriteExt};

    #[tokio::test]
    async fn accepts_only_authenticated_peer_metadata_and_preserves_payload() {
        let token = local_token();
        let (mut sender, mut receiver) = tokio::io::duplex(128);
        sender.write_all(&token).await.unwrap();
        let mut identity = [0; 16];
        identity[0] = 0xfd;
        identity[15] = 42;
        sender.write_all(&identity).await.unwrap();
        sender.write_all(b"payload").await.unwrap();
        let peer = authenticate_local(&mut receiver, &token).await.unwrap();
        assert_eq!(peer.ip(), Ipv6Addr::from(identity));
        let mut payload = [0; 7];
        receiver.read_exact(&mut payload).await.unwrap();
        assert_eq!(&payload, b"payload");
    }

    #[tokio::test]
    async fn rejects_wrong_token_and_invalid_identity() {
        let token = local_token();
        let mut valid_identity = [0; 16];
        valid_identity[0] = 0xfd;
        for (received, identity) in [(local_token(), valid_identity), (token, [0; 16])] {
            let (mut sender, mut receiver) = tokio::io::duplex(64);
            sender.write_all(&received).await.unwrap();
            sender.write_all(&identity).await.unwrap();
            assert!(authenticate_local(&mut receiver, &token).await.is_err());
        }
    }

    #[tokio::test]
    async fn refuses_truncated_and_stalled_local_connections() {
        let token = local_token();
        let (sender, mut receiver) = tokio::io::duplex(64);
        drop(sender);
        assert!(authenticate_local(&mut receiver, &token).await.is_err());
        let (_sender, mut receiver) = tokio::io::duplex(64);
        assert!(authenticate_local(&mut receiver, &token).await.is_err());
    }
}
