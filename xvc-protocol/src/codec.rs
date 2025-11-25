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
            // TODO: could also be error (command is not valid)
            return Err(ParseErr::Incomplete);
        };
        let (mut buf, rest) = buf.split_at(pos);
        if !buf.starts_with(XVC_SERVER_PREFIX) {
            return Err(ParseErr::InvalidCommand(buf));
        }
        buf = &buf[XVC_SERVER_PREFIX.len()..];
        let colon_index = buf
            .iter()
            .position(|byte| *byte == b':')
            .ok_or_else(|| ParseErr::InvalidCommand(buf))?;
        let (version_buf, buf) = buf.split_at(colon_index);
        let version = str::from_utf8(version_buf)?.parse::<Version>()?;
        let max_vector_len = str::from_utf8(&buf[1..])?.parse::<u32>()?;
        Ok((XvcInfo::new(version, max_vector_len), &rest[1..]))
    }
}

#[test]
fn parses_valid_xvc_info() {
    assert_eq!(
        XvcInfo::parse(b"xvcServer_v1.0:4\n"),
        Ok((XvcInfo::new(Version::new(1, 0), 4), &[] as &[u8]))
    );
    assert_eq!(
        XvcInfo::parse(b"xvcServer_v10.2:24\n"),
        Ok((XvcInfo::new(Version::new(10, 2), 24), &[] as &[u8]))
    );
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

pub type ParseResult<'a, T> = std::result::Result<(T, &'a [u8]), ParseErr<'a>>;

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
    /// assert!(rest.is_empty());
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
    /// assert_eq!(rest, b"\x64");
    /// ```
    pub fn parse<'a>(buf: &'a [u8]) -> ParseResult<'a, XvcCommand> {
        let Some(position) = buf.iter().position(|byte| *byte == b':') else {
            // TODO: could also be error (no ':' in command found)
            return Err(ParseErr::Incomplete);
        };
        // Note: We use position + 1 to include the ':' character
        let (command, rhs) = buf.split_at(position + 1);
        match command {
            b"getinfo:" => ParseResult::Ok((XvcCommand::GetInfo, rhs)),
            b"settck:" => ParseResult::Ok((XvcCommand::SetTck, rhs)),
            b"shift:" => ParseResult::Ok((XvcCommand::Shift, rhs)),
            other => ParseResult::Err(ParseErr::InvalidCommand(other)),
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
            Ok((SetTck { period }, &buf[4..]))
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

impl<'a> Shift<'a> {
    pub fn parse_num_bits(buf: &'a [u8]) -> ParseResult<'a, u32> {
        if buf.len() < 4 {
            return Err(ParseErr::Incomplete);
        }
        Ok((u32::from_le_bytes(buf[..4].try_into().unwrap()), &buf[4..]))
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
    pub fn parse_tdi_or_tms(
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
        Ok(buf.split_at(num_bytes))
    }

    pub fn parse(buf: &'a [u8], max_len: usize) -> ParseResult<'a, Self> {
        let (num_bits, buf) = Self::parse_num_bits(buf)?;
        let num_bytes = num_bits.div_ceil(8) as usize;
        let (tdi, buf) = Self::parse_tdi_or_tms(buf, num_bytes, max_len)?;
        let (tms, buf) = Self::parse_tdi_or_tms(buf, num_bytes, max_len)?;
        Ok((Shift { num_bits, tdi, tms }, buf))
    }
}
