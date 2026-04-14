# BUILD_CHECKSUM Feature Design

## 1. Overview

The BUILD_CHECKSUM feature allows the XCP Master to **verify ECU flash memory integrity** by comparing checksums between:
- **Slave side**: ECU computes checksum over a memory region via XCP `BUILD_CHECKSUM` command
- **Master side**: Master computes checksum over the **same memory region** from a local HEX file, using the **same algorithm** specified in the A2L file

If both checksums match, the memory content is verified. This is commonly used after flashing to confirm the ECU holds the expected binary.

## 2. XCP Protocol Foundation

### 2.1 Existing Commands (already implemented)

| Command | Code | Purpose | Status |
|---------|------|---------|--------|
| `SET_MTA` | 0xF6 | Set Memory Transfer Address (base address for checksum) | ✅ |
| `BUILD_CHECKSUM` | 0xF3 | Ask slave to compute checksum over block_size bytes from current MTA | ✅ |
| `UPLOAD` / `SHORT_UPLOAD` | 0xF5 / 0xF4 | Read memory (alternative verification method) | ✅ |

### 2.2 BUILD_CHECKSUM Command Flow

```
Master                              Slave
  │                                   │
  │── SET_MTA(addr=0xA0380000, ext=0)──▶│  Set start address
  │◀── OK ─────────────────────────────│
  │                                    │
  │── BUILD_CHECKSUM(block_size=0x40000)▶│  Request checksum over 256KB
  │◀── OK(type=9, value=0xABCD1234) ───│  Returns: checksum_type + 32-bit result
  │                                    │
```
if max_block_size > max_segment_size, the master should issue multiple BUILD_CHECKSUM commands to cover the entire memory region.

### 2.3 BUILD_CHECKSUM Response

```
Byte 0:    checksum_type (u8)   ← Algorithm used by slave (may differ from request!)
Byte 1-2:  Reserved (u16)      skip two bytes
Bytes 3-6: checksum_value (u32) ← 32-bit checksum result (little-endian)
```

**Important**: The slave's response includes the `checksum_type` it actually used. This may differ from the A2L specification if the slave has its own implementation. The master should verify the returned type matches expectations.

### 2.4 BUILD_CHECKSUM Error responses
```
Byte 0:    error_code (u8)
Byte 1-2: MTA_BLOCK_SIZE_ALIGN (u16)
Byte 3-6: maximum block size (u32)
```

**Errors**: If the slave returns an error, the master should abort the operation. If MTA and block size does not meet alignment requirements, an ERR_OUT_OF_RANGE
with the required MTA_BLOCK_SIZE_ALIGN will be returned. If the block size exceeds
the allowed maximum value, an ERR_OUT_OF_RANGE will be returned. The maximum
block size will be returned in the checksum field.

**IMPORTANT**: If the master uses the wrong max_block_size, the slave will return an error with the correct max_block_size. Then the master should retry the operation with the correct max_block_size.


## 3. A2L Checksum Specification

### 3.1 MEMORY_SEGMENT Structure

Each A2L file defines memory segments in `MOD_PAR`:

```a2l
/begin MEMORY_SEGMENT
  segment_0_type_data           /* name */
  "segment_0_type_data"         /* description */
  DATA                          /* type: DATA / CODE / OFFLINE_DATA */
  FLASH                         /* memory type: FLASH / RAM / EEPROM */
  INTERN                        /* attribute */
  0xA0380000                    /* base_address */
  0x40000                       /* size (bytes) = 256 KB */
  -1 -1 -1 -1 -1               /* (reserved) */
  /begin IF_DATA XCPplus 0x0102
    /begin SEGMENT
      0x0                       /* segment logical number */
      0x2                       /* number of pages */
      0x00                      /* address extension */
      0x00                      /* compression method */
      0x00                      /* encryption method */
      /begin CHECKSUM
        XCP_CRC_32              /* ← checksum algorithm for this segment */
      /end CHECKSUM
      /begin PAGE ... /end PAGE
      /begin PAGE ... /end PAGE
    /end SEGMENT
  /end IF_DATA
/end MEMORY_SEGMENT
```

