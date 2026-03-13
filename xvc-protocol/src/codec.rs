use std::{num::ParseIntError, str::Utf8Error};

use crate::{
    XvcCommand,
    error::ParseVersionError,
    protocol::{Version, XvcInfo},
};

const XVC_SERVER_PREFIX: &[u8] = b"xvcServer_v";

pub(crate) const CMD_GET_INFO: &[u8] = b"getinfo:";
pub(crate) const CMD_SET_TCK: &[u8] = b"settck:";
pub(crate) const CMD_SHIFT: &[u8] = b"shift:";

/// A lightweight cursor over a borrowed byte slice.
struct SliceReader<'a>(&'a [u8]);

impl<'a> SliceReader<'a> {
    fn remaining(&self) -> usize {
        self.0.len()
    }

    fn advance(&mut self, n: usize) {
        self.0 = &self.0[n..];
    }

    fn get_u32_le(&mut self) -> u32 {
        let v = u32::from_le_bytes([self.0[0], self.0[1], self.0[2], self.0[3]]);
        self.advance(4);
        v
    }

    fn copy_to_boxed_slice(&mut self, n: usize) -> Box<[u8]> {
        let out: Box<[u8]> = self.0[..n].into();
        self.advance(n);
        out
    }
}

impl XvcInfo {
    pub fn parse(buf: &mut &[u8]) -> ParseResult<XvcInfo> {
        let Some(newline_index) = buf.iter().position(|b| *b == b'\n') else {
            return Err(ParseErr::Incomplete);
        };
        let line = &buf[..newline_index];
        *buf = &buf[newline_index + 1..];
        let rest = line
            .strip_prefix(XVC_SERVER_PREFIX)
            .ok_or_else(|| ParseErr::InvalidCommand(line.into()))?;
        let colon_index = rest
            .iter()
            .position(|byte| *byte == b':')
            .ok_or_else(|| ParseErr::InvalidCommand(line.into()))?;
        let version = core::str::from_utf8(&rest[..colon_index])?.parse::<Version>()?;
        let max_vector_len = core::str::from_utf8(&rest[colon_index + 1..])?.parse::<u32>()?;
        Ok(XvcInfo::new(version, max_vector_len))
    }
}

/// Errors that happen while parsing a command.
/// Note that `ParseErr::Incomplete` is usually used to indicate
/// upstream that it should increase the buffer size and re-try.
#[derive(Eq, PartialEq, Clone, Debug)]
pub enum ParseErr {
    /// The buffer is too small to parse the message.
    Incomplete,
    /// A command was recognized, but it does not match any known command.
    InvalidCommand(Box<[u8]>),
    /// A command requested more size than is available.
    /// This can happen in the `Shift` command when the passed `tdi` or `tms`
    /// vectors are larger than the maximum size negotiated in the beginning.
    TooManyBytes { max: usize, got: usize },
    /// Conversion to UTF-8 failed
    Utf8Error(Utf8Error),
    /// Parsing an integer failed
    ParseIntError(ParseIntError),
    /// Parsing a version failed
    ParseVersionError(ParseVersionError),
}

impl From<Utf8Error> for ParseErr {
    fn from(value: Utf8Error) -> Self {
        ParseErr::Utf8Error(value)
    }
}

impl From<ParseIntError> for ParseErr {
    fn from(value: ParseIntError) -> Self {
        ParseErr::ParseIntError(value)
    }
}

impl From<ParseVersionError> for ParseErr {
    fn from(value: ParseVersionError) -> Self {
        ParseErr::ParseVersionError(value)
    }
}

pub type ParseResult<T> = core::result::Result<T, ParseErr>;

