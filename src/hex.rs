//! Intel HEX File Parser
//!
//! Parses Intel HEX format files into a memory image that can be used
//! for checksum verification against XCP slave memory.

use std::collections::BTreeMap;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during HEX file parsing
#[derive(Debug, Error)]
pub enum HexError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error at line {line}: {message}")]
    Parse { line: usize, message: String },

    #[error("Checksum mismatch at line {line}: expected 0x{expected:02X}, got 0x{actual:02X}")]
    Checksum {
        line: usize,
        expected: u8,
        actual: u8,
    },

    #[error("No data found in HEX file for address range 0x{address:08X}..0x{end:08X}")]
    NoData { address: u32, end: u32 },

    #[error("Incomplete data: only {found} of {expected} bytes found for range 0x{address:08X}..0x{end:08X}")]
    IncompleteData {
        address: u32,
        end: u32,
        expected: u32,
        found: u32,
    },
}

/// A parsed Intel HEX memory image
#[derive(Debug, Clone)]
pub struct HexImage {
    /// Sparse memory map: address → byte value
    data: BTreeMap<u32, u8>,
}

impl HexImage {
    /// Create an empty HexImage (for building from uploaded data)
    pub fn new() -> Self {
        Self { data: BTreeMap::new() }
    }

    /// Insert raw bytes at `base_address`.
    /// Any existing bytes at those addresses are overwritten.
    pub fn add_data(&mut self, base_address: u32, data: &[u8]) {
        for (i, &byte) in data.iter().enumerate() {
            self.data.insert(base_address + i as u32, byte);
        }
    }

    /// Serialize the memory image to Intel HEX format.
    ///
    /// Uses 32-byte data records and extended linear address records (type 0x04)
    /// whenever the upper 16 bits of the address change.
    pub fn to_intel_hex(&self) -> String {
        const BYTES_PER_LINE: usize = 32;
        let mut output = String::with_capacity(self.data.len() * 7 + 16);

        if self.data.is_empty() {
            output.push_str(":00000001FF\n");
            return output;
        }

        // Build contiguous blocks from the sparse map
        let mut blocks: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut current_start: Option<u32> = None;
        let mut current_bytes: Vec<u8> = Vec::new();

        for (&addr, &byte) in &self.data {
            match current_start {
                None => {
                    current_start = Some(addr);
                    current_bytes.push(byte);
                }
                Some(start) if addr == start + current_bytes.len() as u32 => {
                    current_bytes.push(byte);
                }
                Some(start) => {
                    blocks.push((start, std::mem::take(&mut current_bytes)));
                    current_start = Some(addr);
                    current_bytes.push(byte);
                }
            }
        }
        if let Some(start) = current_start {
            if !current_bytes.is_empty() {
                blocks.push((start, current_bytes));
            }
        }

        let mut last_upper: Option<u16> = None;

        for (block_start, block_data) in &blocks {
            let mut offset = 0usize;

            while offset < block_data.len() {
                let addr = block_start + offset as u32;
                let upper = (addr >> 16) as u16;
                let lower = (addr & 0xFFFF) as u16;

                if last_upper != Some(upper) {
                    intel_hex_write_ela(&mut output, upper);
                    last_upper = Some(upper);
                }

                // Cap at BYTES_PER_LINE and at the 64 KB boundary
                let remaining_in_block = block_data.len() - offset;
                let remaining_in_64k = (0x10000usize).saturating_sub(lower as usize);
                let line_len = remaining_in_block.min(BYTES_PER_LINE).min(remaining_in_64k);

                intel_hex_write_data(&mut output, lower, &block_data[offset..offset + line_len]);
                offset += line_len;
            }
        }

        output.push_str(":00000001FF\n");
        output
    }