### 3.2 Checksum Algorithms (from A2L enum)

| A2L Name | Code | Algorithm | Description |
|----------|------|-----------|-------------|
| `XCP_ADD_11` | 1 | ADD byte → byte | Byte-wise addition, result truncated to 8 bits |
| `XCP_ADD_12` | 2 | ADD byte → word | Byte-wise addition, result truncated to 16 bits |
| `XCP_ADD_14` | 3 | ADD byte → dword | Byte-wise addition, result as 32 bits |
| `XCP_ADD_22` | 4 | ADD word → word | 16-bit word-wise addition, result 16 bits |
| `XCP_ADD_24` | 5 | ADD word → dword | 16-bit word-wise addition, result 32 bits |
| `XCP_ADD_44` | 6 | ADD dword → dword | 32-bit dword-wise addition, result 32 bits |
| `XCP_CRC_16` | 7 | CRC-16 | Standard CRC-16 polynomial |
| `XCP_CRC_16_CITT` | 8 | CRC-16 CCITT | CRC-16 CCITT (0x1021 polynomial) |
| `XCP_CRC_32` | 9 | CRC-32 | IEEE 802.3 CRC-32 (0x04C11DB7 polynomial) |
| `XCP_USER_DEFINED` | 255 | Custom | Slave-specific, needs EXTERNAL_FUNCTION |

#### 3.2.1 CRC algorithms details

| Name | Width | Poly | Init | Refin | Refout | XORout|
|------|-------|------|-------|-------|--------|-------|
| XCP_CRC_16 | 16 | 0x8005 | 0x0000 | TRUE | TRUE | 0x0000 |
| XCP_CRC16_CITT | 16 | 0x1021 | 0xFFFF | FALSE | FALSE | 0x0000 |
| XCP_CRC_32 | 32 | 0x04C11DB7 | 0xFFFFFFFF | TRUE | TRUE | 0xFFFFFFFF |

#### Test case
Test pattern:
0x01 0x02 0x03 0x04 0x05 0x06 0x07 0x08 0x09 0x0A 0x0B 0x0C 0x0D 0x0E 0x0F
0x10 0xF1 0xF2 0xF3 0xF4 0xF5 0xF6 0xF7 0xF8 0xF9 0xFA 0xFB 0xFC 0xFD 0xFE
0xFF 0x00

feed the above data to the algorithms in LSB order.
Expected result(in INTEL HEX format):

| Algorithm | Result |
| XCP_ADD_11 | 0x10 |
| XCP_ADD_12 | 0x0F10 |
| XCP_ADD_14 | 0x00000F10 |
| XCP_ADD_22 | 0x1800 |
| XCP_ADD_24 | 0x00071800 |
| XCP_ADD_44 | 0x140C03F8 |
| XCP_CRC_16 | 0xC76A |
| XCP_CRC_16_CITT | 0x9D50|
| XCP_CRC_32 | 0x89CD97CE |

### 3.3 Addition Checksum Details

For `XCP_ADD_XY` where X = input width and Y = output width:
- **Input width (X)**: How many bytes to group per addition step
  - 1 = byte-wise (1 byte at a time)
  - 2 = word-wise (2 bytes at a time, little-endian)
  - 4 = dword-wise (4 bytes at a time, little-endian)
- **Output width (Y)**: How wide the accumulator is
  - 1 = 8-bit result (truncate to u8)
  - 2 = 16-bit result (truncate to u16)
  - 4 = 32-bit result (u32, no truncation needed)

Example: `XCP_ADD_44` = Read 4 bytes (as u32 LE), accumulate into u32 sum, wrapping on overflow.

### 3.4 Byte Order

The byte order for word/dword grouping follows the A2L `BYTE_ORDER` setting:
- `MSB_LAST` (little-endian) — most common in automotive (ARM, TriCore)
- `MSB_FIRST` (big-endian) — some PowerPC-based ECUs

## 4. Intel HEX File Format

### 4.1 Format Overview

Intel HEX (`.hex`) is the standard binary format in automotive ECU development. Each line:

