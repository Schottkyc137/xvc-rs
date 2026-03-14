//! [`tokio_util::codec`] implementations for the XVC protocol.
//!
//! This module provides [`MessageDecoder`] and [`XvcInfoDecoder`], which implement
//! [`tokio_util::codec::Decoder`] and can be used with [`tokio_util::codec::FramedRead`]
//! to drive async XVC message parsing over a [`tokio::net::TcpStream`] (or any other
//! [`tokio::io::AsyncRead`] source).
//!
//! Enable with the `tokio` feature flag:
//!
//! ```toml
//! xvc-protocol = { version = "...", features = ["tokio"] }
//! ```
//!
//! ## Usage
//!
//! ### Decoding inbound client messages (server side)
//!
//! ```ignore
//! use tokio_util::codec::FramedRead;
//! use xvc_protocol::tokio_codec::MessageDecoder;
//! use futures::StreamExt;
//!
//! async fn example(tcp: tokio::net::TcpStream) {
//!     let mut framed = FramedRead::new(tcp, MessageDecoder::new(1024));
//!     while let Some(msg) = framed.next().await {
//!         // msg: Result<xvc_protocol::Message, ReadError>
//!     }
//! }
//! ```
//!
//! ### Decoding the server capability response (client side)
//!
//! ```ignore
//! use tokio_util::codec::FramedRead;
//! use xvc_protocol::tokio_codec::XvcInfoDecoder;
//! use futures::StreamExt;
//!
//! async fn example(tcp: tokio::net::TcpStream) {
//!     let mut framed = FramedRead::new(tcp, XvcInfoDecoder);
//!     if let Some(Ok(info)) = framed.next().await {
//!         // info: xvc_protocol::XvcInfo
//!     }
//! }
//! ```

use bytes::{Buf, BytesMut};
use tokio_util::codec::Decoder;

use crate::{
    Message, XvcCommand, XvcInfo,
    codec::{ParseErr, SetTck, Shift},
    error::ReadError,
};

/// Decodes [`Message`]s from an inbound byte stream (client → server direction).
///
/// Intended for use with [`tokio_util::codec::FramedRead`] on the server side.
/// Each call to [`Decoder::decode`] attempts to parse one complete [`Message`]
/// from the accumulated bytes; it returns `Ok(None)` when more data is needed.
///
/// The `max_shift` parameter caps the maximum number of bytes allowed per TMS
/// and TDI vector in a `Shift` command. Frames exceeding this limit produce a
/// [`ReadError::TooManyBytes`] error.
pub struct MessageDecoder {
    max_shift: usize,
}

impl MessageDecoder {
    /// Create a new decoder.
    ///
    /// `max_shift` is the per-vector byte limit for `Shift` payloads (each of
    /// TMS and TDI independently). Should match the `max_vector_size` advertised
    /// via [`XvcInfo`].
    pub fn new(max_shift: usize) -> Self {
        Self { max_shift }
    }
}

impl Decoder for MessageDecoder {
    type Item = Message;
    type Error = ReadError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut slice: &[u8] = src;

        let cmd = match XvcCommand::parse(&mut slice) {
            Ok(cmd) => cmd,
            Err(ParseErr::Incomplete) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let msg = match cmd {
            XvcCommand::GetInfo => Message::GetInfo,
            XvcCommand::SetTck => match SetTck::parse(&mut slice) {
                Ok(tck) => Message::SetTck {
                    period_ns: tck.period(),
                },
                Err(ParseErr::Incomplete) => return Ok(None),
                Err(e) => return Err(e.into()),
            },
            XvcCommand::Shift => match Shift::parse(&mut slice, self.max_shift) {
                Ok(shift) => {
                    let num_bits = shift.num_bits();
                    let (tms, tdi) = shift.into_tms_tdi();
                    Message::Shift { num_bits, tms, tdi }
                }
                Err(ParseErr::Incomplete) => return Ok(None),
                Err(e) => return Err(e.into()),
            },
        };

        let consumed = src.len() - slice.len();
        src.advance(consumed);
        Ok(Some(msg))
    }
}

/// Decodes an [`XvcInfo`] frame from an inbound byte stream (server → client direction).
///
/// Intended for use with [`tokio_util::codec::FramedRead`] on the client side,
/// to read the server's capability response after sending a `getinfo:` message.
/// The frame is newline-terminated; `Ok(None)` is returned until a `\n` is seen.
pub struct XvcInfoDecoder;

