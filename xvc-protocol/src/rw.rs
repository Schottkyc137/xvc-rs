/// Read and write implementations for the protocol messages
use std::{
    io::{self, Read, Write},
    prelude::v1::*,
};

use crate::{
    Message, XvcCommand, XvcInfo,
    codec::{ParseErr, SetTck, Shift},
    error::ReadError,
};

pub struct Decoder {
    buf: Vec<u8>,
    max: usize,
}

impl Decoder {
    /// Create a new decoder with a given initial capacity and max size.
    pub fn new(max_size: usize) -> Self {
        Self {
            buf: Vec::new(),
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
            // EOF: if incomplete, it's a protocol error
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "unexpected EOF while reading",
            )
            .into());
        }

        if self.max < read + self.buf.len() {
            return Err(ReadError::TooManyBytes {
                max: self.max,
                got: read + self.buf.len(),
            });
        }
        self.buf.extend_from_slice(&temp);

        Ok(())
    }

    pub fn read_xvc_info(&mut self, reader: &mut impl Read) -> Result<XvcInfo, ReadError> {
        loop {
            match XvcInfo::parse(&self.buf) {
                Ok((frame, _)) => {
                    return Ok(frame);
                }
                Err(ParseErr::Incomplete) => {
                    self.read_chunk(reader)?;
                }
                Err(other) => return Err(other.into()),
            }
        }
    }

    pub fn read_message(&mut self, reader: &mut impl Read) -> Result<Message, ReadError> {
        let (cmd, size) = loop {
            match XvcCommand::parse(&self.buf) {
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
                match SetTck::parse(&self.buf[size..]) {
                    Ok((tck, _)) => {
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
                match Shift::parse(&self.buf[size..], self.max) {
                    Ok((shift, _)) => {
                        return Ok(Message::Shift {
                            num_bits: shift.num_bits(),
                            tms: shift.tms().into(),
                            tdi: shift.tdi().into(),
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
    pub fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        writeln!(
            writer,
            "xvcServer_v{}:{}",
            self.version(),
            self.max_vector_len()
        )
    }

    pub fn from_reader(
        reader: &mut impl Read,
        max_buffer_size: usize,
    ) -> Result<XvcInfo, ReadError> {
        Decoder::new(max_buffer_size).read_xvc_info(reader)
    }
}

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

impl Message {
    const CMD_NAME_GET_INFO: &'static [u8; 7] = b"getinfo";
    const CMD_NAME_SET_TCK: &'static [u8; 6] = b"settck";
    const CMD_NAME_SHIFT: &'static [u8; 5] = b"shift";
    const CMD_DELIMITER: u8 = b':';

    pub fn from_reader(
        reader: &mut impl Read,
        max_shift_bytes: usize,
    ) -> Result<Message, ReadError> {
        Decoder::new(max_shift_bytes).read_message(reader)
    }

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
            Err(ReadError::TooManyBytes { max, got }) => {
                assert_eq!(max, 1024);
                assert_eq!(got, num_bytes_exceed);
            }
            other => panic!("expected TooManyBytes, got {:?}", other),
        }
    }
}