    /// Parse an Intel HEX file from a file path
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, HexError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    /// Parse an Intel HEX file from a string
    pub fn from_str(content: &str) -> Result<Self, HexError> {
        let mut data = BTreeMap::new();
        let mut extended_address: u32 = 0;

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let record = HexRecord::parse(line, line_num + 1)?;
            record.verify_checksum(line_num + 1)?;

            match record.record_type {
                0x00 => {
                    // Data record
                    let base_addr = extended_address + record.address as u32;
                    for (i, &byte) in record.data.iter().enumerate() {
                        data.insert(base_addr + i as u32, byte);
                    }
                }
                0x01 => {
                    // End of File
                    break;
                }
                0x02 => {
                    // Extended Segment Address: physical = (segment << 4) + offset
                    if record.data.len() >= 2 {
                        let segment = u16::from_be_bytes([record.data[0], record.data[1]]);
                        extended_address = (segment as u32) << 4;
                    }
                }
                0x03 => {
                    // Start Segment Address (CS:IP) — ignored
                }
                0x04 => {
                    // Extended Linear Address: physical = (upper16 << 16) + offset
                    if record.data.len() >= 2 {
                        let upper = u16::from_be_bytes([record.data[0], record.data[1]]);
                        extended_address = (upper as u32) << 16;
                    }
                }
                0x05 => {
                    // Start Linear Address (EIP) — ignored
                }
                _ => {
                    return Err(HexError::Parse {
                        line: line_num + 1,
                        message: format!("Unknown record type: 0x{:02X}", record.record_type),
                    });
                }
            }
        }

        Ok(Self { data })
    }

    /// Extract a contiguous block of data for the given address range.
    /// Missing bytes are filled with `fill_byte` (typically 0xFF for flash memory).
    pub fn extract_range(&self, base_address: u32, size: u32, fill_byte: u8) -> Vec<u8> {
        let mut result = vec![fill_byte; size as usize];
        for offset in 0..size {
            if let Some(&byte) = self.data.get(&(base_address + offset)) {
                result[offset as usize] = byte;
            }
        }
        result
    }

    /// Check if any data exists in the given address range
    pub fn has_data_in_range(&self, base_address: u32, size: u32) -> bool {
        let end = base_address.saturating_add(size);
        self.data.range(base_address..end).next().is_some()
    }

    /// Count how many bytes are populated in the given address range
    pub fn byte_count_in_range(&self, base_address: u32, size: u32) -> u32 {
        let end = base_address.saturating_add(size);
        self.data.range(base_address..end).count() as u32
    }

    /// Get the total number of bytes stored
    pub fn total_bytes(&self) -> usize {
        self.data.len()
    }

    /// Get the address range covered by the data (min_addr, max_addr inclusive)
    pub fn address_range(&self) -> Option<(u32, u32)> {
        let first = self.data.keys().next()?;
        let last = self.data.keys().next_back()?;
        Some((*first, *last))
    }
}

/// Write an Intel HEX Extended Linear Address record (type 0x04).
fn intel_hex_write_ela(output: &mut String, upper: u16) {
    let h = (upper >> 8) as u8;
    let l = upper as u8;
    // sum = len(2) + addr_H(0) + addr_L(0) + type(4) + data_H + data_L
    let sum: u8 = 0x02u8
        .wrapping_add(0x04)
        .wrapping_add(h)
        .wrapping_add(l);
    let cs = (!sum).wrapping_add(1);
    output.push_str(&format!(":02000004{:02X}{:02X}{:02X}\n", h, l, cs));
}

/// Write a single Intel HEX data record (type 0x00) with a 16-bit address offset.
fn intel_hex_write_data(output: &mut String, addr16: u16, data: &[u8]) {
    let len = data.len() as u8;
    let ah = (addr16 >> 8) as u8;
    let al = addr16 as u8;
    let mut sum: u8 = len.wrapping_add(ah).wrapping_add(al); // type 0x00 adds nothing
    for &b in data {
        sum = sum.wrapping_add(b);
    }
    let cs = (!sum).wrapping_add(1);

    output.push(':');
    output.push_str(&format!("{:02X}{:04X}00", len, addr16));
    for &b in data {
        output.push_str(&format!("{:02X}", b));
    }
    output.push_str(&format!("{:02X}\n", cs));
}

/// A single record from an Intel HEX file
#[derive(Debug)]
struct HexRecord {
    /// Number of data bytes
    byte_count: u8,
    /// 16-bit address field
    address: u16,
    /// Record type (0x00-0x05)
    record_type: u8,
    /// Data bytes
    data: Vec<u8>,
    /// Checksum byte
    checksum: u8,
}

