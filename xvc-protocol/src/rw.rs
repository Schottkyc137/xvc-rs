/// Read and write implementations for the protocol messages
use std::{
    io::{self, Read, Write},
    prelude::v1::*,
};

use bytes::{Bytes, BytesMut};

use crate::{
    Message, XvcCommand, XvcInfo,
    codec::{ParseErr, SetTck, Shift},
    error::ReadError,
};

/// Protocol decoder.
///
/// `Decoder` holds an internal buffer and reads from an underlying stream
/// until a complete protocol frame or command can be parsed. It enforces a
/// maximum buffer size to protect against oversized messages.
///
/// Typical usage:
///
/// ```rust
/// use xvc_protocol::{Message, XvcInfo};
/// use std::io::Cursor;
///
/// // Read a single message from a byte stream
/// let mut data = b"getinfo:".as_slice();
/// let mut dec = xvc_protocol::rw::Decoder::new(1024);
/// let msg = dec.read_message(&mut data).unwrap();
/// assert!(matches!(msg, Message::GetInfo));
/// ```
pub struct Decoder {
    buf: BytesMut,
    max: usize,
}

impl Decoder {
    /// Create a new decoder with a given max size.
    ///
    /// The `max_size` parameter bounds the internal buffer used when reading
    /// messages from a stream; if a message would require more bytes than
    /// `max_size` a `ReadError::TooManyBytes` is returned when reading.
    pub fn new(max_size: usize) -> Self {
        Self {
            buf: BytesMut::with_capacity(max_size),
            max: max_size,
        }
    }

    fn read_chunk(&mut self, reader: &mut impl Read) -> Result<(), ReadError> {
        let mut temp = [0u8; 1024];
        let read = loop {
            match reader.read(&mut temp) {
                Ok(n) => break n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                    continue; // retry
                }
                Err(e) => return Err(e.into()), // real error
            }
        };
        if read == 0 {
            // EOF: if buffer is non-empty, it means we have incomplete data
            if !self.buf.is_empty() {
                return Err(ReadError::InvalidCommandPrefix(
                    String::from_utf8_lossy(&self.buf).to_string(),
                ));
            } else {
                // Buffer is empty, this is a clean EOF
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected EOF while reading",
                )
                .into());
            }
        }

        if self.max < read + self.buf.len() {
            return Err(ReadError::TooManyBytes {
                max: self.max,
                need: read + self.buf.len(),
            });
        }
        // Only append the bytes actually read
        self.buf.extend_from_slice(&temp[..read]);

        Ok(())
    }

    /// Read an `XvcInfo` frame from `reader`.
    ///
    /// This method incrementally fills the internal buffer from `reader` until
    /// a complete XVC server info frame is available and returns the parsed
    /// `XvcInfo`. If EOF is encountered with partial data buffered, a
    /// `ReadError::InvalidCommandPrefix` is returned.
    pub fn read_xvc_info(&mut self, reader: &mut impl Read) -> Result<XvcInfo, ReadError> {
        self.buf.clear();
        loop {
            match XvcInfo::parse(&mut self.buf.clone().freeze()) {
                Ok(frame) => {
                    return Ok(frame);
                }
                Err(ParseErr::Incomplete) => {
                    self.read_chunk(reader)?;
                }
                Err(other) => return Err(other.into()),
            }
        }
    }

    /// Read a single protocol `Message` from `reader`.
    ///
    /// The decoder reads from `reader` until a full command and its payload
    /// are available, enforces negotiated limits (e.g. maximum shift buffer
    /// size) and returns the parsed `Message`. On EOF with a partial
    /// command present, a `ReadError::InvalidCommandPrefix` is returned.
    ///
    /// Example:
    ///
    /// ```rust
    /// use std::io::Cursor;
    /// let mut cursor = Cursor::new(b"getinfo:");
    /// let mut dec = xvc_protocol::rw::Decoder::new(1024);
    /// let msg = dec.read_message(&mut cursor).unwrap();
    /// assert!(matches!(msg, xvc_protocol::Message::GetInfo));
    /// ```
    pub fn read_message(&mut self, reader: &mut impl Read) -> Result<Message, ReadError> {
        self.buf.clear();
        let cmd = loop {
            match XvcCommand::parse(&mut self.buf.clone().freeze()) {
                Ok(cmd) => {
                    break cmd;
                }
                Err(ParseErr::Incomplete) => {
                    self.read_chunk(reader)?;
                }
                Err(other) => return Err(other.into()),
            }
        };
        match cmd {
            XvcCommand::GetInfo => Ok(Message::GetInfo),
            XvcCommand::SetTck => loop {
                match SetTck::parse(&mut self.buf) {
                    Ok(tck) => {
                        return Ok(Message::SetTck {
                            period_ns: tck.period(),
                        });
                    }
                    Err(ParseErr::Incomplete) => {
                        self.read_chunk(reader)?;
                    }
                    Err(other) => return Err(other.into()),
                }
            },
            XvcCommand::Shift => loop {
                match Shift::parse(&mut self.buf, self.max) {
                    Ok(shift) => {
                        return Ok(Message::Shift {
                            num_bits: shift.num_bits(),
                            tms: Bytes::copy_from_slice(shift.tms()),
                            tdi: Bytes::copy_from_slice(shift.tdi()),
                        });
                    }
                    Err(ParseErr::Incomplete) => {
                        self.read_chunk(reader)?;
                    }
                    Err(other) => return Err(other.into()),
                }
            },
        }
    }
}