impl XvcCommand {
    /// Parse a command from a buffer.
    ///
    /// # Example
    /// ```
    /// use xvc_protocol::{XvcCommand};
    ///
    /// let mut buf: &[u8] = b"getinfo:";
    /// let command = XvcCommand::parse(&mut buf).expect("Parsing a large enough buffer should not fail");
    /// assert_eq!(command, XvcCommand::GetInfo);
    /// assert_eq!(buf.len(), 0);
    /// ```
    ///
    /// If the buffer is not large enough, `ParseErr::Incomplete` is returned.
    /// This usually indicates to the caller to read more bytes into the buffer:
    ///
    /// A buffer that is too large is permitted. After a successful parse the
    /// buffer is advanced past the consumed command bytes:
    /// ```
    /// use xvc_protocol::XvcCommand;
    ///
    /// let mut buf: &[u8] = b"settck:\x64";
    /// let command = XvcCommand::parse(&mut buf).expect("Parsing a large enough buffer should not fail");
    /// assert_eq!(command, XvcCommand::SetTck);
    /// assert_eq!(buf.len(), 1);
    /// ```
    pub fn parse(buf: &mut &[u8]) -> ParseResult<XvcCommand> {
        let (cmd, n) = if buf.starts_with(CMD_GET_INFO) {
            (XvcCommand::GetInfo, CMD_GET_INFO.len())
        } else if buf.starts_with(CMD_SET_TCK) {
            (XvcCommand::SetTck, CMD_SET_TCK.len())
        } else if buf.starts_with(CMD_SHIFT) {
            (XvcCommand::Shift, CMD_SHIFT.len())
        } else {
            return if CMD_GET_INFO.starts_with(buf)
                || CMD_SET_TCK.starts_with(buf)
                || CMD_SHIFT.starts_with(buf)
            {
                Err(ParseErr::Incomplete)
            } else {
                Err(ParseErr::InvalidCommand((*buf).into()))
            };
        };
        *buf = &buf[n..];
        Ok(cmd)
    }
}

pub struct SetTck {
    period: u32,
}

impl SetTck {
    pub fn period(&self) -> u32 {
        self.period
    }
}

impl SetTck {
    pub fn parse(buf: &mut &[u8]) -> ParseResult<Self> {
        let mut r = SliceReader(buf);
        if r.remaining() < 4 {
            return Err(ParseErr::Incomplete);
        }
        let period = r.get_u32_le();
        *buf = r.0;
        Ok(SetTck { period })
    }
}

pub struct Shift {
    num_bits: u32,
    tdi: Box<[u8]>,
    tms: Box<[u8]>,
}

impl Shift {
    pub fn num_bits(&self) -> u32 {
        self.num_bits
    }

    #[cfg(test)]
    pub fn tdi(&self) -> &[u8] {
        &self.tdi
    }

    #[cfg(test)]
    pub fn tms(&self) -> &[u8] {
        &self.tms
    }

    pub fn into_tms_tdi(self) -> (Box<[u8]>, Box<[u8]>) {
        (self.tms, self.tdi)
    }
}

impl Shift {
    pub fn parse_num_bits(buf: &mut &[u8]) -> ParseResult<u32> {
        let mut r = SliceReader(buf);
        if r.remaining() < 4 {
            return Err(ParseErr::Incomplete);
        }
        let n = r.get_u32_le();
        *buf = r.0;
        Ok(n)
    }

    /// This is an internal convenience method when parsing the `Shift` command.
    pub fn parse_tdi_or_tms(
        buf: &mut &[u8],
        num_bytes: usize,
        max_len: usize,
    ) -> ParseResult<Box<[u8]>> {
        if num_bytes > max_len {
            return Err(ParseErr::TooManyBytes {
                max: max_len,
                got: num_bytes,
            });
        }
        let mut r = SliceReader(buf);
        if r.remaining() < num_bytes {
            return Err(ParseErr::Incomplete);
        }
        let out = r.copy_to_boxed_slice(num_bytes);
        *buf = r.0;
        Ok(out)
    }

    pub fn parse(buf: &mut &[u8], max_len: usize) -> ParseResult<Shift> {
        let num_bits = Self::parse_num_bits(buf)?;
        let num_bytes = num_bits.div_ceil(8) as usize;
        let tms = Self::parse_tdi_or_tms(buf, num_bytes, max_len)?;
        let tdi = Self::parse_tdi_or_tms(buf, num_bytes, max_len)?;
        Ok(Shift { num_bits, tdi, tms })
    }
}

#[cfg(test)]
mod tests {
    use std::vec::Vec;

    use super::*;

    #[test]
    fn parses_valid_xvc_info() {
        let mut info1: &[u8] = b"xvcServer_v1.0:4\n";
        assert_eq!(
            XvcInfo::parse(&mut info1),
            Ok(XvcInfo::new(Version::new(1, 0), 4))
        );

        let mut info2: &[u8] = b"xvcServer_v10.2:24\n";
        assert_eq!(
            XvcInfo::parse(&mut info2),
            Ok(XvcInfo::new(Version::new(10, 2), 24))
        );
    }

    #[test]
    fn xvc_info_incomplete_no_newline() {
        let mut buf: &[u8] = b"xvcServer_v1.0:4"; // no newline
        assert!(matches!(
            XvcInfo::parse(&mut buf),
            Err(ParseErr::Incomplete)
        ));
    }

    #[test]
    fn xvc_info_invalid_prefix() {
        let mut buf: &[u8] = b"badprefix:1.0:4\n";
        assert!(matches!(
            XvcInfo::parse(&mut buf),
            Err(ParseErr::InvalidCommand(_))
        ));
    }