```
:LLAAAATT[DD...]CC
 │ │    │  │     └─ Checksum (two's complement of sum of all bytes)
 │ │    │  └─────── Data bytes (LL bytes)
 │ │    └────────── Record type
 │ └─────────────── Address (16-bit, relative to base)
 └───────────────── Byte count
```

### 4.2 Record Types

| Type | Name | Purpose |
|------|------|---------|
| 0x00 | Data | Actual binary data at address |
| 0x01 | EOF | End of file |
| 0x02 | Extended Segment Address | Sets upper 16 bits of address (segment << 4) |
| 0x03 | Start Segment Address | CS:IP for 8086 (rarely used) |
| 0x04 | Extended Linear Address | Sets upper 16 bits of 32-bit address |
| 0x05 | Start Linear Address | Entry point (EIP) |

### 4.3 Address Calculation

For a data record with address field `AAAA`:
- If preceded by **Type 04** record with value `BBBB`: physical address = `(BBBB << 16) + AAAA`
- If preceded by **Type 02** record with value `SSSS`: physical address = `(SSSS << 4) + AAAA`
- Otherwise: physical address = `AAAA`

## 5. Verification Workflow

### 5.1 High-Level Flow

```
                    ┌──────────────┐
                    │  A2L File    │
                    │ (segments +  │
                    │  checksums)  │
                    └──────┬───────┘
                           │ parse
                           ▼
              ┌────────────────────────┐
              │  Memory Segment List   │
              │  [{addr, size, algo}]  │
              └────────────┬───────────┘
                           │
          ┌────────────────┼────────────────┐
          │                │                │
          ▼                ▼                ▼
   ┌──────────┐    ┌──────────────┐  ┌──────────────┐
   │ HEX File │    │  Segment 0   │  │  Segment 1   │
   │ (binary  │    │ 0xA0380000   │  │ 0xA03C0000   │
   │  image)  │    │ size=0x40000 │  │ size=0x100   │
   └────┬─────┘    │ algo=CRC_32  │  │ algo=CRC_32  │
        │          └──────┬───────┘  └──────┬───────┘
        │ extract         │                 │
        ▼                 ▼                 ▼
   ┌──────────┐    ┌──────────────┐  ┌──────────────┐
   │ Master   │    │ Slave        │  │ Slave        │
   │ compute  │    │ BUILD_CHKSUM │  │ BUILD_CHKSUM │
   │ checksum │    │ → result     │  │ → result     │
   └────┬─────┘    └──────┬───────┘  └──────┬───────┘
        │                 │                 │
        └────────┬────────┘                 │
                 ▼                          │
          ┌─────────────┐           ┌──────────────┐
          │ Compare     │           │ Compare      │
          │ match/fail  │           │ match/fail   │
          └─────────────┘           └──────────────┘
```

### 5.2 Step-by-Step

1. **Parse A2L** → Extract `MEMORY_SEGMENT` list:
   - `name`, `type` (DATA/CODE/OFFLINE_DATA), `base_address`, `size`
   - From `IF_DATA XCP` → `SEGMENT` → `CHECKSUM` → algorithm type
   - `MAX_BLOCK_SIZE` (maximum block the slave can checksum at once)

2. **Parse HEX file** → Build flat memory image:
   - `HashMap<u32, u8>` (sparse) or `Vec<u8>` per address range
   - Handle extended address records (Type 02/04)
   - Validate per-line checksums

3. **For each memory segment** from A2L:
   a. **Extract data** from HEX image for `[base_address, base_address + size)`
   b. **Compute master checksum** locally using the A2L-specified algorithm
   c. **Send to slave**: `SET_MTA(base_address, extension)` + `BUILD_CHECKSUM(size)`
      - If `size > MAX_BLOCK_SIZE`: split into multiple BUILD_CHECKSUM calls and combine results
   d. **Verify**: slave's returned `checksum_type` matches expected algorithm
   e. **Compare**: master's computed value == slave's returned value
   f. **Multipe Pages**: if segment have more than one page, use set_cal_page to switch page and build checksum again, the master still uses the hex data to compute checksum for different pages