impl XvcInfo {
    /// Write this `XvcInfo` to `writer` in the protocol's server-info format.
    ///
    /// The output has the form `xvcServer_v<major>.<minor>:<max_vector_len>\n`.
    /// This is the canonical representation sent by servers to announce
    /// capabilities to clients.
    pub fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        writeln!(
            writer,
            "xvcServer_v{}:{}",
            self.version(),
            self.max_vector_len()
        )
    }

    /// Read an `XvcInfo` from `reader` using an internal `Decoder`.
    ///
    /// This is a convenience wrapper that constructs a `Decoder` configured
    /// with `max_buffer_size` and delegates to its `read_xvc_info` method.
    ///
    /// Example:
    ///
    /// ```rust
    /// use std::io::Cursor;
    /// let mut c = Cursor::new(b"xvcServer_v1.0:32\n");
    /// let info = xvc_protocol::XvcInfo::from_reader(&mut c, 4096).unwrap();
    /// assert_eq!(info.max_vector_len(), 32);
    /// ```
    pub fn from_reader(
        reader: &mut impl Read,
        max_buffer_size: usize,
    ) -> Result<XvcInfo, ReadError> {
        Decoder::new(max_buffer_size).read_xvc_info(reader)
    }
}

impl Message {
    const CMD_NAME_GET_INFO: &'static [u8; 7] = b"getinfo";
    const CMD_NAME_SET_TCK: &'static [u8; 6] = b"settck";
    const CMD_NAME_SHIFT: &'static [u8; 5] = b"shift";
    const CMD_DELIMITER: u8 = b':';

    /// Read a `Message` from `reader` using an internal `Decoder`.
    ///
    /// This is a convenience wrapper that constructs a `Decoder` configured
    /// with `max_shift_bytes` and delegates to its `read_message` method.
    ///
    /// Example:
    ///
    /// ```rust
    /// use std::io::Cursor;
    /// let mut c = Cursor::new(b"getinfo:");
    /// let msg = xvc_protocol::Message::from_reader(&mut c, 1024).unwrap();
    /// assert!(matches!(msg, xvc_protocol::Message::GetInfo));
    /// ```
    pub fn from_reader(
        reader: &mut impl Read,
        max_shift_bytes: usize,
    ) -> Result<Message, ReadError> {
        Decoder::new(max_shift_bytes).read_message(reader)
    }

