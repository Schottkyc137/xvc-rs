/// Read and write implementations for the protocol messages
use std::io::{self, Read, Write};

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
    buf: Vec<u8>,
    /// Limit on the internal buffer. Triggers `TooManyBytes` if exceeded.
    max_buf: usize,
    /// Per-vector limit for `Shift` payloads, enforced by the codec parser.
    max_shift: usize,
}

impl Decoder {
    /// Create a new decoder for reading protocol [`Message`]s.
    ///
    /// `max_shift` is the maximum number of bytes allowed for each of the TMS
    /// and TDI vectors in a `Shift` command. The internal buffer is sized to
    /// accommodate the full shift payload.
    pub fn new(max_shift: usize) -> Self {
        // Worst-case buffer during command parsing:
        // command prefix ("settck:" = 7 bytes) + 4-byte num_bits field
        // + two shift vectors. Padded to 16 for simplicity.
        let max_buf = max_shift.saturating_mul(2).saturating_add(16);
        Self {
            buf: Vec::new(),
            max_buf,
            max_shift,
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
            // EOF with partial data or on an empty buffer — either way unexpected.
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected EOF while reading",
            )
            .into());
        }

        if self.max_buf < read + self.buf.len() {
            return Err(ReadError::TooManyBytes {
                max: self.max_buf,
                need: read + self.buf.len(),
            });
        }
        self.buf.extend_from_slice(&temp[..read]);

        Ok(())
    }

    /// Read an `XvcInfo` frame from `reader`.
    ///
    /// This method incrementally fills the internal buffer from `reader` until
    /// a complete XVC server info frame is available and returns the parsed
    /// `XvcInfo`. If EOF is encountered with partial data buffered, a
    /// `ReadError::InvalidCommand` is returned.
    pub fn read_xvc_info(&mut self, reader: &mut impl Read) -> Result<XvcInfo, ReadError> {
        self.buf.clear();
        loop {
            let mut slice: &[u8] = &self.buf;
            match XvcInfo::parse(&mut slice) {
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
    /// command present, a `ReadError::InvalidCommand` is returned.
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
            let mut slice: &[u8] = &self.buf;
            match XvcCommand::parse(&mut slice) {
                Ok(cmd) => {
                    let consumed = self.buf.len() - slice.len();
                    self.buf.drain(..consumed);
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
                let mut slice: &[u8] = &self.buf;
                match SetTck::parse(&mut slice) {
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
                let mut slice: &[u8] = &self.buf;
                match Shift::parse(&mut slice, self.max_shift) {
                    Ok(shift) => {
                        let num_bits = shift.num_bits();
                        let (tms, tdi) = shift.into_tms_tdi();
                        return Ok(Message::Shift { num_bits, tms, tdi });
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
    /// Example:
    ///
    /// ```rust
    /// use std::io::Cursor;
    /// let mut c = Cursor::new(b"xvcServer_v1.0:32\n");
    /// let info = xvc_protocol::XvcInfo::from_reader(&mut c).unwrap();
    /// assert_eq!(info.max_vector_len(), 32);
    /// ```
    pub fn from_reader(reader: &mut impl Read) -> Result<XvcInfo, ReadError> {
        Decoder::new(4096).read_xvc_info(reader)
    }
}

impl Message {
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
        use crate::codec::{CMD_GET_INFO, CMD_SET_TCK, CMD_SHIFT};
        match self {
            Message::GetInfo => writer.write_all(CMD_GET_INFO),
            Message::SetTck {
                period_ns: period_in_ns,
            } => {
                writer.write_all(CMD_SET_TCK)?;
                writer.write_all(&period_in_ns.to_le_bytes())
            }
            Message::Shift {
                num_bits,
                tms: tms_vector,
                tdi: tdi_vector,
            } => {
                writer.write_all(CMD_SHIFT)?;
                writer.write_all(&num_bits.to_le_bytes())?;
                writer.write_all(tms_vector)?;
                writer.write_all(tdi_vector)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::{io, io::Cursor, vec};

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
        let info = XvcInfo::from_reader(&mut cursor).unwrap();
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
            tms: tms.clone(),
            tdi: tdi.clone(),
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
            Err(ReadError::InvalidCommand(p)) => assert_eq!(p, "xx"),
            other => panic!("expected InvalidCommand, got {:?}", other),
        }
    }

    #[test]
    fn too_many_bytes_shift() {
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
        let info = XvcInfo::from_reader(&mut cursor).unwrap();
        assert_eq!(info.version(), crate::protocol::Version::V1_0);
        assert_eq!(info.max_vector_len(), u32::MAX);
    }

    #[test]
    fn read_xvc_info_with_zero_vector_len() {
        let data = b"xvcServer_v1.0:0\n";
        let mut cursor = Cursor::new(data);
        let info = XvcInfo::from_reader(&mut cursor).unwrap();
        assert_eq!(info.version(), crate::protocol::Version::V1_0);
        assert_eq!(info.max_vector_len(), 0);
    }

    #[test]
    fn read_xvc_info_with_large_version_numbers() {
        let data = b"xvcServer_v999.999:1024\n";
        let mut cursor = Cursor::new(data);
        let info = XvcInfo::from_reader(&mut cursor).unwrap();
        assert_eq!(info.version(), crate::protocol::Version::new(999, 999));
        assert_eq!(info.max_vector_len(), 1024);
    }

    #[test]
    fn read_xvc_info_incomplete_then_complete() {
        let data = b"xvcServer_v1.0:4\n";
        let mut cursor = Cursor::new(data);
        match XvcInfo::from_reader(&mut cursor) {
            Ok(info) => assert_eq!(info.max_vector_len(), 4),
            Err(e) => panic!("unexpected error: {:?}", e),
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
        // Command parsed successfully but stream ends before the 4-byte payload
        let data = b"settck:".to_vec();
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::IoError(_)) => {}
            other => panic!("expected IoError (unexpected EOF), got {:?}", other),
        }
    }

    #[test]
    fn read_settck_partial_period() {
        let mut data = b"settck:".to_vec();
        data.extend_from_slice(&[0xAA, 0xBB]);
        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            other => panic!("expected UnexpectedEof, got {:?}", other),
        }
    }

    #[test]
    fn read_shift_zero_bits() {
        let num_bits: u32 = 0;
        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap() {
            Message::Shift {
                num_bits: nb,
                tms: tms_vector,
                tdi: tdi_vector,
            } => {
                assert_eq!(nb, 0);
                assert_eq!(&*tms_vector, &[] as &[u8]);
                assert_eq!(&*tdi_vector, &[] as &[u8]);
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
            Err(ReadError::TooManyBytes { .. }) => {}
            other => panic!("expected TooManyBytes, got {:?}", other),
        }
    }

    #[test]
    fn read_shift_incomplete_num_bits() {
        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&[0xAA, 0xBB]);

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            other => panic!("expected UnexpectedEof, got {:?}", other),
        }
    }

    #[test]
    fn read_shift_incomplete_tms() {
        let num_bits: u32 = 16;
        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&[0xAA]);

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            other => panic!("expected UnexpectedEof, got {:?}", other),
        }
    }

    #[test]
    fn read_shift_incomplete_tdi() {
        let num_bits: u32 = 8;
        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&[0xAA]); // TMS only

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES) {
            Err(ReadError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            other => panic!("expected UnexpectedEof, got {:?}", other),
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
    fn shift_at_exact_max_vector_size_succeeds() {
        // Vectors that are exactly max_shift bytes must be accepted.
        let max_shift = 4;
        let num_bytes = 4usize;
        let num_bits: u32 = (num_bytes * 8) as u32;

        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&[0xAA; 4]);
        data.extend_from_slice(&[0x55; 4]);

        let mut cursor = Cursor::new(data);
        assert!(
            Message::from_reader(&mut cursor, max_shift).is_ok(),
            "vectors exactly at max_shift should be accepted"
        );
    }

    #[test]
    fn shift_over_max_vector_size_fails() {
        // Vectors one byte over max_shift must be rejected with TooManyBytes.
        let max_shift = 4;
        let num_bytes = 5usize;
        let num_bits: u32 = (num_bytes * 8) as u32;

        let mut data = b"shift:".to_vec();
        data.extend_from_slice(&num_bits.to_le_bytes());
        data.extend_from_slice(&[0xAA; 5]);
        data.extend_from_slice(&[0x55; 5]);

        let mut cursor = Cursor::new(data);
        match Message::from_reader(&mut cursor, max_shift) {
            Err(ReadError::TooManyBytes { .. }) => {}
            other => panic!("expected TooManyBytes, got {:?}", other),
        }
    }

    #[test]
    fn write_shift_zero_bits() {
        let cmd = Message::Shift {
            num_bits: 0,
            tms: Box::new([]),
            tdi: Box::new([]),
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
            tms: Box::new([0xFF; 512]),
            tdi: Box::new([0xAA; 512]),
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
            Err(ReadError::InvalidCommand(_)) => {}
            other => panic!("expected InvalidCommand, got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_xvc_info() {
        let original = XvcInfo::new(crate::protocol::Version::new(1, 0), 8192);
        let mut buffer = Vec::new();
        original.write_to(&mut buffer).unwrap();

        let mut cursor = Cursor::new(buffer);
        let parsed = XvcInfo::from_reader(&mut cursor).unwrap();

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
            tms: vec![0xAA; num_bytes].into_boxed_slice(),
            tdi: vec![0x55; num_bytes].into_boxed_slice(),
        };
        let mut buffer = Vec::new();
        original.write_to(&mut buffer).unwrap();

        let mut cursor = Cursor::new(buffer);
        let parsed = Message::from_reader(&mut cursor, DEFAULT_MAX_SHIFT_BYTES).unwrap();

        assert_eq!(parsed, original);
    }

    #[test]
    fn shift_num_bits_rounding() {
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
