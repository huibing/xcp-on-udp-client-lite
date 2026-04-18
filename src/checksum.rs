//! XCP Checksum Calculation and Verification
//!
//! Implements all XCP-standard checksum algorithms (ADD_xx and CRC_xx)
//! and provides a verification engine for comparing master-computed checksums
//! against slave-reported checksums.

use crate::a2l::{A2lMemorySegment, XcpChecksum};
use crate::hex::{HexError, HexImage};
use crate::protocol::Mta;
use crate::slave::{SlaveError, XcpSlave};
use thiserror::Error;

/// Default max block size (0x4000 = 16 KiB) for BUILD_CHECKSUM when no A2L
/// override is provided. Using a smaller block size improves robustness by
/// avoiding ERR_OUT_OF_RANGE on ECUs with limited checksum buffer.
pub const DEFAULT_MAX_BLOCK_SIZE: u32 = 0x4000;

/// Errors from checksum operations
#[derive(Debug, Error)]
pub enum ChecksumError {
    #[error("HEX file error: {0}")]
    Hex(#[from] HexError),

    #[error("Slave communication error: {0}")]
    Slave(#[from] SlaveError),

    #[error("No checksum algorithm specified for segment '{0}'")]
    NoAlgorithm(String),

    #[error("Checksum type mismatch: expected {expected} (code {expected_code}), slave returned code {actual_code}")]
    TypeMismatch {
        expected: String,
        expected_code: u8,
        actual_code: u8,
    },

    #[error("User-defined checksum not supported")]
    UserDefinedNotSupported,

    #[error("No HEX data for segment '{name}' at 0x{address:08X}")]
    NoHexData { name: String, address: u32 },
}

// ---------------------------------------------------------------------------
// Checksum algorithm implementations
// ---------------------------------------------------------------------------

/// Compute checksum over data using the specified XCP algorithm.
/// Returns the 32-bit checksum result.
pub fn compute_checksum(algorithm: &XcpChecksum, data: &[u8]) -> Result<u32, ChecksumError> {
    match algorithm {
        XcpChecksum::Add11 => Ok(checksum_add_11(data)),
        XcpChecksum::Add12 => Ok(checksum_add_12(data)),
        XcpChecksum::Add14 => Ok(checksum_add_14(data)),
        XcpChecksum::Add22 => Ok(checksum_add_22(data)),
        XcpChecksum::Add24 => Ok(checksum_add_24(data)),
        XcpChecksum::Add44 => Ok(checksum_add_44(data)),
        XcpChecksum::Crc16 => Ok(checksum_crc16(data) as u32),
        XcpChecksum::Crc16Citt => Ok(checksum_crc16_citt(data) as u32),
        XcpChecksum::Crc32 => Ok(checksum_crc32(data)),
        XcpChecksum::UserDefined(_) => Err(ChecksumError::UserDefinedNotSupported),
    }
}

/// XCP_ADD_11: byte-wise addition, result truncated to 8 bits
fn checksum_add_11(data: &[u8]) -> u32 {
    let mut sum: u8 = 0;
    for &b in data {
        sum = sum.wrapping_add(b);
    }
    sum as u32
}

/// XCP_ADD_12: byte-wise addition, result truncated to 16 bits
fn checksum_add_12(data: &[u8]) -> u32 {
    let mut sum: u16 = 0;
    for &b in data {
        sum = sum.wrapping_add(b as u16);
    }
    sum as u32
}

/// XCP_ADD_14: byte-wise addition, result as 32 bits
fn checksum_add_14(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    for &b in data {
        sum = sum.wrapping_add(b as u32);
    }
    sum
}

/// XCP_ADD_22: 16-bit word-wise addition (little-endian), result 16 bits
fn checksum_add_22(data: &[u8]) -> u32 {
    let mut sum: u16 = 0;
    for chunk in data.chunks(2) {
        let word = if chunk.len() == 2 {
            u16::from_le_bytes([chunk[0], chunk[1]])
        } else {
            chunk[0] as u16
        };
        sum = sum.wrapping_add(word);
    }
    sum as u32
}

/// XCP_ADD_24: 16-bit word-wise addition (little-endian), result 32 bits
fn checksum_add_24(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    for chunk in data.chunks(2) {
        let word = if chunk.len() == 2 {
            u16::from_le_bytes([chunk[0], chunk[1]])
        } else {
            chunk[0] as u16
        };
        sum = sum.wrapping_add(word as u32);
    }
    sum
}

/// XCP_ADD_44: 32-bit dword-wise addition (little-endian), result 32 bits
fn checksum_add_44(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    for chunk in data.chunks(4) {
        let dword = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => chunk[0] as u32,
            _ => 0,
        };
        sum = sum.wrapping_add(dword);
    }
    sum
}

/// XCP_CRC_16: CRC-16 with polynomial 0x8005
/// Init=0x0000, RefIn=true, RefOut=true, XorOut=0x0000
fn checksum_crc16(data: &[u8]) -> u16 {
    const CRC16: crc::Algorithm<u16> = crc::Algorithm {
        width: 16,
        poly: 0x8005,
        init: 0x0000,
        refin: true,
        refout: true,
        xorout: 0x0000,
        check: 0xBB3D,
        residue: 0x0000,
    };
    let crc = crc::Crc::<u16>::new(&CRC16);
    crc.checksum(data)
}

/// XCP_CRC_16_CITT: CRC-16 CCITT with polynomial 0x1021
/// Init=0xFFFF, RefIn=false, RefOut=false, XorOut=0x0000
fn checksum_crc16_citt(data: &[u8]) -> u16 {
    const CRC16_CCITT: crc::Algorithm<u16> = crc::Algorithm {
        width: 16,
        poly: 0x1021,
        init: 0xFFFF,
        refin: false,
        refout: false,
        xorout: 0x0000,
        check: 0x29B1,
        residue: 0x0000,
    };
    let crc = crc::Crc::<u16>::new(&CRC16_CCITT);
    crc.checksum(data)
}

/// XCP_CRC_32: IEEE 802.3 CRC-32 with polynomial 0x04C11DB7
/// Init=0xFFFFFFFF, RefIn=true, RefOut=true, XorOut=0xFFFFFFFF
fn checksum_crc32(data: &[u8]) -> u32 {
    let crc = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
    crc.checksum(data)
}

/// Check if an algorithm is addition-based (partial sums can be combined)
pub fn is_additive(algorithm: &XcpChecksum) -> bool {
    matches!(
        algorithm,
        XcpChecksum::Add11
            | XcpChecksum::Add12
            | XcpChecksum::Add14
            | XcpChecksum::Add22
            | XcpChecksum::Add24
            | XcpChecksum::Add44
    )
}

// ---------------------------------------------------------------------------
// Verification engine types
// ---------------------------------------------------------------------------

/// Result of verifying one chunk within a segment (used when splitting large segments)
#[derive(Debug, Clone)]
pub struct ChunkResult {
    /// Chunk offset from segment base address
    pub offset: u32,
    /// Chunk size in bytes
    pub size: u32,
    /// Master-computed checksum for this chunk
    pub master_checksum: u32,
    /// Slave-reported checksum for this chunk
    pub slave_checksum: u32,
    /// Whether this chunk's checksums match
    pub passed: bool,
}

/// Result of verifying one memory segment
#[derive(Debug, Clone)]
pub struct SegmentResult {
    /// Segment name from A2L
    pub name: String,
    /// Base address
    pub address: u32,
    /// Segment size in bytes
    pub size: u32,
    /// Algorithm used
    pub algorithm: String,
    /// Page number (if multi-page)
    pub page: Option<u8>,
    /// Master-computed checksum (or CRC-of-CRCs for chunked CRC segments)
    pub master_checksum: u32,
    /// Slave-reported checksum (or CRC-of-CRCs for chunked CRC segments)
    pub slave_checksum: u32,
    /// Whether checksums match
    pub passed: bool,
    /// Status message (details on skip/failure)
    pub status: SegmentStatus,
    /// Per-chunk details (populated when segment was split into chunks)
    pub chunks: Option<Vec<ChunkResult>>,
}

/// Status of a segment verification
#[derive(Debug, Clone)]
pub enum SegmentStatus {
    Pass,
    Fail,
    Skipped(String),
    Error(String),
}

impl std::fmt::Display for SegmentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SegmentStatus::Pass => write!(f, "PASS"),
            SegmentStatus::Fail => write!(f, "FAIL"),
            SegmentStatus::Skipped(reason) => write!(f, "SKIPPED: {}", reason),
            SegmentStatus::Error(msg) => write!(f, "ERROR: {}", msg),
        }
    }
}

