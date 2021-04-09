use crate::errors::RTPError;
use byteorder::{BigEndian, ByteOrder};

pub(super) const HEADER_LENGTH: usize = 4;
pub(super) const VERSION_SHIFT: u8 = 6;
pub(super) const VERSION_MASK: u8 = 0x3;
pub(super) const PADDING_SHIFT: u8 = 5;
pub(super) const PADDING_MASK: u8 = 0x1;
pub(super) const EXTENSION_SHIFT: u8 = 4;
pub(super) const EXTENSION_MASK: u8 = 0x1;
pub(super) const EXTENSION_PROFILE_ONE_BYTE: u16 = 0xBEDE;
pub(super) const EXTENSION_PROFILE_TWO_BYTE: u16 = 0x1000;
pub(super) const EXTENSION_ID_RESERVED: u8 = 0xF;
pub(super) const CC_MASK: u8 = 0xF;
pub(super) const MARKER_SHIFT: u8 = 7;
pub(super) const MARKER_MASK: u8 = 0x1;
pub(super) const PT_MASK: u8 = 0x7F;
pub(super) const SEQ_NUM_OFFSET: usize = 2;
pub(super) const SEQ_NUM_LENGTH: usize = 2;
pub(super) const TIMESTAMP_OFFSET: usize = 4;
pub(super) const TIMESTAMP_LENGTH: usize = 4;
pub(super) const SSRC_OFFSET: usize = 8;
pub(super) const SSRC_LENGTH: usize = 4;
pub(super) const CSRC_OFFSET: usize = 12;
pub(super) const CSRC_LENGTH: usize = 4;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u16)]
pub enum ExtensionProfile {
    OneByte = 0xBEDE,
    TwoByte = 0x1000,
    Undefined,
}

impl Default for ExtensionProfile {
    fn default() -> Self {
        0.into()
    }
}

impl From<u16> for ExtensionProfile {
    fn from(val: u16) -> Self {
        match val {
            0xBEDE => ExtensionProfile::OneByte,
            0x1000 => ExtensionProfile::TwoByte,
            _ => ExtensionProfile::Undefined,
        }
    }
}

#[derive(Debug, Eq, Clone, PartialEq, Default)]
pub struct Extension {
    pub id: u8,
    pub payload: Vec<u8>,
}

/// Header represents an RTP packet header
/// NOTE: PayloadOffset is populated by Marshal/Unmarshal and should not be modified
#[derive(Debug, Eq, Clone, PartialEq, Default)]
pub struct Header {
    pub version: u8,
    pub padding: bool,
    pub extension: bool,
    pub marker: bool,
    pub payload_type: u8,
    pub sequence_number: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub csrc: Vec<u32>,
    pub extension_profile: u16,
    pub extensions: Vec<Extension>,
}

impl Header {
    // Returns the size of the packet once marshaled.
    pub fn marshal_size(&self) -> usize {
        let mut head_size = 12 + (self.csrc.len() * CSRC_LENGTH);
        if self.extension {
            let extension_payload_len = self.get_extension_payload_len();
            let extension_payload_size = (extension_payload_len + 3) / 4;
            head_size += 4 + extension_payload_size * 4;
        }

        head_size
    }

    fn get_extension_payload_len(&self) -> usize {
        let mut extension_length = 0;

        match ExtensionProfile::from(self.extension_profile) {
            ExtensionProfile::OneByte => {
                for extension in &self.extensions {
                    extension_length += 1 + extension.payload.len();
                }
            }
            ExtensionProfile::TwoByte => {
                for extension in &self.extensions {
                    extension_length += 2 + extension.payload.len();
                }
            }
            _ => {
                extension_length += self.extensions[0].payload.len();
            }
        };

        extension_length
    }

