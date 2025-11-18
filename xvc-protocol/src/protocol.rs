use std::fmt::Display;

/// The version of the protocol.
/// A version always consists of a major and a minor part.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Version {
    major: usize,
    minor: usize,
}

impl Version {
    /// Version 1.0 of the protocol
    pub const V1_0: Version = Version { major: 1, minor: 0 };

    /// Returns the latest supported version
    pub fn latest() -> Version {
        Version::V1_0
    }

    /// The major part of the version
    pub fn major(&self) -> usize {
        self.major
    }

    /// The minor part of the version
    pub fn minor(&self) -> usize {
        self.minor
    }
}

#[test]
fn version_ordering() {
    assert!(Version { major: 1, minor: 0 } < Version { major: 1, minor: 1 });
    assert!(Version { major: 2, minor: 0 } > Version { major: 1, minor: 0 });
}

impl Default for Version {
    fn default() -> Self {
        Self::V1_0
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// A Message is transfered from the client to the server.
/// For each message, the client is expected to send the message and wait for a response from the server.
/// The server needs to process each message in the order received and promptly provide a response.
/// For the XVC 1.0 protocol, only one connection is assumed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    /// Requests info from the server. This is used to determine protocol capabilities of the server.
    GetInfo,
    /// Configures the TCK period. When sending JTAG vectors the TCK rate may need to be varied to accomodate cable and board signal integrity conditions.
    /// This command is used by clients to adjust the TCK rate in order to slow down or speed up the shifting of JTAG vectors.
    SetTck { period_ns: u32 },
    /// Used to shift JTAG vectors in-and out of a device.
    Shift {
        /// represents the number of TCK clk toggles needed to shift the vectors out
        num_bits: u32,
        /// a byte sized vector with all the TMS data.
        /// The vector is num_bits and rounds up to the nearest byte.
        tms: Box<[u8]>,
        /// a byte sized vector with all the TDI data.
        /// The vector is num_bits and rounds up to the nearest byte.
        tdi: Box<[u8]>,
    },
}

/// Contains static information about the server capabilities that are transfered between
/// client and server in the beginning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XvcInfo {
    version: Version,
    max_vector_len: u32,
}

impl XvcInfo {
    /// Creates a new info object from version and the maximum receivable vector length.
    pub fn new(version: Version, max_vector_len: u32) -> XvcInfo {
        XvcInfo {
            version,
            max_vector_len,
        }
    }

    /// The version of the protocol
    pub fn version(&self) -> Version {
        self.version
    }

    /// the max width of the vector that can be shifted into the server
    pub fn max_vector_len(&self) -> u32 {
        self.max_vector_len
    }
}

impl Default for XvcInfo {
    fn default() -> XvcInfo {
        XvcInfo {
            version: Version::default(),
            max_vector_len: 10 * 1024 * 1024, // 10 MiB default
        }
    }
}