    #[test]
    fn xvc_info_missing_colon() {
        let mut buf: &[u8] = b"xvcServer_v1.0\n";
        assert!(matches!(
            XvcInfo::parse(&mut buf),
            Err(ParseErr::InvalidCommand(_))
        ));
    }

    #[test]
    fn xvc_info_malformed_version() {
        let mut buf: &[u8] = b"xvcServer_v1.a:4\n";
        assert!(matches!(
            XvcInfo::parse(&mut buf),
            Err(ParseErr::ParseVersionError(_))
        ));
    }

    #[test]
    fn xvc_info_invalid_max_vector_len() {
        let mut buf: &[u8] = b"xvcServer_v1.0:NaN\n";
        assert!(matches!(
            XvcInfo::parse(&mut buf),
            Err(ParseErr::ParseIntError(_))
        ));
    }

    #[test]
    fn xvc_command_parse_valid_and_rest() {
        let mut buf: &[u8] = b"settck:\x64";
        let cmd = XvcCommand::parse(&mut buf).expect("should parse settck");
        assert_eq!(cmd, XvcCommand::SetTck);
        assert_eq!(buf, b"\x64");
    }

    #[test]
    fn xvc_command_parse_incomplete() {
        let mut buf: &[u8] = b"getin";
        assert!(matches!(
            XvcCommand::parse(&mut buf),
            Err(ParseErr::Incomplete)
        ));
    }

    #[test]
    fn xvc_command_parse_invalid() {
        let mut buf: &[u8] = b"unknown:";
        assert!(matches!(
            XvcCommand::parse(&mut buf),
            Err(ParseErr::InvalidCommand(_))
        ));
    }

    #[test]
    fn set_tck_parse_ok_and_incomplete() {
        let mut buf: &[u8] = &[0x01u8, 0x00, 0x00, 0x00];
        let set = SetTck::parse(&mut buf).expect("should parse period");
        assert_eq!(set.period(), 1);
        assert!(buf.is_empty());

        let mut short: &[u8] = &[0u8, 0u8, 0u8];
        assert!(matches!(
            SetTck::parse(&mut short),
            Err(ParseErr::Incomplete)
        ));
    }

    #[test]
    fn shift_parse_num_bits_behaviour() {
        let mut short: &[u8] = &[0u8, 0, 0];
        assert!(matches!(
            Shift::parse_num_bits(&mut short),
            Err(ParseErr::Incomplete)
        ));
        let mut v: &[u8] = &[0x0Cu8, 0, 0, 0]; // 12 bits
        let num_bits = Shift::parse_num_bits(&mut v).expect("should parse num bits");
        assert_eq!(num_bits, 12);
        assert!(v.is_empty());
    }

    #[test]
    fn parse_tdi_or_tms_edge_cases() {
        let mut buf: &[u8] = &[0u8; 4];
        assert!(matches!(
            Shift::parse_tdi_or_tms(&mut buf, 5, 4),
            Err(ParseErr::TooManyBytes { .. })
        ));

        let mut buf: &[u8] = &[0u8; 1];
        assert!(matches!(
            Shift::parse_tdi_or_tms(&mut buf, 2, 4),
            Err(ParseErr::Incomplete)
        ));

        let mut buf: &[u8] = &[0xAAu8; 4];
        let slice = Shift::parse_tdi_or_tms(&mut buf, 4, 4).expect("should parse all bytes");
        assert_eq!(&slice[..], &[0xAAu8; 4]);
        assert!(buf.is_empty());
    }

    #[test]
    fn shift_parse_ok() {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&12u32.to_le_bytes());
        let tms = [0xAAu8, 0xBB];
        let tdi = [0x11u8, 0x22];
        buf.extend_from_slice(&tms);
        buf.extend_from_slice(&tdi);

        let mut slice: &[u8] = &buf;
        let shift = Shift::parse(&mut slice, 4).expect("shift parse should succeed");
        assert_eq!(shift.num_bits(), 12);
        assert_eq!(shift.tms(), &tms);
        assert_eq!(shift.tdi(), &tdi);
        assert!(slice.is_empty());
    }

    #[test]
    fn shift_parse_too_many_bytes_error() {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&16u32.to_le_bytes()); // 16 bits -> 2 bytes each
        buf.extend_from_slice(&[0u8, 0u8]);
        buf.extend_from_slice(&[0u8, 0u8]);

        let mut slice: &[u8] = &buf;
        assert!(matches!(
            Shift::parse(&mut slice, 1),
            Err(ParseErr::TooManyBytes { .. })
        ));
    }
}