    /// Sets an RTP header extension
    pub fn set_extension(&mut self, id: u8, payload: &[u8]) -> Result<(), RTPError> {
        if self.extension {
            match ExtensionProfile::from(self.extension_profile) {
                ExtensionProfile::OneByte => {
                    if !(1..=14).contains(&id) {
                        return Err(RTPError::RFC8285OneByteHeaderIDRange(id));
                    }
                    if payload.len() > 16 {
                        return Err(RTPError::RFC8285OneByteHeaderSize(id));
                    }
                }

                ExtensionProfile::TwoByte => {
                    if id < 1 {
                        return Err(RTPError::RFC8285TwoByteHeaderIDRange(id));
                    }
                    if payload.len() > 255 {
                        return Err(RTPError::RFC8285TwoByteHeaderSize(id));
                    }
                }
                _ => {
                    if id != 0 {
                        return Err(RTPError::RFC3550HeaderIDRange(id));
                    }
                }
            };

            // Update existing if it exists else add new extension
            for extension in &mut self.extensions {
                if extension.id == id {
                    extension.payload.clear();
                    extension.payload.extend_from_slice(payload);
                    return Ok(());
                }
            }
            self.extensions.push(Extension {
                id,
                payload: payload.to_owned(),
            });
            return Ok(());
        }

        // No existing header extensions
        self.extension = true;

        let len = payload.len();
        if len <= 16 {
            self.extension_profile = ExtensionProfile::OneByte as u16
        } else if len > 16 && len < 256 {
            self.extension_profile = ExtensionProfile::TwoByte as u16
        }

        self.extensions.push(Extension {
            id,
            payload: payload.to_owned(),
        });

        Ok(())
    }

    /// Returns an RTP header extension
    pub fn get_extension(&self, id: u8) -> Option<&[u8]> {
        if !self.extension {
            return None;
        }

        for extension in &self.extensions {
            if extension.id == id {
                return Some(&extension.payload);
            }
        }
        None
    }

    // GetExtensionIDs returns an extension id array
    pub fn get_extension_ids(&self) -> Vec<u8> {
        if !self.extension {
            return vec![];
        }

        if self.extensions.is_empty() {
            return vec![];
        }

        let mut ids = vec![0u8; self.extensions.len()];

        for id in ids.iter_mut().take(self.extensions.len()) {
            *id = self.extensions[0].id
        }

        ids
    }

    /// Removes an RTP Header extension
    pub fn del_extension(&mut self, id: u8) -> Result<(), RTPError> {
        if !self.extension {
            return Err(RTPError::HeaderExtensionNotEnabled);
        }
        for index in 0..self.extensions.len() {
            if self.extensions[index].id == id {
                self.extensions.remove(index);
                return Ok(());
            }
        }

        Err(RTPError::HeaderExtensionNotFound)
    }