impl HexRecord {
    /// Parse a single HEX record line
    fn parse(line: &str, line_num: usize) -> Result<Self, HexError> {
        if !line.starts_with(':') {
            return Err(HexError::Parse {
                line: line_num,
                message: "Line does not start with ':'".to_string(),
            });
        }

        let hex_str = &line[1..];
        if hex_str.len() < 10 {
            return Err(HexError::Parse {
                line: line_num,
                message: "Line too short".to_string(),
            });
        }

        let bytes = Self::decode_hex(hex_str, line_num)?;
        if bytes.len() < 5 {
            return Err(HexError::Parse {
                line: line_num,
                message: "Not enough bytes".to_string(),
            });
        }

        let byte_count = bytes[0];
        let address = u16::from_be_bytes([bytes[1], bytes[2]]);
        let record_type = bytes[3];

        let expected_len = 4 + byte_count as usize + 1; // header + data + checksum
        if bytes.len() < expected_len {
            return Err(HexError::Parse {
                line: line_num,
                message: format!("Expected {} bytes but got {}", expected_len, bytes.len()),
            });
        }

        let data = bytes[4..4 + byte_count as usize].to_vec();
        let checksum = bytes[4 + byte_count as usize];

        Ok(Self {
            byte_count,
            address,
            record_type,
            data,
            checksum,
        })
    }

    /// Verify the record's checksum (two's complement of sum of all bytes)
    fn verify_checksum(&self, line_num: usize) -> Result<(), HexError> {
        let mut sum: u8 = 0;
        sum = sum.wrapping_add(self.byte_count);
        sum = sum.wrapping_add((self.address >> 8) as u8);
        sum = sum.wrapping_add(self.address as u8);
        sum = sum.wrapping_add(self.record_type);
        for &b in &self.data {
            sum = sum.wrapping_add(b);
        }
        let expected = (!sum).wrapping_add(1); // two's complement

        if expected != self.checksum {
            return Err(HexError::Checksum {
                line: line_num,
                expected,
                actual: self.checksum,
            });
        }
        Ok(())
    }