/// Summary of a full verification run
#[derive(Debug, Clone)]
pub struct VerificationReport {
    /// Per-segment results
    pub results: Vec<SegmentResult>,
}

impl VerificationReport {
    pub fn total_segments(&self) -> usize {
        self.results.len()
    }

    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }

    pub fn failed(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.status, SegmentStatus::Fail))
            .count()
    }

    pub fn skipped(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.status, SegmentStatus::Skipped(_)))
            .count()
    }

    pub fn errors(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.status, SegmentStatus::Error(_)))
            .count()
    }

    pub fn all_passed(&self) -> bool {
        self.failed() == 0 && self.errors() == 0
    }

    /// Get a formatted summary string
    pub fn summary(&self) -> String {
        format!(
            "Verification: {} total, {} passed, {} failed, {} skipped, {} errors",
            self.total_segments(),
            self.passed(),
            self.failed(),
            self.skipped(),
            self.errors()
        )
    }
}

// ---------------------------------------------------------------------------
// Verification engine
// ---------------------------------------------------------------------------

/// Verify memory segments by comparing master-computed checksums (from HEX file)
/// against slave-reported checksums (via BUILD_CHECKSUM command).
///
/// For each segment with a checksum algorithm defined in the A2L:
/// 1. Extract the data from the HEX image for that address range
/// 2. Compute the checksum locally using the algorithm from A2L
/// 3. Ask the slave to compute its checksum via SET_MTA + BUILD_CHECKSUM
/// 4. Compare both results
///
/// If a segment has multiple pages, each page is verified separately using
/// SET_CAL_PAGE before the slave BUILD_CHECKSUM.
/// After verification, pages are restored to their original state.
pub async fn verify_segments(
    slave: &mut XcpSlave,
    hex_image: &HexImage,
    segments: &[A2lMemorySegment],
    max_block_size_override: Option<u32>,
) -> Result<VerificationReport, ChecksumError> {
    let mut results = Vec::new();

    // Save current page state before verification so we can restore afterward
    let saved_pages: Vec<(u8, u8)> = {
        let mut pages = Vec::new();
        for segment in segments {
            if let Some(xcp_info) = &segment.xcp_info {
                if xcp_info.num_pages > 1 {
                    match slave.get_cal_page(xcp_info.segment_number, 0x02).await {
                        Ok(page) => pages.push((xcp_info.segment_number, page)),
                        Err(e) => {
                            tracing::warn!(
                                "Failed to save page state for segment {}: {:?}",
                                xcp_info.segment_number,
                                e
                            );
                        }
                    }
                }
            }
        }
        pages
    };

    for segment in segments {
        let xcp_info = match &segment.xcp_info {
            Some(info) => info,
            None => {
                results.push(SegmentResult {
                    name: segment.name.clone(),
                    address: segment.address,
                    size: segment.size,
                    algorithm: "N/A".to_string(),
                    page: None,
                    master_checksum: 0,
                    slave_checksum: 0,
                    passed: false,
                    status: SegmentStatus::Skipped("No XCP segment info".to_string()),
                    chunks: None,
                });
                continue;
            }
        };

        let algorithm = match &xcp_info.checksum {
            Some(algo) => algo,
            None => {
                results.push(SegmentResult {
                    name: segment.name.clone(),
                    address: segment.address,
                    size: segment.size,
                    algorithm: "N/A".to_string(),
                    page: None,
                    master_checksum: 0,
                    slave_checksum: 0,
                    passed: false,
                    status: SegmentStatus::Skipped("No checksum algorithm defined".to_string()),
                    chunks: None,
                });
                continue;
            }
        };

        if matches!(algorithm, XcpChecksum::UserDefined(_)) {
            results.push(SegmentResult {
                name: segment.name.clone(),
                address: segment.address,
                size: segment.size,
                algorithm: algorithm.name().to_string(),
                page: None,
                master_checksum: 0,
                slave_checksum: 0,
                passed: false,
                status: SegmentStatus::Skipped("User-defined checksum not supported".to_string()),
                chunks: None,
            });
            continue;
        }

        // Check if HEX has data for this segment
        if !hex_image.has_data_in_range(segment.address, segment.size) {
            results.push(SegmentResult {
                name: segment.name.clone(),
                address: segment.address,
                size: segment.size,
                algorithm: algorithm.name().to_string(),
                page: None,
                master_checksum: 0,
                slave_checksum: 0,
                passed: false,
                status: SegmentStatus::Skipped(format!("No HEX data at 0x{:08X}", segment.address)),
                chunks: None,
            });
            continue;
        }

        // Extract data from HEX for this segment
        let hex_data = hex_image.extract_range(segment.address, segment.size, 0xFF);
        let master_checksum = compute_checksum(algorithm, &hex_data)?;

        // Determine max block size
        let max_block = max_block_size_override.unwrap_or(DEFAULT_MAX_BLOCK_SIZE);

        // If segment has multiple pages, verify each page
        if xcp_info.num_pages > 1 {
            for page in &xcp_info.pages {
                let page_results = verify_segment_on_slave(
                    slave,
                    &segment.name,
                    segment.address,
                    segment.size,
                    xcp_info.address_extension,
                    algorithm,
                    master_checksum,
                    &hex_data,
                    max_block,
                    Some((xcp_info.segment_number, page.page_number)),
                )
                .await;
                results.push(page_results);
            }
        } else {
            let result = verify_segment_on_slave(
                slave,
                &segment.name,
                segment.address,
                segment.size,
                xcp_info.address_extension,
                algorithm,
                master_checksum,
                &hex_data,
                max_block,
                None,
            )
            .await;
            results.push(result);
        }
    }

    // Restore original page state
    for (seg_num, original_page) in &saved_pages {
        if let Err(e) = slave.set_cal_page(*seg_num, *original_page, 0x03).await {
            tracing::warn!(
                "Failed to restore page {} for segment {}: {:?}",
                original_page,
                seg_num,
                e
            );
        } else {
            tracing::info!(
                "Restored segment {} to original page {}",
                seg_num,
                original_page
            );
        }
    }

    Ok(VerificationReport { results })
}