    /// Unmarshal parses the passed byte slice and stores the result in the Header this method is called upon
    pub fn unmarshal(&mut self, raw_packet: &[u8]) -> Result<usize, RTPError> {
        /*
         *  0                   1                   2                   3
         *  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
         * +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
         * |V=2|P|X|  CC   |M|     PT      |       sequence number         |
         * +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
         * |                           timestamp                           |
         * +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
         * |           synchronization source (SSRC) identifier            |
         * +=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+
         * |            contributing source (CSRC) identifiers             |
         * |                             ....                              |
         * +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
         */

        if raw_packet.len() < HEADER_LENGTH {
            return Err(RTPError::HeaderSizeInsufficient);
        }

        self.version = raw_packet[0] >> VERSION_SHIFT & VERSION_MASK;
        self.padding = (raw_packet[0] >> PADDING_SHIFT & PADDING_MASK) > 0;
        self.extension = (raw_packet[0] >> EXTENSION_SHIFT & EXTENSION_MASK) > 0;
        self.csrc = vec![0u32; (raw_packet[0] & CC_MASK) as usize];

        let mut current_offset = CSRC_OFFSET + (self.csrc.len() * CSRC_LENGTH);
        if raw_packet.len() < current_offset {
            return Err(RTPError::HeaderSizeInsufficient);
        }

        self.marker = (raw_packet[1] >> MARKER_SHIFT & MARKER_MASK) > 0;
        self.payload_type = raw_packet[1] & PT_MASK;

        self.sequence_number =
            BigEndian::read_u16(&raw_packet[SEQ_NUM_OFFSET..SEQ_NUM_OFFSET + SEQ_NUM_LENGTH]);
        self.timestamp =
            BigEndian::read_u32(&raw_packet[TIMESTAMP_OFFSET..TIMESTAMP_OFFSET + TIMESTAMP_LENGTH]);
        self.ssrc = BigEndian::read_u32(&raw_packet[SSRC_OFFSET..SSRC_OFFSET + SSRC_LENGTH]);

        for i in 0..self.csrc.len() {
            let offset = CSRC_OFFSET + (i * CSRC_LENGTH);
            self.csrc[i] = BigEndian::read_u32(&raw_packet[offset..]);
        }

        if self.extension {
            if raw_packet.len() < current_offset + 4 {
                return Err(RTPError::HeaderSizeInsufficientForExtension);
            }

            self.extension_profile = BigEndian::read_u16(&raw_packet[current_offset..]);
            current_offset += 2;

            let extension_length =
                (BigEndian::read_u16(&raw_packet[current_offset..]) as usize) * 4;
            current_offset += 2;

            if raw_packet.len() < current_offset + extension_length as usize {
                return Err(RTPError::HeaderSizeInsufficientForExtension);
            }

            match ExtensionProfile::from(self.extension_profile) {
                // RFC 8285 RTP One Byte Header Extension
                ExtensionProfile::OneByte => {
                    let end = current_offset + extension_length as usize;

                    while current_offset < end {
                        // Padding
                        if raw_packet[current_offset] == 0x00 {
                            current_offset += 1;
                            continue;
                        }

                        let ext_id = raw_packet[current_offset] >> 4;
                        let len = (raw_packet[current_offset] as usize & !0xF0) + 1;
                        current_offset += 1;

                        if ext_id == EXTENSION_ID_RESERVED {
                            break;
                        }

                        self.extensions.push(Extension {
                            id: ext_id,
                            payload: raw_packet[current_offset..current_offset + len].to_vec(),
                        });

                        current_offset += len;
                    }
                }

                // RFC 8285 RTP Two Byte Header Extension
                ExtensionProfile::TwoByte => {
                    let end = current_offset + extension_length as usize;

                    while current_offset < end {
                        // Padding
                        if raw_packet[current_offset] == 0x00 {
                            current_offset += 1;
                            continue;
                        }

                        let ext_id = raw_packet[current_offset];
                        current_offset += 1;

                        let len = raw_packet[current_offset];
                        current_offset += 1;

                        self.extensions.push(Extension {
                            id: ext_id,
                            payload: raw_packet[current_offset..current_offset + len as usize]
                                .to_vec(),
                        });

                        current_offset += len as usize;
                    }
                }

                // RFC3550 Extension
                _ => {
                    if raw_packet.len() < current_offset + extension_length as usize {
                        return Err(RTPError::HeaderSizeInsufficientForExtension);
                    }

                    self.extensions.push(Extension {
                        id: 0,
                        payload: raw_packet
                            [current_offset..current_offset + extension_length as usize]
                            .to_vec(),
                    });

                    current_offset += self.extensions[0].payload.len();
                }
            }
        }

        Ok(current_offset)
    }

    /// Marshal serializes the packet into bytes.
    pub fn marshal(&mut self) -> Result<Vec<u8>, RTPError> {
        let mut buf = vec![0u8; self.marshal_size()];

        let n = self.marshal_to(&mut buf)?;
        assert_eq!(n, buf.len(), "buf size should be exactly allocated");

        Ok(buf)
    }