    /// Decode a hex string into bytes
    fn decode_hex(hex_str: &str, line_num: usize) -> Result<Vec<u8>, HexError> {
        if hex_str.len() % 2 != 0 {
            return Err(HexError::Parse {
                line: line_num,
                message: "Odd number of hex characters".to_string(),
            });
        }

        let mut bytes = Vec::with_capacity(hex_str.len() / 2);
        for i in (0..hex_str.len()).step_by(2) {
            let byte = u8::from_str_radix(&hex_str[i..i + 2], 16).map_err(|_| HexError::Parse {
                line: line_num,
                message: format!("Invalid hex at position {}", i),
            })?;
            bytes.push(byte);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_data_record() {
        let hex = ":0400000001020304F2\n:00000001FF\n";
        let image = HexImage::from_str(hex).unwrap();
        assert_eq!(image.total_bytes(), 4);
        let data = image.extract_range(0x0000, 4, 0xFF);
        assert_eq!(data, vec![0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_extended_linear_address() {
        // Set upper address to 0xA038, then write data at offset 0x0000
        let hex = concat!(
            ":02000004A03822\n",     // Extended Linear Address: 0xA038
            ":04000000AABBCCDDEE\n", // Data at 0xA0380000
            ":00000001FF\n",         // EOF
        );
        let image = HexImage::from_str(hex).unwrap();
        let data = image.extract_range(0xA0380000, 4, 0xFF);
        assert_eq!(data, vec![0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn test_extended_segment_address() {
        // Set segment to 0x1000, so base = 0x10000
        let hex = concat!(
            ":020000021000EC\n", // Extended Segment Address: 0x1000
            ":020000001234B8\n", // Data at 0x10000
            ":00000001FF\n",
        );
        let image = HexImage::from_str(hex).unwrap();
        let data = image.extract_range(0x10000, 2, 0xFF);
        assert_eq!(data, vec![0x12, 0x34]);
    }

    #[test]
    fn test_extract_with_fill() {
        let hex = ":02000200AABB97\n:00000001FF\n";
        let image = HexImage::from_str(hex).unwrap();
        // Extract 4 bytes starting at 0x0000, but data only at 0x0002-0x0003
        let data = image.extract_range(0x0000, 4, 0xFF);
        assert_eq!(data, vec![0xFF, 0xFF, 0xAA, 0xBB]);
    }

    #[test]
    fn test_has_data_in_range() {
        let hex = ":04000000AABBCCDDEE\n:00000001FF\n";
        let image = HexImage::from_str(hex).unwrap();
        assert!(image.has_data_in_range(0x0000, 4));
        assert!(!image.has_data_in_range(0x1000, 4));
    }

    #[test]
    fn test_address_range() {
        let hex = ":04001000AABBCCDDDE\n:00000001FF\n";
        let image = HexImage::from_str(hex).unwrap();
        assert_eq!(image.address_range(), Some((0x0010, 0x0013)));
    }

    #[test]
    fn test_checksum_validation_fails() {
        let hex = ":04000000AABBCCDDF0\n:00000001FF\n"; // wrong checksum
        let result = HexImage::from_str(hex);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), HexError::Checksum { .. }));
    }

    #[test]
    fn test_empty_hex() {
        let hex = ":00000001FF\n";
        let image = HexImage::from_str(hex).unwrap();
        assert_eq!(image.total_bytes(), 0);
    }

    #[test]
    fn test_multiple_data_records() {
        let hex = concat!(
            ":0200000480007A\n",     // Extended: 0x8000
            ":0400000001020304F2\n", // 4 bytes at 0x80000000
            ":0400040005060708DE\n", // 4 bytes at 0x80000004
            ":00000001FF\n",
        );
        let image = HexImage::from_str(hex).unwrap();
        assert_eq!(image.total_bytes(), 8);
        let data = image.extract_range(0x80000000, 8, 0xFF);
        assert_eq!(data, vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
    }

    #[test]
    fn test_byte_count_in_range() {
        let hex = concat!(":04000000AABBCCDDEE\n", ":00000001FF\n",);
        let image = HexImage::from_str(hex).unwrap();
        assert_eq!(image.byte_count_in_range(0x0000, 4), 4);
        assert_eq!(image.byte_count_in_range(0x0000, 8), 4);
        assert_eq!(image.byte_count_in_range(0x0004, 4), 0);
    }

    #[test]
    fn test_extract_range_exact_boundary() {
        // Data at addresses 0x0000-0x0003, extract exactly that range
        let hex = ":04000000DEADBEEFC4\n:00000001FF\n";
        let image = HexImage::from_str(hex).unwrap();
        let data = image.extract_range(0x0000, 4, 0x00);
        assert_eq!(data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_extract_range_no_data_all_fill() {
        let hex = ":04000000AABBCCDDEE\n:00000001FF\n";
        let image = HexImage::from_str(hex).unwrap();
        // No data at 0x1000, should be all fill bytes
        let data = image.extract_range(0x1000, 4, 0xAA);
        assert_eq!(data, vec![0xAA, 0xAA, 0xAA, 0xAA]);
    }

    #[test]
    fn test_has_data_in_range_partial_overlap() {
        let hex = ":04000200AABBCCDDEC\n:00000001FF\n";
        let image = HexImage::from_str(hex).unwrap();
        // Data at 0x0002-0x0005, check range 0x0000-0x0003 should overlap
        assert!(image.has_data_in_range(0x0000, 4));
        // But range 0x0006-0x0009 should not overlap
        assert!(!image.has_data_in_range(0x0006, 4));
    }

    #[test]
    fn test_address_range_empty() {
        let hex = ":00000001FF\n";
        let image = HexImage::from_str(hex).unwrap();
        assert_eq!(image.address_range(), None);
    }

    #[test]
    fn test_start_linear_address_ignored() {
        // Record type 05 (start linear address) should be ignored, not error
        let hex = concat!(
            ":04000000AABBCCDDEE\n",
            ":0400000508000000EF\n", // Start Linear Address (entry point)
            ":00000001FF\n",
        );
        let image = HexImage::from_str(hex).unwrap();
        assert_eq!(image.total_bytes(), 4);
    }

    #[test]
    fn test_hex_error_display() {
        let err = HexError::Parse {
            line: 3,
            message: "bad line".to_string(),
        };
        assert!(format!("{}", err).contains("bad line"));

        let err = HexError::Checksum {
            line: 5,
            expected: 0xAA,
            actual: 0xBB,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("5"));
    }

    #[test]
    fn test_large_continuous_region() {
        // Build a HEX with 256 bytes of data
        let mut hex = String::new();
        for i in 0..16 {
            let offset = i * 16;
            let mut line_data = String::new();
            let mut sum: u8 = 16; // byte count
            sum = sum.wrapping_add((offset >> 8) as u8);
            sum = sum.wrapping_add(offset as u8);
            // Record type 00
            for j in 0..16u8 {
                let byte = ((i as u8) * 16 + j) & 0xFF;
                line_data.push_str(&format!("{:02X}", byte));
                sum = sum.wrapping_add(byte);
            }
            let checksum = (!sum).wrapping_add(1);
            hex.push_str(&format!(
                ":10{:04X}00{}{:02X}\n",
                offset, line_data, checksum
            ));
        }
        hex.push_str(":00000001FF\n");

        let image = HexImage::from_str(&hex).unwrap();
        assert_eq!(image.total_bytes(), 256);
        assert_eq!(image.address_range(), Some((0x0000, 0x00FF)));

        // Verify first and last bytes
        let data = image.extract_range(0x0000, 256, 0xFF);
        assert_eq!(data[0], 0x00);
        assert_eq!(data[255], 0xFF);
    }

    // ── Writer tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_new_and_add_data() {
        let mut img = HexImage::new();
        assert_eq!(img.total_bytes(), 0);
        img.add_data(0x1000, &[0xAA, 0xBB, 0xCC]);
        assert_eq!(img.total_bytes(), 3);
        let data = img.extract_range(0x1000, 3, 0xFF);
        assert_eq!(data, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn test_to_intel_hex_empty() {
        let img = HexImage::new();
        assert_eq!(img.to_intel_hex(), ":00000001FF\n");
    }

    #[test]
    fn test_to_intel_hex_roundtrip_simple() {
        let mut img = HexImage::new();
        img.add_data(0x0000, &[0x01, 0x02, 0x03, 0x04]);
        let hex_str = img.to_intel_hex();
        let parsed = HexImage::from_str(&hex_str).unwrap();
        assert_eq!(parsed.total_bytes(), 4);
        assert_eq!(parsed.extract_range(0x0000, 4, 0xFF), vec![0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_to_intel_hex_roundtrip_high_address() {
        let mut img = HexImage::new();
        img.add_data(0xA038_0000, &[0xDE, 0xAD, 0xBE, 0xEF]);
        let hex_str = img.to_intel_hex();
        assert!(hex_str.contains(":02000004A038"));
        let parsed = HexImage::from_str(&hex_str).unwrap();
        assert_eq!(parsed.extract_range(0xA038_0000, 4, 0xFF), vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_to_intel_hex_roundtrip_large() {
        let payload: Vec<u8> = (0u16..512).map(|i| (i & 0xFF) as u8).collect();
        let mut img = HexImage::new();
        img.add_data(0x8000_0000, &payload);
        let hex_str = img.to_intel_hex();
        let parsed = HexImage::from_str(&hex_str).unwrap();
        assert_eq!(parsed.total_bytes(), 512);
        let recovered = parsed.extract_range(0x8000_0000, 512, 0xFF);
        assert_eq!(recovered, payload);
    }

    #[test]
    fn test_to_intel_hex_multiple_gaps() {
        let mut img = HexImage::new();
        img.add_data(0x0000, &[0xAA, 0xBB]);
        img.add_data(0x1000, &[0xCC, 0xDD]);
        let hex_str = img.to_intel_hex();
        let parsed = HexImage::from_str(&hex_str).unwrap();
        assert_eq!(parsed.extract_range(0x0000, 2, 0xFF), vec![0xAA, 0xBB]);
        assert_eq!(parsed.extract_range(0x1000, 2, 0xFF), vec![0xCC, 0xDD]);
        assert!(!parsed.has_data_in_range(0x0002, 0x0FFE));
    }
}
