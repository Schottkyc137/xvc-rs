use core::{num::ParseIntError, str::Utf8Error};

use bytes::{Buf, Bytes};

use crate::{
    XvcCommand,
    error::ParseVersionError,
    protocol::{Version, XvcInfo},
};

const XVC_SERVER_PREFIX: &[u8] = b"xvcServer_v";

impl XvcInfo {
    pub fn parse<'a>(buf: &mut Bytes) -> ParseResult<XvcInfo> {
        let Some(newline_index) = buf.iter().position(|b| *b == b'\n') else {
            return Err(ParseErr::Incomplete)
        };
        let mut buf = buf.split_to(newline_index);
        if !buf.starts_with(XVC_SERVER_PREFIX) {
            return Err(ParseErr::InvalidCommand(buf))
        }
        buf.advance(XVC_SERVER_PREFIX.len());
        let colon_index = buf
            .iter()
            .position(|byte| *byte == b':')
            .ok_or(ParseErr::InvalidCommand(buf.clone()))?;
        let version_buf = buf.split_to(colon_index);
        let version = str::from_utf8(&version_buf)?.parse::<Version>()?;
        buf.advance(1); // Consume colon token
        let max_vector_len = str::from_utf8(&buf)?.parse::<u32>()?;
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
    InvalidCommand(Bytes),
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
    /// use xvc_protocol::{XvcCommand, codec::ParseErr};
    ///
    /// let buf = b"getinfo:";
    /// let (command, rest) = XvcCommand::parse(buf).expect("Parsing a large enough buffer should not fail");
    /// assert_eq!(command, XvcCommand::GetInfo);
    /// assert!(rest == 0);
    /// ```
    ///
    /// If the buffer is not large enough, `ParseErr::Incomplete` is returned.
    /// This usually indicates to the caller to allocate more space and read more bytes:
    ///
    /// ```
    /// use xvc_protocol::{XvcCommand, codec::ParseErr};
    ///
    /// let buf = b"getin";
    /// let result = XvcCommand::parse(buf);
    /// assert_eq!(result, Err(ParseErr::Incomplete));
    /// // ... get more buffer from a stream
    /// ```
    ///
    /// A buffer that is too large is permitted. On success, the function will return the portion
    /// of the buffer after the command:
    /// ```
    /// use xvc_protocol::{XvcCommand, codec::ParseErr};
    ///
    /// let buf = b"settck:\x64";
    /// let (command, rest) = XvcCommand::parse(buf).expect("Parsing a large enough buffer should not fail");
    /// assert_eq!(command, XvcCommand::SetTck);
    /// assert_eq!(rest, 1);
    /// ```
    pub fn parse<'a>(buf: &mut Bytes) -> ParseResult<XvcCommand> {
        let Some(position) = buf.iter().position(|byte| *byte == b':') else {
            return Err(ParseErr::Incomplete);
        };
        // Note: We use position + 1 to include the ':' character
        let command = buf.split_to(position + 1);
        match command.as_ref() {
            b"getinfo:" => Ok(XvcCommand::GetInfo),
            b"settck:" => Ok(XvcCommand::SetTck),
            b"shift:" => Ok(XvcCommand::Shift),
            _ => Err(ParseErr::InvalidCommand(command)),
        }
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
    pub fn parse<'a>(mut buf: impl Buf) -> ParseResult<Self> {
        if buf.remaining() < 4 {
            return Err(ParseErr::Incomplete)
        }
        let period = buf.get_u32_le();
        Ok(SetTck { period })
    }
}

pub struct Shift {
    num_bits: u32,
    tdi: Bytes,
    tms: Bytes,
}

impl Shift {
    pub fn num_bits(&self) -> u32 {
        self.num_bits
    }

    pub fn tdi(&self) -> &[u8] {
        &self.tdi
    }

    pub fn tms(&self) -> &[u8] {
        &self.tms
    }
}