    /// Serializes the header and writes to the buffer. It requires buf length size to have been allocated.
    pub fn marshal_to(&mut self, buf: &mut [u8]) -> Result<usize, RTPError> {
        /*
         *  0                   1                   2                   3
         *  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
         * +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
         * |V=2|P|X|  CC   |M|     PT      |       sequence number         |
         * +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
         * |                           timestamp                           |
         * +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
         * |           synchronization source (SSRC) identifier            |
         * +=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+=+
         * |            contributing source (CSRC) identifiers             |
         * |                             ....                              |
         * +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
         */

        let size = self.marshal_size();
        if size > buf.len() {
            return Err(RTPError::ShortBuffer);
        }

        // The first byte contains the version, padding bit, extension bit, and csrc size
        buf[0] = (self.version << VERSION_SHIFT) | self.csrc.len() as u8;

        if self.padding {
            buf[0] |= 1 << PADDING_SHIFT
        }

        if self.extension {
            buf[0] |= 1 << EXTENSION_SHIFT
        }

        // The second byte contains the marker bit and payload type.
        buf[1] = self.payload_type;

        if self.marker {
            buf[1] |= 1 << MARKER_SHIFT
        }

        BigEndian::write_u16(&mut buf[2..4], self.sequence_number);
        BigEndian::write_u32(&mut buf[4..8], self.timestamp);
        BigEndian::write_u32(&mut buf[8..12], self.ssrc);

        let mut no_alloc = 12usize;

        let csrc = self.csrc.clone();
        for n in csrc {
            BigEndian::write_u32(&mut buf[no_alloc..no_alloc + 4], n);
            no_alloc += 4;
        }

        if self.extension {
            let ext_header_pos = no_alloc;
            BigEndian::write_u16(&mut buf[no_alloc..no_alloc + 2], self.extension_profile);

            no_alloc += 4;
            let start_extensions_pos = no_alloc;

            match ExtensionProfile::from(self.extension_profile) {
                // RFC 8285 RTP One Byte Header Extension
                ExtensionProfile::OneByte => {
                    for extension in &self.extensions {
                        buf[no_alloc] = extension.id << 4 | (extension.payload.len() - 1) as u8;
                        no_alloc += 1;

                        buf[no_alloc..no_alloc + extension.payload.len()]
                            .copy_from_slice(&extension.payload);

                        no_alloc += extension.payload.len();
                    }
                }

                // RFC 8285 RTP Two Byte Header Extension
                ExtensionProfile::TwoByte => {
                    for extension in &self.extensions {
                        buf[no_alloc] = extension.id;
                        no_alloc += 1;

                        buf[no_alloc] = extension.payload.len() as u8;
                        no_alloc += 1;

                        buf[no_alloc..no_alloc + extension.payload.len()]
                            .copy_from_slice(&extension.payload);

                        no_alloc += extension.payload.len();
                    }
                }

                // RFC3550 Extension
                _ => {
                    let ext_len = self.extensions[0].payload.len();

                    if ext_len % 4 != 0 {
                        // The payload must be in 32-bit words.
                        return Err(RTPError::ShortBuffer);
                    }

                    buf[no_alloc..no_alloc + self.extensions[0].payload.len()]
                        .copy_from_slice(&self.extensions[0].payload);

                    no_alloc += self.extensions[0].payload.len();
                }
            }

            // calculate extensions size and round to 4 bytes boundaries
            let ext_size = no_alloc - start_extensions_pos;
            let rounded_ext_size = ((ext_size + 3) / 4) * 4;

            BigEndian::write_u16(
                &mut buf[ext_header_pos + 2..ext_header_pos + 4],
                (rounded_ext_size / 4) as u16,
            );

            // add padding to reach 4 bytes boundaries
            for _ in 0..(rounded_ext_size - ext_size) {
                buf[no_alloc] = 0;
                no_alloc += 1;
            }
        }

        Ok(no_alloc)
    }
}