impl Decoder for XvcInfoDecoder {
    type Item = XvcInfo;
    type Error = ReadError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let mut slice: &[u8] = src;
        match XvcInfo::parse(&mut slice) {
            Ok(info) => {
                let consumed = src.len() - slice.len();
                src.advance(consumed);
                Ok(Some(info))
            }
            Err(ParseErr::Incomplete) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;
    use tokio_util::codec::Decoder;

    use super::{MessageDecoder, XvcInfoDecoder};
    use crate::{Message, Version, XvcInfo};

    // MARK: MessageDecoder

    #[test]
    fn decode_getinfo() {
        let mut dec = MessageDecoder::new(1024);
        let mut buf = BytesMut::from(&b"getinfo:"[..]);
        assert_eq!(dec.decode(&mut buf).unwrap(), Some(Message::GetInfo));
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_settck() {
        let period: u32 = 0x1234_5678;
        let mut data = b"settck:".to_vec();
        data.extend_from_slice(&period.to_le_bytes());
        let mut dec = MessageDecoder::new(1024);
        let mut buf = BytesMut::from(data.as_slice());
        assert_eq!(
            dec.decode(&mut buf).unwrap(),
            Some(Message::SetTck { period_ns: period })
        );
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_shift() {
        let num_bits: u32 = 16;
        let tms = vec![0xAAu8, 0xBB];
        let tdi = vec![0x11u8, 0x22];
        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&tms);
        data.extend_from_slice(&tdi);
        let mut dec = MessageDecoder::new(1024);
        let mut buf = BytesMut::from(data.as_slice());
        match dec.decode(&mut buf).unwrap().unwrap() {
            Message::Shift {
                num_bits: nb,
                tms: t,
                tdi: d,
            } => {
                assert_eq!(nb, 16);
                assert_eq!(&*t, &tms[..]);
                assert_eq!(&*d, &tdi[..]);
            }
            other => panic!("expected Shift, got {:?}", other),
        }
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_incomplete_returns_none() {
        let mut dec = MessageDecoder::new(1024);
        // Only partial command prefix
        let mut buf = BytesMut::from(&b"geti"[..]);
        assert_eq!(dec.decode(&mut buf).unwrap(), None);
        // Command present but no payload yet
        let mut buf = BytesMut::from(&b"settck:"[..]);
        assert_eq!(dec.decode(&mut buf).unwrap(), None);
    }

    #[test]
    fn decode_leaves_trailing_bytes() {
        // Two back-to-back getinfo messages: decoder should consume exactly one.
        let mut dec = MessageDecoder::new(1024);
        let mut buf = BytesMut::from(&b"getinfo:getinfo:"[..]);
        assert_eq!(dec.decode(&mut buf).unwrap(), Some(Message::GetInfo));
        assert_eq!(&buf[..], b"getinfo:");
    }

    #[test]
    fn decode_too_many_bytes() {
        let max_shift = 2;
        let num_bits: u32 = 32; // 4 bytes each, exceeds max_shift=2
        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        let mut dec = MessageDecoder::new(max_shift);
        let mut buf = BytesMut::from(data.as_slice());
        assert!(matches!(
            dec.decode(&mut buf),
            Err(crate::error::ReadError::TooManyBytes { .. })
        ));
    }

    // MARK: XvcInfoDecoder

    #[test]
    fn decode_xvc_info() {
        let mut dec = XvcInfoDecoder;
        let mut buf = BytesMut::from(&b"xvcServer_v1.0:1024\n"[..]);
        let info = dec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(info.version(), Version::V1_0);
        assert_eq!(info.max_vector_len(), 1024);
        assert!(buf.is_empty());
    }

    #[test]
    fn decode_xvc_info_incomplete() {
        let mut dec = XvcInfoDecoder;
        // No newline yet
        let mut buf = BytesMut::from(&b"xvcServer_v1.0:1024"[..]);
        assert_eq!(dec.decode(&mut buf).unwrap(), None);
    }

    #[test]
    fn decode_xvc_info_leaves_trailing_bytes() {
        let mut dec = XvcInfoDecoder;
        let mut buf = BytesMut::from(&b"xvcServer_v1.0:32\nextra"[..]);
        let info = dec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(info, XvcInfo::new(Version::V1_0, 32));
        assert_eq!(&buf[..], b"extra");
    }
}