impl Shift {
    pub fn parse_num_bits<'b>(buf: &mut Bytes) -> ParseResult<u32> {
        if buf.remaining() < 4 {
            return Err(ParseErr::Incomplete);
        }
        Ok(buf.get_u32_le())
    }

    /// This is mostly an internal convenience method when parsing the `Shift` command.
    /// However, it may also be used to avoid allocating two buffers in resource constrained systems,
    /// if the `tdi` buffer can be stored somewhere else.
    /// ```
    /// use xvc_protocol::codec::{ParseResult, Shift};
    ///
    /// let buf = [0xAA;64];
    ///
    /// let (tdi, rest) = Shift::parse_tdi_or_tms(&buf, 32, 32).expect("Parsing a large enough buffer should not fail");
    /// assert_eq!(tdi, [0xAAu8;32]);
    /// // Write the buffer to a JTAG device
    /// Shift::parse_tdi_or_tms(&buf, 32, 32);
    /// ```
    pub fn parse_tdi_or_tms<'a>(
        buf: &mut Bytes,
        num_bytes: usize,
        max_len: usize,
    ) -> ParseResult<Bytes> {
        if num_bytes > max_len {
            return Err(ParseErr::TooManyBytes {
                max: max_len,
                got: num_bytes,
            });
        }
        if buf.remaining() < num_bytes {
            return Err(ParseErr::Incomplete);
        }
        Ok(buf.split_off(num_bytes))
    }

    pub fn parse<'a>(buf: &mut Bytes, max_len: usize) -> ParseResult<Shift> {
        let num_bits = Self::parse_num_bits(buf)?;
        let num_bytes = num_bits.div_ceil(8) as usize;
        let tms = Self::parse_tdi_or_tms(buf, num_bytes, max_len)?;
        let tdi = Self::parse_tdi_or_tms(buf, num_bytes, max_len)?;
        // Total consumed bytes: bytes used to encode num_bits + bytes for tms + bytes for tdi
        Ok(Shift { num_bits, tdi, tms })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    #[test]
    fn parses_valid_xvc_info() {
        let mut info1 = Bytes::from_static(b"xvcServer_v1.0:4\n");
        assert_eq!(
            XvcInfo::parse(&mut info1),
            Ok(XvcInfo::new(Version::new(1, 0), 4))
        );

        let mut info2 = Bytes::from_static(b"xvcServer_v10.2:24\n");
        assert_eq!(
            XvcInfo::parse(&mut info2),
            Ok(XvcInfo::new(Version::new(10, 2), 24))
        );
    }

    #[test]
    fn xvc_info_incomplete_no_newline() {
        let mut buf = Bytes::from_static(b"xvcServer_v1.0:4"); // no newline
        assert!(matches!(XvcInfo::parse(&mut buf), Err(ParseErr::Incomplete)));
    }

    #[test]
    fn xvc_info_invalid_prefix() {
        let mut buf = Bytes::from_static(b"badprefix:1.0:4\n");
        let res = XvcInfo::parse(&mut buf);
        assert!(matches!(res, Err(ParseErr::InvalidCommand(_))));
    }

    #[test]
    fn xvc_info_missing_colon() {
        let mut buf = Bytes::from_static(b"xvcServer_v1.0\n");
        let res = XvcInfo::parse(&mut buf);
        assert!(matches!(res, Err(ParseErr::InvalidCommand(_))));
    }

    #[test]
    fn xvc_info_malformed_version() {
        // version contains non-digit component
        let mut buf = Bytes::from_static(b"xvcServer_v1.a:4\n");
        let res = XvcInfo::parse(&mut buf);
        assert!(matches!(res, Err(ParseErr::ParseVersionError(_))));
    }

    #[test]
    fn xvc_info_invalid_max_vector_len() {
        let mut buf = Bytes::from_static(b"xvcServer_v1.0:NaN\n");
        let res = XvcInfo::parse(&mut buf);
        assert!(matches!(res, Err(ParseErr::ParseIntError(_))));
    }

    #[test]
    fn xvc_command_parse_valid_and_rest() {
        let mut buf = Bytes::from_static(b"settck:\x64");
        let cmd = XvcCommand::parse(&mut buf).expect("should parse settck");
        assert_eq!(cmd, XvcCommand::SetTck);
        assert_eq!(buf, "\x64");
    }

    #[test]
    fn xvc_command_parse_incomplete() {
        let mut buf = Bytes::from_static(b"getin");
        assert!(matches!(XvcCommand::parse(&mut buf), Err(ParseErr::Incomplete)));
    }

    #[test]
    fn xvc_command_parse_invalid() {
        let mut buf = Bytes::from_static(b"unknown:");
        let res = XvcCommand::parse(&mut buf);
        assert!(matches!(res, Err(ParseErr::InvalidCommand(_))));
    }

    #[test]
    fn set_tck_parse_ok_and_incomplete() {
        let mut buf = Bytes::from_static(&[0x01u8, 0x00, 0x00, 0x00]);
        let set = SetTck::parse(&mut buf).expect("should parse period");
        assert_eq!(set.period(), 1);
        assert!(buf.is_empty());

        let mut short = Bytes::from_static(&[0u8, 0u8, 0u8]);
        assert!(matches!(SetTck::parse(&mut short), Err(ParseErr::Incomplete)));
    }

    #[test]
    fn shift_parse_num_bits_behaviour() {
        assert!(matches!(
            Shift::parse_num_bits(&mut Bytes::from_static(&[0u8, 0, 0])),
            Err(ParseErr::Incomplete)
        ));
        let mut v = Bytes::from_static(&[0x0Cu8, 0, 0, 0]); // 12 bits
        let num_bits = Shift::parse_num_bits(&mut v).expect("should parse num bits");
        assert_eq!(num_bits, 12);
        assert!(v.is_empty());
    }

    // #[test]
    // fn parse_tdi_or_tms_edge_cases() {
    //     // too many bytes
    //     let buf = Bytes::from_static(&[0u8; 4]);
    //     assert!(matches!(
    //         Shift::parse_tdi_or_tms(&mut buf, 5, 4),
    //         Err(ParseErr::TooManyBytes { .. })
    //     ));

    //     // incomplete
    //     assert_eq!(
    //         Shift::parse_tdi_or_tms(&mutbuf[1..], 2, 4),
    //         Err(ParseErr::Incomplete)
    //     );

    //     // ok
    //     let slice = Shift::parse_tdi_or_tms(&buf, 4, 4).expect("should parse all bytes");
    //     assert_eq!(count, 4);
    //     assert_eq!(slice, &buf[..4]);
    // }

    // #[test]
    // fn shift_parse_ok_and_consumed_count() {
    //     // num_bits = 12 -> num_bytes = 2
    //     let mut buf: Vec<u8> = Vec::new();
    //     buf.extend_from_slice(&12u32.to_le_bytes()); // 4 bytes num_bits
    //     let tms = [0xAAu8, 0xBB];
    //     let tdi = [0x11u8, 0x22];
    //     buf.extend_from_slice(&tms);
    //     buf.extend_from_slice(&tdi);

    //     let (shift, consumed) = Shift::parse(&buf, 4).expect("shift parse should succeed");
    //     assert_eq!(shift.num_bits(), 12);
    //     assert_eq!(shift.tdi(), &tdi);
    //     assert_eq!(shift.tms(), &tms);
    //     // Expected consumed bytes: 4 (num_bits) + 2 (tms) + 2 (tdi) = 8
    //     assert_eq!(consumed, 4 + 2 + 2);
    // }

    // #[test]
    // fn shift_parse_too_many_bytes_error() {
    //     // num_bits -> num_bytes larger than max_len
    //     let mut buf: Vec<u8> = Vec::new();
    //     buf.extend_from_slice(&16u32.to_le_bytes()); // 16 bits -> 2 bytes
    //     // Now request max_len = 1 so parse_tdi_or_tms fails
    //     buf.extend_from_slice(&[0u8, 0u8]);
    //     buf.extend_from_slice(&[0u8, 0u8]);

    //     let res = Shift::parse(&buf, 1);
    //     assert!(matches!(res, Err(ParseErr::TooManyBytes { .. })));
    // }
}
