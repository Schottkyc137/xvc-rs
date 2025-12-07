use core::{num::ParseIntError, str::Utf8Error};

use crate::{
    XvcCommand,
    error::ParseVersionError,
    protocol::{Version, XvcInfo},
};

const XVC_SERVER_PREFIX: &[u8] = b"xvcServer_v";

impl XvcInfo {
    pub fn parse<'a>(buf: &'a [u8]) -> ParseResult<'a, XvcInfo> {
        let Some(pos) = buf.iter().position(|byte| *byte == b'\n') else {
            todo!("Correct error recovery")
        };
        let mut buf = &buf[..pos];
        if !buf.starts_with(XVC_SERVER_PREFIX) {
            return Err(ParseErr::InvalidCommand(buf));
        }
        buf = &buf[XVC_SERVER_PREFIX.len()..];
        let colon_index = buf
            .iter()
            .position(|byte| *byte == b':')
            .ok_or(ParseErr::InvalidCommand(buf))?;
        let (version_buf, buf) = buf.split_at(colon_index);
        let version = str::from_utf8(version_buf)?.parse::<Version>()?;
        let max_vector_len = str::from_utf8(&buf[1..])?.parse::<u32>()?;
        // Return the number of bytes consumed including the trailing newline
        Ok((XvcInfo::new(version, max_vector_len), pos + 1))
    }
}

/// Errors that happen while parsing a command.
/// Note that `ParseErr::Incomplete` is usually used to indicate
/// upstream that it should increase the buffer size and re-try.
#[derive(Eq, PartialEq, Clone, Debug)]
pub enum ParseErr<'a> {
    /// The buffer is too small to parse the message.
    Incomplete,
    /// A command was recognized, but it does not match any known command.
    InvalidCommand(&'a [u8]),
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

impl<'a> From<Utf8Error> for ParseErr<'a> {
    fn from(value: Utf8Error) -> Self {
        ParseErr::Utf8Error(value)
    }
}

impl<'a> From<ParseIntError> for ParseErr<'a> {
    fn from(value: ParseIntError) -> Self {
        ParseErr::ParseIntError(value)
    }
}

impl<'a> From<ParseVersionError> for ParseErr<'a> {
    fn from(value: ParseVersionError) -> Self {
        ParseErr::ParseVersionError(value)
    }
}

pub type ParseResult<'a, T> = core::result::Result<(T, usize), ParseErr<'a>>;

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
    pub fn parse<'a>(buf: &'a [u8]) -> ParseResult<'a, XvcCommand> {
        let Some(position) = buf.iter().position(|byte| *byte == b':') else {
            todo!("Correct error recovery")
        };
        // Note: We use position + 1 to include the ':' character
        let (command, _) = buf.split_at(position + 1);
        match command {
            b"getinfo:" => Ok((XvcCommand::GetInfo, command.len())),
            b"settck:" => Ok((XvcCommand::SetTck, command.len())),
            b"shift:" => Ok((XvcCommand::Shift, command.len())),
            other => Err(ParseErr::InvalidCommand(other)),
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
    pub fn parse<'a>(buf: &'a [u8]) -> ParseResult<'a, Self> {
        if buf.len() < 4 {
            Err(ParseErr::Incomplete)
        } else {
            let period = u32::from_le_bytes(buf[..4].try_into().unwrap());
            Ok((SetTck { period }, 4))
        }
    }
}

pub struct Shift<'a> {
    num_bits: u32,
    tdi: &'a [u8],
    tms: &'a [u8],
}

impl<'a> Shift<'a> {
    pub fn num_bits(&self) -> u32 {
        self.num_bits
    }

    pub fn tdi(&self) -> &'a [u8] {
        self.tdi
    }

    pub fn tms(&self) -> &'a [u8] {
        self.tms
    }
}

