/// Read and write implementations for the protocol messages
use std::io::{self, BufRead, BufReader, Read, Write};

use crate::{
    error::ReadError,
    protocol::{Message, Version, XvcInfo},
};

const XVC_INFO_PREFIX: &[u8] = b"xvcServer";

impl XvcInfo {
    pub fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        writeln!(
            writer,
            "xvcServer_v{}:{}",
            self.version(),
            self.max_vector_len()
        )
    }

    pub fn from_reader(reader: &mut impl Read) -> Result<XvcInfo, ReadError> {
        let mut buf_reader = BufReader::with_capacity(32, reader);
        let mut line = Vec::with_capacity(32);
        let _ = buf_reader.read_until(b'\n', &mut line)?;

        // Remove trailing newline
        let mut line = line.trim_ascii_end();

        // Parse format: "xvcServer_v{version}:{max_vector_len_bytes}"
        if !line.starts_with(XVC_INFO_PREFIX) {
            return Err(ReadError::InvalidFormat(
                "Invalid prefix in info message".to_string(),
            ));
        }

        line = &line[XVC_INFO_PREFIX.len()..];
        if line[0] != b'_' {
            return Err(ReadError::InvalidFormat(
                "Missing '_' separator".to_string(),
            ));
        }

        line = &line[1..];
        if line[0] != b'v' {
            return Err(ReadError::InvalidFormat(
                "Version must start with 'v".to_string(),
            ));
        }

        line = &line[1..];
        let colon_index = line.iter().position(|l| *l == b':').ok_or_else(|| {
            ReadError::InvalidFormat("Missing ':' separator in info message".to_string())
        })?;

        let (version_part, rest) = line.split_at(colon_index);

        let version = match version_part {
            b"1.0" => Version::V1_0,
            _ => {
                return Err(ReadError::UnsupportedVersion(
                    String::from_utf8_lossy(version_part).to_string(),
                ));
            }
        };

        let max_vector_len = str::from_utf8(&rest[1..])?.parse::<u32>()?;

        Ok(XvcInfo::new(version, max_vector_len))
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
    let info = XvcInfo::from_reader(&mut cursor).unwrap();
    assert_eq!(info.version(), Version::V1_0);
    assert_eq!(info.max_vector_len(), 32)
}

impl Message {
    const CMD_NAME_GET_INFO: &[u8; 7] = b"getinfo";
    const CMD_NAME_SET_TCK: &[u8; 6] = b"settck";
    const CMD_NAME_SHIFT: &[u8; 5] = b"shift";
    const CMD_DELIMITER: u8 = b':';

    pub fn from_reader(
        reader: &mut impl Read,
        max_shift_bytes: usize,
    ) -> Result<Message, ReadError> {
        // Buffer must accommodate: "shift:" (5) + num_bits (4) = 13 bytes minimum
        let mut buf = [0u8; 16];
        // read 2 bytes into the buffer
        reader.read_exact(&mut buf[..2])?;
        match &buf[..2] {
            b"ge" => {
                reader.read_exact(&mut buf[2..Self::CMD_NAME_GET_INFO.len() + 1])?;
                if &buf[..Self::CMD_NAME_GET_INFO.len()] != Self::CMD_NAME_GET_INFO
                    || buf[Self::CMD_NAME_GET_INFO.len()] != Self::CMD_DELIMITER
                {
                    return Err(ReadError::InvalidCommand(
                        String::from_utf8_lossy(&buf).to_string(),
                    ));
                }
                Ok(Message::GetInfo)
            }
            b"se" => {
                reader.read_exact(&mut buf[2..Self::CMD_NAME_SET_TCK.len() + 1 + 4])?;
                if &buf[..Self::CMD_NAME_SET_TCK.len()] != Self::CMD_NAME_SET_TCK
                    || buf[Self::CMD_NAME_SET_TCK.len()] != Self::CMD_DELIMITER
                {
                    return Err(ReadError::InvalidCommand(
                        String::from_utf8_lossy(&buf).to_string(),
                    ));
                }
                let period = u32::from_le_bytes(
                    buf[Self::CMD_NAME_SET_TCK.len() + 1..Self::CMD_NAME_SET_TCK.len() + 5]
                        .try_into()
                        .unwrap(),
                );
                Ok(Message::SetTck { period_ns: period })
            }
            b"sh" => {
                reader.read_exact(&mut buf[2..Self::CMD_NAME_SHIFT.len() + 1 + 4])?;
                if &buf[..Self::CMD_NAME_SHIFT.len()] != Self::CMD_NAME_SHIFT
                    || buf[Self::CMD_NAME_SHIFT.len()] != Self::CMD_DELIMITER
                {
                    return Err(ReadError::InvalidCommand(
                        String::from_utf8_lossy(&buf).to_string(),
                    ));
                }
                let num_bits = u32::from_le_bytes(
                    buf[Self::CMD_NAME_SHIFT.len() + 1..Self::CMD_NAME_SHIFT.len() + 5]
                        .try_into()
                        .unwrap(),
                );
                let num_bytes = num_bits.div_ceil(8_u32) as usize;
                if num_bytes > max_shift_bytes {
                    return Err(ReadError::TooManyBytes {
                        max: max_shift_bytes,
                        got: num_bytes,
                    });
                }
                let mut tms_vector = vec![0_u8; num_bytes].into_boxed_slice();
                reader.read_exact(&mut tms_vector[..])?;
                let mut tdi_vector = vec![0_u8; num_bytes].into_boxed_slice();
                reader.read_exact(&mut tdi_vector[..])?;
                Ok(Message::Shift {
                    num_bits,
                    tms: tms_vector,
                    tdi: tdi_vector,
                })
            }
            _ => Err(ReadError::InvalidCommandPrefix(
                String::from_utf8_lossy(&buf[..2]).to_string(),
            )),
        }
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
    use crate::error::ReadError;
    use crate::protocol::Message;
    use std::io::Cursor;

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
        let num_bytes = ((num_bits + 7) / 8) as usize;
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
        let num_bytes = ((num_bits + 7) / 8) as usize;
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
        expected.extend_from_slice(&*tms);
        expected.extend_from_slice(&*tdi);

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