4. **Report results**:
   - Per-segment: PASS / FAIL / SKIPPED (no HEX data for region)
   - Summary: total segments, in each page(if any), passed, failed
   - Failed segments: list of segments with FAIL
   - Failed pages: list of pages with FAIL
   - Also, checksum value for each segment in hex string format

### 5.3 MAX_BLOCK_SIZE Handling

Some slaves have a maximum block size for BUILD_CHECKSUM (specified in A2L as `MAX_BLOCK_SIZE` within the CHECKSUM block). If the segment size exceeds this:

**For addition checksums (ADD_xx)**:
- Split into chunks ≤ MAX_BLOCK_SIZE
- Sum all partial results (same accumulation as the algorithm)
- This works because addition is associative

**For CRC checksums (CRC_16/CRC_32)**:
- Split into chunks ≤ MAX_BLOCK_SIZE
- Calculate and verify each chunk's checksum
- If any chunk fails, fail the whole segment
- Combine the results into one single checksum

## 6. Current Codebase Status

### 6.1 Already Implemented

| Component | Location | Notes |
|-----------|----------|-------|
| `build_checksum()` command builder | `src/protocol/commands.rs:273` | Sends 0xF3 + block_size |
| `parse_checksum()` response parser | `src/protocol/responses.rs:207` | Returns (type, value) |
| `XcpChecksum` enum + A2L parsing | `src/a2l/reader.rs:145-180` | All 9 types + UserDefined |
| `SET_MTA` + memory read/write | `src/slave.rs:921-1224` | Full upload/download support |
| MEMORY_SEGMENT parsing | `src/a2l/reader.rs` | Segment info extraction |

### 6.2 Not Yet Implemented

| Component | Description |
|-----------|-------------|
| **HEX file parser** | Parse Intel HEX format into memory image |
| **Checksum calculators** | ADD_11/12/14/22/24/44, CRC_16, CRC_16_CITT, CRC_32 |
| **Verification engine** | Orchestrate: parse A2L segments → compute → compare |
| **`build_checksum()` on XcpSlave** | High-level method wrapping SET_MTA + BUILD_CHECKSUM |
| **Web UI / CLI integration** | Upload HEX, trigger verify, display results |

### 6.3 Suggested Dependencies

```toml
# Intel HEX file parsing
ihex = "3.0"           # or manually parse (simple format)

# CRC computation
crc = "3.2"            # CRC-16, CRC-32 with configurable polynomials
```

## 7. Example: Real A2L Segments

### From `abc.a2l` (TriCore ECU):

| Segment | Type | Base Address | Size | Checksum |
|---------|------|-------------|------|----------|
| segment_0_type_data | DATA/FLASH | 0xA0380000 | 0x40000 (256 KB) | XCP_CRC_32 |
| segment_1_type_offline_data | OFFLINE_DATA/FLASH | 0xA03C0000 | 0x100 (256 B) | XCP_CRC_32 |
| segment_2_type_code | CODE/FLASH | 0x80044000 | 0x20 (32 B) | XCP_CRC_32 |

### From `hello_xcp.a2l` (Simulator):

| Segment | Type | Base Address | Size | Checksum |
|---------|------|-------------|------|----------|
| epk | DATA/FLASH | 0x80000000 | 28 B | XCP_ADD_44 |
| my_params | DATA/FLASH | 0x80010000 | 48 B | XCP_ADD_44 |

## 8. Checksum Algorithm Pseudo-Code

### XCP_ADD_14 (byte → dword accumulation)
```
sum: u32 = 0
for each byte b in data:
    sum = sum.wrapping_add(b as u32)
return sum
```

### XCP_ADD_44 (dword → dword accumulation)
```
sum: u32 = 0
for i in (0..data.len()).step_by(4):
    word = u32::from_le_bytes(data[i..i+4])   // or from_be_bytes per byte_order
    sum = sum.wrapping_add(word)
return sum
```

### XCP_CRC_32 (IEEE 802.3)
```
Standard CRC-32 with polynomial 0x04C11DB7
Initial value: 0xFFFFFFFF
Final XOR: 0xFFFFFFFF
Input/output reflection: yes
```