impl Shift<'_> {
    pub fn parse_num_bits<'b>(buf: &'b [u8]) -> ParseResult<'b, u32> {
        if buf.len() < 4 {
            return Err(ParseErr::Incomplete);
        }
        Ok((u32::from_le_bytes(buf[..4].try_into().unwrap()), 4))
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
        buf: &'a [u8],
        num_bytes: usize,
        max_len: usize,
    ) -> ParseResult<'a, &'a [u8]> {
        if num_bytes > max_len {
            return Err(ParseErr::TooManyBytes {
                max: max_len,
                got: num_bytes,
            });
        }
        if buf.len() < num_bytes {
            return Err(ParseErr::Incomplete);
        }
        Ok((&buf[..num_bytes], num_bytes))
    }

    pub fn parse<'a>(buf: &'a [u8], max_len: usize) -> ParseResult<'a, Shift<'a>> {
        let (num_bits, nb_count) = Self::parse_num_bits(buf)?;
        let num_bytes = num_bits.div_ceil(8) as usize;
        let buf = &buf[nb_count..];
        let (tms, tms_count) = Self::parse_tdi_or_tms(&buf, num_bytes, max_len)?;
        let buf = &buf[tms_count..];
        let (tdi, tdi_count) = Self::parse_tdi_or_tms(&buf, num_bytes, max_len)?;
        // Total consumed bytes: bytes used to encode num_bits + bytes for tms + bytes for tdi
        let consumed = nb_count + tms_count + tdi_count;
        Ok((Shift { num_bits, tdi, tms }, consumed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    #[test]
    fn parses_valid_xvc_info() {
        let info1 = b"xvcServer_v1.0:4\n";
        assert_eq!(
            XvcInfo::parse(info1),
            Ok((XvcInfo::new(Version::new(1, 0), 4), info1.len()))
        );

        let info2 = b"xvcServer_v10.2:24\n";
        assert_eq!(
            XvcInfo::parse(b"xvcServer_v10.2:24\n"),
            Ok((XvcInfo::new(Version::new(10, 2), 24), info2.len()))
        );
    }

    #[test]
    fn xvc_info_incomplete_no_newline() {
        let buf = b"xvcServer_v1.0:4"; // no newline
        assert!(matches!(XvcInfo::parse(buf), Err(ParseErr::Incomplete)));
    }

    #[test]
    fn xvc_info_invalid_prefix() {
        let buf = b"badprefix:1.0:4\n";
        let res = XvcInfo::parse(buf);
        assert!(matches!(res, Err(ParseErr::InvalidCommand(_))));
    }

    #[test]
    fn xvc_info_missing_colon() {
        let buf = b"xvcServer_v1.0\n";
        let res = XvcInfo::parse(buf);
        assert!(matches!(res, Err(ParseErr::InvalidCommand(_))));
    }

    #[test]
    fn xvc_info_malformed_version() {
        // version contains non-digit component
        let buf = b"xvcServer_v1.a:4\n";
        let res = XvcInfo::parse(buf);
        assert!(matches!(res, Err(ParseErr::ParseVersionError(_))));
    }

    #[test]
    fn xvc_info_invalid_max_vector_len() {
        let buf = b"xvcServer_v1.0:NaN\n";
        let res = XvcInfo::parse(buf);
        assert!(matches!(res, Err(ParseErr::ParseIntError(_))));
    }

    #[test]
    fn xvc_command_parse_valid_and_rest() {
        let buf = b"settck:\x64";
        let (cmd, consumed) = XvcCommand::parse(buf).expect("should parse settck");
        assert_eq!(cmd, XvcCommand::SetTck);
        assert_eq!(consumed, b"settck:".len());
    }

    #[test]
    fn xvc_command_parse_incomplete() {
        let buf = b"getin";
        assert!(matches!(XvcCommand::parse(buf), Err(ParseErr::Incomplete)));
    }

    #[test]
    fn xvc_command_parse_invalid() {
        let buf = b"unknown:";
        let res = XvcCommand::parse(buf);
        assert!(matches!(res, Err(ParseErr::InvalidCommand(_))));
    }

    #[test]
    fn set_tck_parse_ok_and_incomplete() {
        let buf = [0x01u8, 0x00, 0x00, 0x00];
        let (set, count) = SetTck::parse(&buf).expect("should parse period");
        assert_eq!(set.period(), 1);
        assert_eq!(count, 4);

        let short = [0u8, 0u8, 0u8];
        assert!(matches!(SetTck::parse(&short), Err(ParseErr::Incomplete)));
    }

    #[test]
    fn shift_parse_num_bits_behaviour() {
        assert!(matches!(
            Shift::parse_num_bits(&[0u8, 0, 0]),
            Err(ParseErr::Incomplete)
        ));
        let v = [0x0Cu8, 0, 0, 0]; // 12 bits
        let (num_bits, count) = Shift::parse_num_bits(&v).expect("should parse num bits");
        assert_eq!(num_bits, 12);
        assert_eq!(count, 4);
    }

    #[test]
    fn parse_tdi_or_tms_edge_cases() {
        // too many bytes
        let buf = [0u8; 4];
        assert!(matches!(
            Shift::parse_tdi_or_tms(&buf, 5, 4),
            Err(ParseErr::TooManyBytes { .. })
        ));

        // incomplete
        assert_eq!(
            Shift::parse_tdi_or_tms(&buf[..1], 2, 4),
            Err(ParseErr::Incomplete)
        );

        // ok
        let (slice, count) = Shift::parse_tdi_or_tms(&buf, 4, 4).expect("should parse all bytes");
        assert_eq!(count, 4);
        assert_eq!(slice, &buf[..4]);
    }

    #[test]
    fn shift_parse_ok_and_consumed_count() {
        // num_bits = 12 -> num_bytes = 2
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&12u32.to_le_bytes()); // 4 bytes num_bits
        let tms = [0xAAu8, 0xBB];
        let tdi = [0x11u8, 0x22];
        buf.extend_from_slice(&tms);
        buf.extend_from_slice(&tdi);

        let (shift, consumed) = Shift::parse(&buf, 4).expect("shift parse should succeed");
        assert_eq!(shift.num_bits(), 12);
        assert_eq!(shift.tdi(), &tdi);
        assert_eq!(shift.tms(), &tms);
        // Expected consumed bytes: 4 (num_bits) + 2 (tms) + 2 (tdi) = 8
        assert_eq!(consumed, 4 + 2 + 2);
    }

    #[test]
    fn shift_parse_too_many_bytes_error() {
        // num_bits -> num_bytes larger than max_len
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&16u32.to_le_bytes()); // 16 bits -> 2 bytes
        // Now request max_len = 1 so parse_tdi_or_tms fails
        buf.extend_from_slice(&[0u8, 0u8]);
        buf.extend_from_slice(&[0u8, 0u8]);

        let res = Shift::parse(&buf, 1);
        assert!(matches!(res, Err(ParseErr::TooManyBytes { .. })));
    }
}