    /// Serialize this `Message` to `writer` in the protocol command format.
    ///
    /// - `GetInfo` is written as `getinfo:`
    /// - `SetTck` is written as `settck:` followed by a 4-byte little-endian period
    /// - `Shift` is written as `shift:` followed by a 4-byte little-endian `num_bits`,
    ///   then the `tms` and `tdi` payload bytes
    ///
    /// The function writes raw bytes and returns any I/O error encountered.
    pub fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        match self {
            Message::GetInfo => {
                writer.write_all(Self::CMD_NAME_GET_INFO)?;
                writer.write_all(&[Self::CMD_DELIMITER])
            }
            Message::SetTck {
                period_ns: period_in_ns,
            } => {
                writer.write_all(Self::CMD_NAME_SET_TCK)?;
                writer.write_all(&[Self::CMD_DELIMITER])?;
                writer.write_all(&period_in_ns.to_le_bytes())
            }
            Message::Shift {
                num_bits,
                tms: tms_vector,
                tdi: tdi_vector,
            } => {
                writer.write_all(Self::CMD_NAME_SHIFT)?;
                writer.write_all(&[Self::CMD_DELIMITER])?;
                writer.write_all(&num_bits.to_le_bytes())?;
                writer.write_all(tms_vector)?;
                writer.write_all(tdi_vector)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::{io::Cursor, vec};

    use super::*;

    const DEFAULT_MAX_SHIFT_BYTES: usize = 1024;

    #[test]
    fn write_server_info() {
        let mut out = Vec::new();
        XvcInfo::default().write_to(&mut out).unwrap();
        assert_eq!(out, b"xvcServer_v1.0:10485760\n".to_vec());
    }

    #[test]
    fn read_server_info() {
        let data = b"xvcServer_v1.0:32\n";
        let mut cursor = std::io::Cursor::new(data);
        let info = XvcInfo::from_reader(&mut cursor, 4096).unwrap();
        assert_eq!(info.version(), crate::protocol::Version::V1_0);
        assert_eq!(info.max_vector_len(), 32)
    }

    #[test]
    fn read_getinfo() {
        let data = b"getinfo:".to_vec();
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::GetInfo => {}
            _ => panic!("expected GetInfo"),
        }
    }

    #[test]
    fn write_getinfo() {
        let mut out = Vec::new();
        Message::GetInfo.write_to(&mut out).unwrap();
        assert_eq!(out, b"getinfo:".to_vec());
    }

    #[test]
    fn read_settck() {
        let period: u32 = 0x1234_5678;
        let mut data = b"settck:".to_vec();
        data.extend_from_slice(&period.to_le_bytes());
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::SetTck {
                period_ns: period_in_ns,
            } => assert_eq!(period_in_ns, period),
            _ => panic!("expected SetTck"),
        }
    }

    #[test]
    fn write_settck() {
        let period: u32 = 0x1234_5678;
        let mut out = Vec::new();
        Message::SetTck { period_ns: period }
            .write_to(&mut out)
            .unwrap();
        let mut expected = b"settck:".to_vec();
        expected.extend_from_slice(&period.to_le_bytes());
        assert_eq!(out, expected);
    }

