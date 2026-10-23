use crate::{tcp, ResultType};
use sodiumoxide::crypto::secretbox::Key;
use std::net::SocketAddr;
use tokio::net::TcpStream;

/// The one transport: a framed TCP stream, end to end encrypted by the key exchange.
pub struct Stream(pub tcp::FramedStream);

impl Stream {
    #[inline]
    pub fn set_send_timeout(&mut self, ms: u64) {
        self.0.set_send_timeout(ms)
    }

    #[inline]
    pub fn set_raw(&mut self) {
        self.0.set_raw()
    }

    /// The largest message the peer may send. Lowered before authorization, where nothing a peer
    /// legitimately sends is large, so an unauthenticated connection cannot make us hold more than
    /// that. `usize::MAX` restores the default.
    ///
    /// Set it before the first read of untrusted data.
    #[inline]
    pub fn set_max_packet_length(&mut self, n: usize) {
        self.0.codec_mut().set_max_packet_length(n)
    }

    #[inline]
    pub async fn send_bytes(&mut self, bytes: bytes::Bytes) -> ResultType<()> {
        self.0.send_bytes(bytes).await
    }

    #[inline]
    pub async fn send_raw(&mut self, bytes: Vec<u8>) -> ResultType<()> {
        self.0.send_raw(bytes).await
    }

    #[inline]
    pub fn set_key(&mut self, key: Key) {
        self.0.set_key(key)
    }

    /// The stream key for the key exchange version in `t.picked`: version 0 is `set_key`, and
    /// version 1 derives one key per direction from the transcript. Any other version is refused
    /// rather than run as one of these. `is_initiator` is the side that sent the sealed key.
    #[inline]
    pub fn set_negotiated_key(
        &mut self,
        key: Key,
        is_initiator: bool,
        t: &tcp::KxTranscript,
    ) -> ResultType<()> {
        match t.picked {
            0 => {
                self.set_key(key);
                Ok(())
            }
            1 => self.0.set_key_split(key, is_initiator, t),
            v => crate::bail!("unsupported key exchange version {}", v),
        }
    }

    #[inline]
    pub fn set_login_binder(&mut self, binder: [u8; 32]) {
        self.0.set_login_binder(binder)
    }

    #[inline]
    pub fn login_binder(&self) -> Option<[u8; 32]> {
        self.0.login_binder()
    }

    #[inline]
    pub fn is_secured(&self) -> bool {
        self.0.is_secured()
    }

    #[inline]
    pub async fn next_timeout(
        &mut self,
        timeout: u64,
    ) -> Option<Result<bytes::BytesMut, std::io::Error>> {
        self.0.next_timeout(timeout).await
    }

    /// send message
    #[inline]
    pub async fn send(&mut self, msg: &impl protobuf::Message) -> ResultType<()> {
        self.0.send(msg).await
    }

    /// receive message
    #[inline]
    pub async fn next(&mut self) -> Option<Result<bytes::BytesMut, std::io::Error>> {
        self.0.next().await
    }

    #[inline]
    pub fn local_addr(&self) -> SocketAddr {
        self.0.local_addr()
    }

    #[inline]
    pub fn from(stream: TcpStream, stream_addr: SocketAddr) -> Self {
        Self(tcp::FramedStream::from(stream, stream_addr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_set_negotiated_key_refuses_unknown_versions() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut stream = Stream::from(TcpStream::connect(addr).await.unwrap(), addr);
        let t = |picked| tcp::KxTranscript {
            initiator_pk: &[1u8; 32],
            responder_pk: &[2u8; 32],
            advertised: 1,
            picked,
        };
        let mut set = |picked| stream.set_negotiated_key(Key([0u8; 32]), true, &t(picked));
        assert!(set(2).is_err());
        assert!(set(1).is_ok());
    }
}