/// Verify a single segment against the slave.
/// If `page` is Some((segment_number, page_number)), SET_CAL_PAGE is called first.
///
/// `hex_data` is the full segment data extracted from the HEX file, used for
/// per-chunk master CRC computation when the segment must be split.
async fn verify_segment_on_slave(
    slave: &mut XcpSlave,
    name: &str,
    address: u32,
    size: u32,
    address_extension: u8,
    algorithm: &XcpChecksum,
    master_checksum: u32,
    hex_data: &[u8],
    max_block_size: u32,
    page: Option<(u8, u8)>,
) -> SegmentResult {
    // Switch page if needed
    if let Some((seg_num, page_num)) = page {
        // mode 0x03 = both ECU and XCP access
        if let Err(e) = slave.set_cal_page(seg_num, page_num, 0x03).await {
            return SegmentResult {
                name: name.to_string(),
                address,
                size,
                algorithm: algorithm.name().to_string(),
                page: Some(page_num),
                master_checksum,
                slave_checksum: 0,
                passed: false,
                status: SegmentStatus::Error(format!("Failed to set cal page: {}", e)),
                chunks: None,
            };
        }
    }

    // Build checksum on slave, handling block size splitting
    match slave_build_checksum(
        slave,
        address,
        size,
        address_extension,
        algorithm,
        hex_data,
        max_block_size,
    )
    .await
    {
        Ok(slave_result) => {
            // Verify the returned checksum type matches expected
            let expected_code = algorithm.to_code();
            if slave_result.checksum_type != expected_code {
                tracing::warn!(
                    "Checksum type mismatch for segment '{}': expected {} (code {}), slave returned code {}",
                    name, algorithm.name(), expected_code, slave_result.checksum_type
                );
            }

            let passed = slave_result.combined_master_checksum
                == slave_result.combined_slave_checksum
                && slave_result.all_chunks_passed;
            SegmentResult {
                name: name.to_string(),
                address,
                size,
                algorithm: algorithm.name().to_string(),
                page: page.map(|(_, p)| p),
                master_checksum: slave_result.combined_master_checksum,
                slave_checksum: slave_result.combined_slave_checksum,
                passed,
                status: if passed {
                    SegmentStatus::Pass
                } else {
                    SegmentStatus::Fail
                },
                chunks: slave_result.chunks,
            }
        }
        Err(e) => SegmentResult {
            name: name.to_string(),
            address,
            size,
            algorithm: algorithm.name().to_string(),
            page: page.map(|(_, p)| p),
            master_checksum,
            slave_checksum: 0,
            passed: false,
            status: SegmentStatus::Error(format!("Slave error: {}", e)),
            chunks: None,
        },
    }
}