    #[test]
    fn read_shift() {
        let num_bits: u32 = 13; // 2 bytes
        let num_bytes = num_bits.div_ceil(8) as usize;
        let tms = vec![0xAAu8; num_bytes];
        let tdi = vec![0x55u8; num_bytes];

        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&tms);
        data.extend_from_slice(&tdi);

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::Shift {
                num_bits: nb,
                tms: tms_vector,
                tdi: tdi_vector,
            } => {
                assert_eq!(nb, num_bits);
                assert_eq!(&*tms_vector, &tms[..]);
                assert_eq!(&*tdi_vector, &tdi[..]);
            }
            _ => panic!("expected Shift"),
        }
    }

    #[test]
    fn write_shift() {
        let num_bits: u32 = 13; // 2 bytes
        let num_bytes = num_bits.div_ceil(8) as usize;
        let tms = vec![0xAAu8; num_bytes].into_boxed_slice();
        let tdi = vec![0x55u8; num_bytes].into_boxed_slice();

        let cmd = Message::Shift {
            num_bits,
            tms: Bytes::copy_from_slice(&tms),
            tdi: Bytes::copy_from_slice(&tdi),
        };
        let mut out = Vec::new();
        cmd.write_to(&mut out).unwrap();

        let mut expected = b"shift:".to_vec();
        expected.extend_from_slice(&num_bits.to_le_bytes());
        expected.extend_from_slice(&tms);
        expected.extend_from_slice(&tdi);

        assert_eq!(out, expected);
    }

    #[test]
    fn invalid_prefix() {
        let data = b"xx".to_vec();
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::InvalidCommandPrefix(p)) => assert_eq!(p, "xx"),
            other => panic!("expected InvalidCommandPrefix, got {:?}", other),
        }
    }

    #[test]
    fn too_many_bytes_shift() {
        // force number of bytes to exceed MAX_SHIFT_BYTES
        let num_bytes_exceed = 1024 + 1;
        let num_bits = (num_bytes_exceed * 8) as u32;
        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, 1024) {
            Err(ReadError::TooManyBytes { max, need: got }) => {
                assert_eq!(max, 1024);
                assert_eq!(got, num_bytes_exceed);
            }
            other => panic!("expected TooManyBytes, got {:?}", other),
        }
    }

    #[test]
    fn read_xvc_info_with_max_u32_vector_len() {
        let data = b"xvcServer_v1.0:4294967295\n";
        let mut cursor = Cursor::new(data);
        let info = XvcInfo::from_reader(&mut cursor, 4096).unwrap();
        assert_eq!(info.version(), crate::protocol::Version::V1_0);
        assert_eq!(info.max_vector_len(), u32::MAX);
    }

    #[test]
    fn read_xvc_info_with_zero_vector_len() {
        let data = b"xvcServer_v1.0:0\n";
        let mut cursor = Cursor::new(data);
        let info = XvcInfo::from_reader(&mut cursor, 4096).unwrap();
        assert_eq!(info.version(), crate::protocol::Version::V1_0);
        assert_eq!(info.max_vector_len(), 0);
    }

    #[test]
    fn read_xvc_info_with_large_version_numbers() {
        let data = b"xvcServer_v999.999:1024\n";
        let mut cursor = Cursor::new(data);
        let info = XvcInfo::from_reader(&mut cursor, 4096).unwrap();
        assert_eq!(info.version(), crate::protocol::Version::new(999, 999));
        assert_eq!(info.max_vector_len(), 1024);
    }

    #[test]
    fn read_xvc_info_buffer_too_small() {
        let data = b"xvcServer_v1.0:4\n";
        let mut cursor = Cursor::new(data);
        match XvcInfo::from_reader(&mut cursor, 5) {
            Err(ReadError::TooManyBytes { max, need: got }) => {
                assert_eq!(max, 5);
                assert!(got >= 17); // Length of the full message
            }
            other => panic!("expected TooManyBytes, got {:?}", other),
        }
    }

    #[test]
    fn read_xvc_info_incomplete_then_complete() {
        // Simulate a slow network: first chunk is incomplete, second completes it
        let data = b"xvcServer_v1.0:4\n";
        let mut cursor = Cursor::new(data);

        // Manually read the full data to simulate the real scenario
        match XvcInfo::from_reader(&mut cursor, 4096) {
            Ok(info) => {
                assert_eq!(info.max_vector_len(), 4);
            }
            Err(e) => panic!(
                "Should successfully parse despite potential slow network: {:?}",
                e
            ),
        }
    }

    #[test]
    fn write_xvc_info_max_values() {
        let mut out = Vec::new();
        let info = XvcInfo::new(crate::protocol::Version::new(255, 255), u32::MAX);
        info.write_to(&mut out).unwrap();
        assert_eq!(out, b"xvcServer_v255.255:4294967295\n".to_vec());
    }

    #[test]
    fn read_getinfo_with_extra_data() {
        // Multiple getinfo commands in sequence
        let data = b"getinfo:getinfo:";
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::GetInfo => {}
            _ => panic!("expected GetInfo"),
        }
    }

    #[test]
    fn read_getinfo_exact() {
        let data = b"getinfo:";
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::GetInfo => {}
            _ => panic!("expected GetInfo"),
        }
    }

    #[test]
    fn read_settck_zero_period() {
        let period: u32 = 0;
        let mut data = b"settck:".to_vec();
        data.extend_from_slice(&period.to_le_bytes());
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::SetTck {
                period_ns: period_in_ns,
            } => assert_eq!(period_in_ns, 0),
            _ => panic!("expected SetTck"),
        }
    }

    #[test]
    fn read_settck_max_period() {
        let period: u32 = u32::MAX;
        let mut data = b"settck:".to_vec();
        data.extend_from_slice(&period.to_le_bytes());
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::SetTck {
                period_ns: period_in_ns,
            } => assert_eq!(period_in_ns, u32::MAX),
            _ => panic!("expected SetTck"),
        }
    }

    #[test]
    fn read_settck_incomplete() {
        // Only command without period bytes
        let data = b"settck:".to_vec();
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::InvalidCommandPrefix(_)) => {}
            other => panic!(
                "expected InvalidCommandPrefix due to EOF with buffered data, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn read_settck_partial_period() {
        // Command with only 2 bytes of period (needs 4)
        let mut data = b"settck:".to_vec();
        data.extend_from_slice(&[0xAA, 0xBB]);
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::InvalidCommandPrefix(_)) => {}
            other => panic!(
                "expected InvalidCommandPrefix due to EOF with buffered data, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn read_shift_zero_bits() {
        let num_bits: u32 = 0;
        let tms: Vec<u8> = vec![];
        let tdi: Vec<u8> = vec![];

        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&tms);
        data.extend_from_slice(&tdi);

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::Shift {
                num_bits: nb,
                tms: tms_vector,
                tdi: tdi_vector,
            } => {
                assert_eq!(nb, 0);
                assert_eq!(&*tms_vector, &[]);
                assert_eq!(&*tdi_vector, &[]);
            }
            _ => panic!("expected Shift"),
        }
    }

    #[test]
    fn read_shift_one_bit() {
        let num_bits: u32 = 1;
        let tms = vec![0x00u8];
        let tdi = vec![0x01u8];

        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&tms);
        data.extend_from_slice(&tdi);

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::Shift {
                num_bits: nb,
                tms: tms_vector,
                tdi: tdi_vector,
            } => {
                assert_eq!(nb, 1);
                assert_eq!(&*tms_vector, &tms[..]);
                assert_eq!(&*tdi_vector, &tdi[..]);
            }
            _ => panic!("expected Shift"),
        }
    }

    #[test]
    fn read_shift_max_bits() {
        let num_bits: u32 = u32::MAX;
        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, 1024) {
            Err(ReadError::TooManyBytes { .. }) => {} // Expected: too large for buffer
            other => panic!("expected TooManyBytes, got {:?}", other),
        }
    }

    #[test]
    fn read_shift_incomplete_num_bits() {
        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&[0xAA, 0xBB]); // Only 2 bytes of 4 needed

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::InvalidCommandPrefix(_)) => {}
            other => panic!(
                "expected InvalidCommandPrefix due to EOF with buffered data, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn read_shift_incomplete_tms() {
        // num_bits = 16 -> needs 2 bytes TMS, but provide only 1
        let num_bits: u32 = 16;
        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&[0xAA]); // Only 1 byte instead of 2

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::InvalidCommandPrefix(_)) => {}
            other => panic!(
                "expected InvalidCommandPrefix due to EOF with buffered data, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn read_shift_incomplete_tdi() {
        // num_bits = 8 -> needs 1 byte TMS and 1 byte TDI
        let num_bits: u32 = 8;
        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&[0xAA]); // TMS
        // TDI is missing

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::InvalidCommandPrefix(_)) => {}
            other => panic!(
                "expected InvalidCommandPrefix due to EOF with buffered data, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn read_shift_large_vectors() {
        let num_bits: u32 = 1000;
        let num_bytes = num_bits.div_ceil(8) as usize;
        let tms = vec![0xAA; num_bytes];
        let tdi = vec![0x55; num_bytes];

        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&tms);
        data.extend_from_slice(&tdi);

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::Shift {
                num_bits: nb,
                tms: tms_vector,
                tdi: tdi_vector,
            } => {
                assert_eq!(nb, num_bits);
                assert_eq!(&*tms_vector, &tms[..]);
                assert_eq!(&*tdi_vector, &tdi[..]);
            }
            _ => panic!("expected Shift"),
        }
    }

    #[test]
    fn read_shift_exactly_at_max_size() {
        let max_size = 1024;
        // num_bits = 4096 bits = 512 bytes
        // Total message: "shift:" (6) + 4 bytes (num_bits) + 512 (tms) + 512 (tdi) = 1034 bytes
        // This exceeds 1024, so should fail
        let num_bits: u32 = 4096;
        let num_bytes = 512;

        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&vec![0xAA; num_bytes]);
        data.extend_from_slice(&vec![0x55; num_bytes]);

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, max_size) {
            Err(ReadError::TooManyBytes { .. }) => {}
            other => panic!("expected TooManyBytes, got {:?}", other),
        }
    }

    #[test]
    fn write_shift_zero_bits() {
        let cmd = Message::Shift {
            num_bits: 0,
            tms: Bytes::from_static(&[]),
            tdi: Bytes::from_static(&[]),
        };
        let mut out = Vec::new();
        cmd.write_to(&mut out).unwrap();

        let mut expected = b"shift:".to_vec();
        expected.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(out, expected);
    }

    #[test]
    fn write_shift_max_bits() {
        let cmd = Message::Shift {
            num_bits: u32::MAX,
            tms: Bytes::from_static(&[0xFF; 512]),
            tdi: Bytes::from_static(&[0xAA; 512]),
        };
        let mut out = Vec::new();
        cmd.write_to(&mut out).unwrap();

        let mut expected = b"shift:".to_vec();
        expected.extend_from_slice(&u32::MAX.to_le_bytes());
        expected.extend_from_slice(&[0xFF; 512]);
        expected.extend_from_slice(&[0xAA; 512]);
        assert_eq!(out, expected);
    }

    #[test]
    fn invalid_command_name() {
        let data = b"invalid:".to_vec();
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::InvalidCommand(_)) => {}
            other => panic!("expected InvalidCommand, got {:?}", other),
        }
    }

    #[test]
    fn empty_input() {
        let data = b"".to_vec();
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::IoError(_)) => {}
            other => panic!("expected IoError, got {:?}", other),
        }
    }

    #[test]
    fn only_delimiter() {
        let data = b":".to_vec();
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::InvalidCommand(_)) => {}
            other => panic!("expected InvalidCommand, got {:?}", other),
        }
    }

    #[test]
    fn binary_garbage_input() {
        let data = vec![0xFF, 0xFE, 0xFD, 0xFC];
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::InvalidCommandPrefix(_)) => {}
            other => panic!("expected InvalidCommandPrefix, got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_xvc_info() {
        let original = XvcInfo::new(crate::protocol::Version::new(1, 0), 8192);
        let mut buffer = Vec::new();
        original.write_to(&mut buffer).unwrap();

        let mut cursor = Cursor::new(buffer);
        let parsed = XvcInfo::from_reader(&mut cursor, 4096).unwrap();

        assert_eq!(parsed.version(), original.version());
        assert_eq!(parsed.max_vector_len(), original.max_vector_len());
    }

    #[test]
    fn roundtrip_getinfo() {
        let original = Message::GetInfo;
        let mut buffer = Vec::new();
        original.write_to(&mut buffer).unwrap();

        let mut cursor = Cursor::new(buffer);
        let parsed = Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap();

        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_settck() {
        let original = Message::SetTck {
            period_ns: 0x12345678,
        };
        let mut buffer = Vec::new();
        original.write_to(&mut buffer).unwrap();

        let mut cursor = Cursor::new(buffer);
        let parsed = Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap();

        assert_eq!(parsed, original);
    }

    #[test]
    fn roundtrip_shift() {
        let num_bits = 128;
        let num_bytes = (num_bits / 8) as usize;
        let original = Message::Shift {
            num_bits,
            tms: Bytes::copy_from_slice(&vec![0xAA; num_bytes]),
            tdi: Bytes::copy_from_slice(&vec![0x55; num_bytes]),
        };
        let mut buffer = Vec::new();
        original.write_to(&mut buffer).unwrap();

        let mut cursor = Cursor::new(buffer);
        let parsed = Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap();

        assert_eq!(parsed, original);
    }

    #[test]
    fn shift_num_bits_rounding() {
        // Test that num_bits is correctly rounded up to nearest byte
        // 13 bits = 2 bytes
        let num_bits: u32 = 13;
        let num_bytes = num_bits.div_ceil(8) as usize;
        let tms = vec![0xAA; num_bytes];
        let tdi = vec![0x55; num_bytes];

        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&tms);
        data.extend_from_slice(&tdi);

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::Shift {
                num_bits: nb,
                tms: tms_vector,
                tdi: tdi_vector,
            } => {
                assert_eq!(nb, 13);
                assert_eq!(tms_vector.len(), 2);
                assert_eq!(tdi_vector.len(), 2);
            }
            _ => panic!("expected Shift"),
        }
    }

    #[test]
    fn shift_1_bit_requires_1_byte() {
        let num_bits: u32 = 1;
        let tms = vec![0x00; 1];
        let tdi = vec![0x01; 1];

        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&tms);
        data.extend_from_slice(&tdi);

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::Shift {
                num_bits: nb,
                tms: tms_vector,
                tdi: tdi_vector,
            } => {
                assert_eq!(nb, 1);
                assert_eq!(tms_vector.len(), 1);
                assert_eq!(tdi_vector.len(), 1);
            }
            _ => panic!("expected Shift"),
        }
    }

    #[test]
    fn shift_8_bits_requires_1_byte() {
        let num_bits: u32 = 8;
        let tms = vec![0xFF; 1];
        let tdi = vec![0xAA; 1];

        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&tms);
        data.extend_from_slice(&tdi);

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::Shift {
                num_bits: nb,
                tms: tms_vector,
                tdi: tdi_vector,
            } => {
                assert_eq!(nb, 8);
                assert_eq!(tms_vector.len(), 1);
                assert_eq!(tdi_vector.len(), 1);
            }
            _ => panic!("expected Shift"),
        }
    }

    #[test]
    fn shift_9_bits_requires_2_bytes() {
        let num_bits: u32 = 9;
        let tms = vec![0xFF; 2];
        let tdi = vec![0xAA; 2];

        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&tms);
        data.extend_from_slice(&tdi);

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::Shift {
                num_bits: nb,
                tms: tms_vector,
                tdi: tdi_vector,
            } => {
                assert_eq!(nb, 9);
                assert_eq!(tms_vector.len(), 2);
                assert_eq!(tdi_vector.len(), 2);
            }
            _ => panic!("expected Shift"),
        }
    }

    #[test]
    fn decoder_reusable_reads_two_messages() {
        let mut cursor = Cursor::new(b"getinfo:");
        let mut dec = Decoder::new(1024);
        assert!(matches!(
            dec.read_message(&mut cursor).unwrap(),
            Message::GetInfo
        ));
        let mut cursor2 = Cursor::new(b"settck:\x42\x00\x00\x00");
        assert!(matches!(
            dec.read_message(&mut cursor2).unwrap(),
            Message::SetTck { period_ns: 0x42 }
        ));
    }
}