/// Result of a (possibly chunked) BUILD_CHECKSUM operation
struct BuildChecksumResult {
    /// Checksum type code returned by the slave
    checksum_type: u8,
    /// Combined master checksum (original for single-block, CRC-of-CRCs for chunked CRC)
    combined_master_checksum: u32,
    /// Combined slave checksum (original for single-block, CRC-of-CRCs for chunked CRC)
    combined_slave_checksum: u32,
    /// Whether all individual chunk comparisons passed
    all_chunks_passed: bool,
    /// Per-chunk detail (None if no splitting was needed)
    chunks: Option<Vec<ChunkResult>>,
}

/// Compute a CRC-32 over a sequence of u32 checksum values (meta-CRC).
/// Each u32 is fed in little-endian byte order.
fn meta_crc32(checksums: &[u32]) -> u32 {
    let crc = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
    let mut digest = crc.digest();
    for &cs in checksums {
        digest.update(&cs.to_le_bytes());
    }
    digest.finalize()
}

/// Execute BUILD_CHECKSUM on the slave, handling block size splitting.
///
/// For addition checksums: split into chunks, sum partial results on the slave side.
/// The master computes one checksum over the full segment and compares against the
/// combined slave result.
///
/// For CRC checksums: split into chunks, compute and verify each chunk's CRC
/// independently (both master and slave). If any chunk mismatches, the segment fails.
/// A final "CRC-of-CRCs" is produced by feeding all per-chunk checksum values into
/// CRC-32, giving a single summary value for display.
async fn slave_build_checksum(
    slave: &mut XcpSlave,
    address: u32,
    total_size: u32,
    address_extension: u8,
    algorithm: &XcpChecksum,
    hex_data: &[u8],
    max_block_size: u32,
) -> Result<BuildChecksumResult, SlaveError> {
    if total_size <= max_block_size {
        // Single block — straightforward
        let (ctype, slave_cs) = slave
            .build_checksum(Mta::new(address, address_extension), total_size)
            .await?;
        let master_cs = compute_checksum(algorithm, hex_data)
            .map_err(|e| SlaveError::ParseError(format!("Master checksum error: {}", e)))?;
        return Ok(BuildChecksumResult {
            checksum_type: ctype,
            combined_master_checksum: master_cs,
            combined_slave_checksum: slave_cs,
            all_chunks_passed: master_cs == slave_cs,
            chunks: None,
        });
    }

    // Need to split into multiple blocks
    let mut offset: u32 = 0;
    let mut checksum_type: u8 = 0;

    if is_additive(algorithm) {
        // For additive checksums, sum partial slave results
        let mut combined: u32 = 0;
        while offset < total_size {
            let block_size = std::cmp::min(max_block_size, total_size - offset);
            let current_addr = address.wrapping_add(offset);
            let (ctype, partial) = slave
                .build_checksum(Mta::new(current_addr, address_extension), block_size)
                .await?;
            checksum_type = ctype;

            combined = match algorithm {
                XcpChecksum::Add11 => ((combined as u8).wrapping_add(partial as u8)) as u32,
                XcpChecksum::Add12 | XcpChecksum::Add22 => {
                    ((combined as u16).wrapping_add(partial as u16)) as u32
                }
                _ => combined.wrapping_add(partial),
            };

            offset += block_size;
        }
        // Master checksum is computed over the full segment data
        let master_cs = compute_checksum(algorithm, hex_data)
            .map_err(|e| SlaveError::ParseError(format!("Master checksum error: {}", e)))?;
        Ok(BuildChecksumResult {
            checksum_type,
            combined_master_checksum: master_cs,
            combined_slave_checksum: combined,
            all_chunks_passed: master_cs == combined,
            chunks: None,
        })
    } else {
        // CRC checksums: verify each chunk independently, then produce CRC-of-CRCs
        let mut chunk_results = Vec::new();
        let mut master_checksums = Vec::new();
        let mut slave_checksums = Vec::new();
        let mut all_passed = true;

        while offset < total_size {
            let block_size = std::cmp::min(max_block_size, total_size - offset);
            let current_addr = address.wrapping_add(offset);

            // Slave computes CRC on this chunk
            let (ctype, slave_chunk_cs) = slave
                .build_checksum(Mta::new(current_addr, address_extension), block_size)
                .await?;
            checksum_type = ctype;

            // Master computes CRC on the same chunk from HEX data
            let chunk_start = offset as usize;
            let chunk_end = (offset + block_size) as usize;
            let chunk_data = &hex_data[chunk_start..chunk_end];
            let master_chunk_cs = compute_checksum(algorithm, chunk_data)
                .map_err(|e| SlaveError::ParseError(format!("Master checksum error: {}", e)))?;

            let chunk_passed = master_chunk_cs == slave_chunk_cs;
            if !chunk_passed {
                all_passed = false;
                tracing::warn!(
                    "CRC chunk mismatch at offset 0x{:08X} (size {}): master=0x{:08X} slave=0x{:08X}",
                    offset, block_size, master_chunk_cs, slave_chunk_cs
                );
            }

            chunk_results.push(ChunkResult {
                offset,
                size: block_size,
                master_checksum: master_chunk_cs,
                slave_checksum: slave_chunk_cs,
                passed: chunk_passed,
            });

            master_checksums.push(master_chunk_cs);
            slave_checksums.push(slave_chunk_cs);

            offset += block_size;
        }

        // Produce CRC-of-CRCs summary values
        let combined_master = meta_crc32(&master_checksums);
        let combined_slave = meta_crc32(&slave_checksums);

        Ok(BuildChecksumResult {
            checksum_type,
            combined_master_checksum: combined_master,
            combined_slave_checksum: combined_slave,
            all_chunks_passed: all_passed,
            chunks: Some(chunk_results),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test vector from the design document
    fn test_pattern() -> Vec<u8> {
        vec![
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFB, 0xFC,
            0xFD, 0xFE, 0xFF, 0x00,
        ]
    }

    #[test]
    fn test_add_11() {
        let data = test_pattern();
        assert_eq!(checksum_add_11(&data), 0x10);
    }

    #[test]
    fn test_add_12() {
        let data = test_pattern();
        assert_eq!(checksum_add_12(&data), 0x0F10);
    }

    #[test]
    fn test_add_14() {
        let data = test_pattern();
        assert_eq!(checksum_add_14(&data), 0x00000F10);
    }

    #[test]
    fn test_add_22() {
        let data = test_pattern();
        assert_eq!(checksum_add_22(&data), 0x1800);
    }

    #[test]
    fn test_add_24() {
        let data = test_pattern();
        assert_eq!(checksum_add_24(&data), 0x00071800);
    }

    #[test]
    fn test_add_44() {
        let data = test_pattern();
        assert_eq!(checksum_add_44(&data), 0x140C03F8);
    }

    #[test]
    fn test_crc_16() {
        let data = test_pattern();
        assert_eq!(checksum_crc16(&data), 0xC76A);
    }

    #[test]
    fn test_crc_16_citt() {
        let data = test_pattern();
        assert_eq!(checksum_crc16_citt(&data), 0x9D50);
    }

    #[test]
    fn test_crc_32() {
        let data = test_pattern();
        assert_eq!(checksum_crc32(&data), 0x89CD97CE);
    }

    #[test]
    fn test_compute_checksum_dispatch() {
        let data = test_pattern();
        assert_eq!(
            compute_checksum(&XcpChecksum::Add44, &data).unwrap(),
            0x140C03F8
        );
        assert_eq!(
            compute_checksum(&XcpChecksum::Crc32, &data).unwrap(),
            0x89CD97CE
        );
    }

    #[test]
    fn test_compute_checksum_user_defined_error() {
        let data = test_pattern();
        let result = compute_checksum(&XcpChecksum::UserDefined(255), &data);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_additive() {
        assert!(is_additive(&XcpChecksum::Add11));
        assert!(is_additive(&XcpChecksum::Add44));
        assert!(!is_additive(&XcpChecksum::Crc16));
        assert!(!is_additive(&XcpChecksum::Crc32));
    }

    #[test]
    fn test_empty_data() {
        let data: Vec<u8> = vec![];
        assert_eq!(checksum_add_11(&data), 0);
        assert_eq!(checksum_add_14(&data), 0);
        assert_eq!(checksum_add_44(&data), 0);
    }

    #[test]
    fn test_verification_report_summary() {
        let report = VerificationReport {
            results: vec![
                SegmentResult {
                    name: "seg0".to_string(),
                    address: 0x1000,
                    size: 256,
                    algorithm: "XCP_CRC_32".to_string(),
                    page: None,
                    master_checksum: 0xAABBCCDD,
                    slave_checksum: 0xAABBCCDD,
                    passed: true,
                    status: SegmentStatus::Pass,
                    chunks: None,
                },
                SegmentResult {
                    name: "seg1".to_string(),
                    address: 0x2000,
                    size: 128,
                    algorithm: "XCP_CRC_32".to_string(),
                    page: None,
                    master_checksum: 0x11223344,
                    slave_checksum: 0x55667788,
                    passed: false,
                    status: SegmentStatus::Fail,
                    chunks: None,
                },
            ],
        };

        assert_eq!(report.total_segments(), 2);
        assert_eq!(report.passed(), 1);
        assert_eq!(report.failed(), 1);
        assert!(!report.all_passed());
    }

    #[test]
    fn test_xcp_checksum_code_roundtrip() {
        let algorithms = vec![
            (XcpChecksum::Add11, 1u8),
            (XcpChecksum::Add12, 2),
            (XcpChecksum::Add14, 3),
            (XcpChecksum::Add22, 4),
            (XcpChecksum::Add24, 5),
            (XcpChecksum::Add44, 6),
            (XcpChecksum::Crc16, 7),
            (XcpChecksum::Crc16Citt, 8),
            (XcpChecksum::Crc32, 9),
        ];

        for (algo, expected_code) in algorithms {
            assert_eq!(algo.to_code(), expected_code);
            assert_eq!(XcpChecksum::from_code(expected_code).unwrap(), algo);
        }
    }

    #[test]
    fn test_meta_crc32_deterministic() {
        // Same input should always produce the same output
        let checksums = vec![0x89CD97CE_u32, 0xAABBCCDD, 0x11223344];
        let result1 = meta_crc32(&checksums);
        let result2 = meta_crc32(&checksums);
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_meta_crc32_order_matters() {
        let a = meta_crc32(&[0x11223344, 0x55667788]);
        let b = meta_crc32(&[0x55667788, 0x11223344]);
        assert_ne!(a, b, "CRC-of-CRCs should be order-dependent");
    }

    #[test]
    fn test_meta_crc32_single_value() {
        // Meta-CRC of a single u32 value is the CRC-32 of its 4 LE bytes
        let val = 0xDEADBEEF_u32;
        let result = meta_crc32(&[val]);
        let expected = checksum_crc32(&val.to_le_bytes());
        assert_eq!(result, expected);
    }

    #[test]
    fn test_meta_crc32_matches_identical_produce_same_result() {
        // If master and slave have the same per-chunk CRCs,
        // their meta-CRC must also match
        let chunk_csums = vec![0x12345678, 0xABCDEF01, 0x99887766];
        let master_meta = meta_crc32(&chunk_csums);
        let slave_meta = meta_crc32(&chunk_csums);
        assert_eq!(master_meta, slave_meta);
    }

    #[test]
    fn test_chunk_result_fields() {
        let cr = ChunkResult {
            offset: 0x1000,
            size: 256,
            master_checksum: 0xAABB,
            slave_checksum: 0xAABB,
            passed: true,
        };
        assert!(cr.passed);
        assert_eq!(cr.offset, 0x1000);
        assert_eq!(cr.size, 256);
    }

    #[test]
    fn test_segment_result_with_chunks() {
        let result = SegmentResult {
            name: "flash".to_string(),
            address: 0x0800_0000,
            size: 1024,
            algorithm: "XCP_CRC_32".to_string(),
            page: None,
            master_checksum: 0x1234,
            slave_checksum: 0x1234,
            passed: true,
            status: SegmentStatus::Pass,
            chunks: Some(vec![
                ChunkResult {
                    offset: 0,
                    size: 512,
                    master_checksum: 0xAAAA,
                    slave_checksum: 0xAAAA,
                    passed: true,
                },
                ChunkResult {
                    offset: 512,
                    size: 512,
                    master_checksum: 0xBBBB,
                    slave_checksum: 0xBBBB,
                    passed: true,
                },
            ]),
        };
        assert!(result.passed);
        assert_eq!(result.chunks.as_ref().unwrap().len(), 2);
        assert!(result.chunks.as_ref().unwrap().iter().all(|c| c.passed));
    }

    #[test]
    fn test_per_chunk_crc_verification_logic() {
        // Simulate what happens in chunked CRC mode:
        // Split 32-byte test_pattern into two 16-byte chunks
        let data = test_pattern();
        let chunk1 = &data[0..16];
        let chunk2 = &data[16..32];

        // Compute per-chunk CRCs
        let crc1 = checksum_crc32(chunk1);
        let crc2 = checksum_crc32(chunk2);

        // Each chunk CRC should be different from the full-data CRC
        let full_crc = checksum_crc32(&data);
        assert_ne!(
            meta_crc32(&[crc1, crc2]),
            full_crc,
            "CRC-of-CRCs should differ from CRC of whole data (CRC is not composable)"
        );

        // But if both sides compute the same per-chunk CRCs, meta-CRC matches
        let master_meta = meta_crc32(&[crc1, crc2]);
        let slave_meta = meta_crc32(&[crc1, crc2]);
        assert_eq!(master_meta, slave_meta);
    }

    #[test]
    fn test_meta_crc32_empty_input() {
        // CRC-32 of empty data should be deterministic
        let result = meta_crc32(&[]);
        // CRC-32 of empty input = 0x00000000 (after XOR with 0xFFFFFFFF init and final)
        let expected = checksum_crc32(&[]);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_additive_chunking_add11() {
        // ADD_11 should produce the same result whether computed in one shot
        // or split into chunks and summed (wrapping u8 addition)
        let data = test_pattern();
        let full = checksum_add_11(&data);

        // Split into 4 chunks of 8 bytes each
        let c1 = checksum_add_11(&data[0..8]);
        let c2 = checksum_add_11(&data[8..16]);
        let c3 = checksum_add_11(&data[16..24]);
        let c4 = checksum_add_11(&data[24..32]);
        let combined = (c1 as u8)
            .wrapping_add(c2 as u8)
            .wrapping_add(c3 as u8)
            .wrapping_add(c4 as u8) as u32;
        assert_eq!(
            combined, full,
            "ADD_11 should be composable via wrapping u8 add"
        );
    }

    #[test]
    fn test_additive_chunking_add12() {
        let data = test_pattern();
        let full = checksum_add_12(&data);

        let c1 = checksum_add_12(&data[0..16]);
        let c2 = checksum_add_12(&data[16..32]);
        let combined = ((c1 as u16).wrapping_add(c2 as u16)) as u32;
        assert_eq!(
            combined, full,
            "ADD_12 should be composable via wrapping u16 add"
        );
    }

    #[test]
    fn test_additive_chunking_add14() {
        let data = test_pattern();
        let full = checksum_add_14(&data);

        let c1 = checksum_add_14(&data[0..16]);
        let c2 = checksum_add_14(&data[16..32]);
        let combined = c1.wrapping_add(c2);
        assert_eq!(
            combined, full,
            "ADD_14 should be composable via wrapping u32 add"
        );
    }

    #[test]
    fn test_additive_chunking_add22() {
        let data = test_pattern();
        let full = checksum_add_22(&data);

        // Split into two 16-byte chunks (each has 8 u16 words)
        let c1 = checksum_add_22(&data[0..16]);
        let c2 = checksum_add_22(&data[16..32]);
        let combined = ((c1 as u16).wrapping_add(c2 as u16)) as u32;
        assert_eq!(
            combined, full,
            "ADD_22 should be composable via wrapping u16 add"
        );
    }

    #[test]
    fn test_additive_chunking_add24() {
        let data = test_pattern();
        let full = checksum_add_24(&data);

        let c1 = checksum_add_24(&data[0..16]);
        let c2 = checksum_add_24(&data[16..32]);
        let combined = c1.wrapping_add(c2);
        assert_eq!(
            combined, full,
            "ADD_24 should be composable via wrapping u32 add"
        );
    }

    #[test]
    fn test_additive_chunking_add44() {
        let data = test_pattern();
        let full = checksum_add_44(&data);

        // Split into four 8-byte chunks (each has 2 u32 words)
        let c1 = checksum_add_44(&data[0..8]);
        let c2 = checksum_add_44(&data[8..16]);
        let c3 = checksum_add_44(&data[16..24]);
        let c4 = checksum_add_44(&data[24..32]);
        let combined = c1.wrapping_add(c2).wrapping_add(c3).wrapping_add(c4);
        assert_eq!(
            combined, full,
            "ADD_44 should be composable via wrapping u32 add"
        );
    }

    #[test]
    fn test_crc_not_composable() {
        // Verify that CRC is NOT composable: CRC(A||B) != CRC(A) + CRC(B) etc.
        let data = test_pattern();
        let full_crc16 = checksum_crc16(&data);
        let full_crc16_citt = checksum_crc16_citt(&data);
        let full_crc32 = checksum_crc32(&data);

        let c1_16 = checksum_crc16(&data[0..16]);
        let c2_16 = checksum_crc16(&data[16..32]);
        assert_ne!(
            (c1_16 as u16).wrapping_add(c2_16 as u16) as u16,
            full_crc16 as u16,
            "CRC-16 should not be composable by addition"
        );

        let c1_citt = checksum_crc16_citt(&data[0..16]);
        let c2_citt = checksum_crc16_citt(&data[16..32]);
        assert_ne!(
            (c1_citt as u16).wrapping_add(c2_citt as u16) as u16,
            full_crc16_citt as u16,
            "CRC-16-CITT should not be composable by addition"
        );

        let c1_32 = checksum_crc32(&data[0..16]);
        let c2_32 = checksum_crc32(&data[16..32]);
        assert_ne!(
            c1_32.wrapping_add(c2_32),
            full_crc32,
            "CRC-32 should not be composable by addition"
        );
    }

    #[test]
    fn test_single_byte_checksums() {
        let data = vec![0x42];
        assert_eq!(checksum_add_11(&data), 0x42);
        assert_eq!(checksum_add_12(&data), 0x42);
        assert_eq!(checksum_add_14(&data), 0x42);
        // ADD_22/24/44 need alignment, single byte gets zero-padded in practice
        // but the function reads by word size, so single byte truncates to empty
        assert_eq!(checksum_add_22(&[0x42, 0x00]), 0x0042);
    }

    #[test]
    fn test_segment_status_display() {
        assert_eq!(format!("{}", SegmentStatus::Pass), "PASS");
        assert_eq!(format!("{}", SegmentStatus::Fail), "FAIL");
        assert_eq!(
            format!("{}", SegmentStatus::Skipped("no data".to_string())),
            "SKIPPED: no data"
        );
        assert_eq!(
            format!("{}", SegmentStatus::Error("timeout".to_string())),
            "ERROR: timeout"
        );
    }

    #[test]
    fn test_verification_report_all_skipped() {
        let report = VerificationReport {
            results: vec![SegmentResult {
                name: "seg0".to_string(),
                address: 0x1000,
                size: 256,
                algorithm: "N/A".to_string(),
                page: None,
                master_checksum: 0,
                slave_checksum: 0,
                passed: false,
                status: SegmentStatus::Skipped("No algorithm".to_string()),
                chunks: None,
            }],
        };
        assert_eq!(report.total_segments(), 1);
        assert_eq!(report.passed(), 0);
        assert_eq!(report.failed(), 0);
        assert_eq!(report.skipped(), 1);
        assert!(
            report.all_passed(),
            "All-skipped should count as all_passed (no failures or errors)"
        );
    }

    #[test]
    fn test_verification_report_with_errors() {
        let report = VerificationReport {
            results: vec![SegmentResult {
                name: "seg0".to_string(),
                address: 0x1000,
                size: 256,
                algorithm: "XCP_CRC_32".to_string(),
                page: None,
                master_checksum: 0,
                slave_checksum: 0,
                passed: false,
                status: SegmentStatus::Error("Timeout".to_string()),
                chunks: None,
            }],
        };
        assert_eq!(report.errors(), 1);
        assert!(
            !report.all_passed(),
            "Should not be all_passed if there are errors"
        );
    }

    #[test]
    fn test_verification_report_mixed_with_chunks() {
        let report = VerificationReport {
            results: vec![
                SegmentResult {
                    name: "flash_a".to_string(),
                    address: 0x0800_0000,
                    size: 2048,
                    algorithm: "XCP_CRC_32".to_string(),
                    page: None,
                    master_checksum: 0x1234,
                    slave_checksum: 0x1234,
                    passed: true,
                    status: SegmentStatus::Pass,
                    chunks: Some(vec![
                        ChunkResult {
                            offset: 0,
                            size: 1024,
                            master_checksum: 0xAA,
                            slave_checksum: 0xAA,
                            passed: true,
                        },
                        ChunkResult {
                            offset: 1024,
                            size: 1024,
                            master_checksum: 0xBB,
                            slave_checksum: 0xBB,
                            passed: true,
                        },
                    ]),
                },
                SegmentResult {
                    name: "flash_b".to_string(),
                    address: 0x0801_0000,
                    size: 2048,
                    algorithm: "XCP_CRC_32".to_string(),
                    page: None,
                    master_checksum: 0x5678,
                    slave_checksum: 0x9ABC,
                    passed: false,
                    status: SegmentStatus::Fail,
                    chunks: Some(vec![
                        ChunkResult {
                            offset: 0,
                            size: 1024,
                            master_checksum: 0xCC,
                            slave_checksum: 0xCC,
                            passed: true,
                        },
                        ChunkResult {
                            offset: 1024,
                            size: 1024,
                            master_checksum: 0xDD,
                            slave_checksum: 0xEE,
                            passed: false,
                        },
                    ]),
                },
            ],
        };

        assert_eq!(report.total_segments(), 2);
        assert_eq!(report.passed(), 1);
        assert_eq!(report.failed(), 1);
        assert!(!report.all_passed());

        // Check chunk details
        let first = &report.results[0];
        assert!(first.chunks.as_ref().unwrap().iter().all(|c| c.passed));

        let second = &report.results[1];
        let failing_chunks: Vec<_> = second
            .chunks
            .as_ref()
            .unwrap()
            .iter()
            .filter(|c| !c.passed)
            .collect();
        assert_eq!(failing_chunks.len(), 1);
        assert_eq!(failing_chunks[0].offset, 1024);
    }

    #[test]
    fn test_per_chunk_crc16_verification() {
        // Verify per-chunk CRC-16 works correctly
        let data = test_pattern();
        let chunk1 = &data[0..16];
        let chunk2 = &data[16..32];

        let crc16_c1 = checksum_crc16(chunk1);
        let crc16_c2 = checksum_crc16(chunk2);

        // Each chunk CRC should be individually verifiable
        assert_eq!(crc16_c1, checksum_crc16(chunk1));
        assert_eq!(crc16_c2, checksum_crc16(chunk2));

        // Meta CRC using u32 representation
        let meta = meta_crc32(&[crc16_c1 as u32, crc16_c2 as u32]);
        assert_ne!(meta, 0, "Meta CRC should be non-zero for non-trivial data");
    }

    #[test]
    fn test_per_chunk_crc16_citt_verification() {
        let data = test_pattern();
        let chunk1 = &data[0..16];
        let chunk2 = &data[16..32];

        let c1 = checksum_crc16_citt(chunk1);
        let c2 = checksum_crc16_citt(chunk2);

        let meta = meta_crc32(&[c1 as u32, c2 as u32]);
        assert_ne!(meta, 0);
        // Verify consistency
        assert_eq!(meta, meta_crc32(&[c1 as u32, c2 as u32]));
    }

    #[test]
    fn test_meta_crc32_different_chunk_counts() {
        // Splitting into different chunk counts should produce different meta-CRCs
        let data = test_pattern();

        // 2 chunks
        let meta_2 = meta_crc32(&[checksum_crc32(&data[0..16]), checksum_crc32(&data[16..32])]);

        // 4 chunks
        let meta_4 = meta_crc32(&[
            checksum_crc32(&data[0..8]),
            checksum_crc32(&data[8..16]),
            checksum_crc32(&data[16..24]),
            checksum_crc32(&data[24..32]),
        ]);

        assert_ne!(
            meta_2, meta_4,
            "Different chunk sizes should produce different meta-CRCs"
        );
    }

    #[test]
    fn test_checksum_error_display() {
        let err = ChecksumError::NoAlgorithm("seg0".to_string());
        assert!(format!("{}", err).contains("seg0"));

        let err = ChecksumError::UserDefinedNotSupported;
        assert!(format!("{}", err).contains("User-defined"));

        let err = ChecksumError::NoHexData {
            name: "flash".to_string(),
            address: 0x08000000,
        };
        assert!(format!("{}", err).contains("flash"));
        assert!(format!("{}", err).contains("08000000"));

        let err = ChecksumError::TypeMismatch {
            expected: "CRC_32".to_string(),
            expected_code: 9,
            actual_code: 7,
        };
        assert!(format!("{}", err).contains("CRC_32"));
    }

    #[test]
    fn test_compute_checksum_all_algorithms() {
        let data = test_pattern();
        let test_cases = vec![
            (XcpChecksum::Add11, 0x10u32),
            (XcpChecksum::Add12, 0x0F10),
            (XcpChecksum::Add14, 0x00000F10),
            (XcpChecksum::Add22, 0x1800),
            (XcpChecksum::Add24, 0x00071800),
            (XcpChecksum::Add44, 0x140C03F8),
            (XcpChecksum::Crc16, 0xC76A),
            (XcpChecksum::Crc16Citt, 0x9D50),
            (XcpChecksum::Crc32, 0x89CD97CE),
        ];

        for (algo, expected) in test_cases {
            let result = compute_checksum(&algo, &data).unwrap();
            assert_eq!(result, expected, "Algorithm {:?} mismatch", algo);
        }
    }
}
