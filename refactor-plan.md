# XCP packet layering refactor plan

## Problem

The intended boundary is stricter than the previous refactor captured: the XCP-on-UDP transport header must exist only inside `src/transport/udp.rs`.

That means more than making protocol parsers payload-only. It also means:

- generic XCP message types must not expose UDP packet counters;
- `MessageRouter` must not parse UDP datagrams or know about UDP frame aggregation;
- the `Transport` abstraction must not require non-UDP transports to fake UDP-only concepts.

Any transport metadata such as the XCP-on-UDP packet counter is an implementation detail of `udp.rs`, not a property of a general XCP message.

## Findings from repository audit

1. `src/transport/message_router.rs`
   - `MessageRouter` still imports `parse_xcp_datagram()` from `udp.rs`, so the router itself knows that UDP datagrams may contain multiple framed XCP packets.
   - `XcpMessage` still contains `counter: u16`, which makes a UDP-only field part of the shared routed message model.
   - `CounterTracker` also lives in the router, so incoming UDP counter sequencing is tracked above the transport boundary.

2. `src/transport/transport.rs`
   - `Transport::recv_packet()` and `try_recv_packet()` still expose raw transport receive units, which for UDP means raw datagrams that the router must split.
   - `Transport::current_counter()` is a UDP-specific concept forced into the cross-transport trait; `can.rs` currently has to return a dummy `0`.

3. `src/transport/udp.rs`
   - `udp.rs` already owns frame encode/decode helpers, but that ownership is incomplete because parsing is re-exported upward through router usage instead of being fully encapsulated.
   - Incoming datagram splitting and counter tracking should terminate here and yield normalized XCP payload packets to the rest of the system.

4. `src/protocol/responses.rs`
   - This file is now correctly payload-only.
   - That earlier improvement should remain intact while the transport boundary is tightened further.

5. `src/slave.rs` and `src/daq.rs`
   - These callers now mostly consume normalized payloads, which is the right direction.
   - They still depend on `XcpMessage`, so any remaining UDP-specific fields on that shared type continue to leak transport concerns upward.

6. `src/transport/can.rs`
   - The placeholder CAN transport reveals the abstraction problem clearly: it must currently implement `current_counter()` even though CAN has no equivalent XCP-on-UDP transport header counter.

## Proposed approach

1. Make `udp.rs` the sole owner of XCP-on-UDP framing
   - Keep header encode/decode, packet counter sequencing, datagram splitting, and any counter continuity diagnostics inside `src/transport/udp.rs`.
   - Do not expose UDP frame-parsing helpers to the router.

2. Redefine the transport receive contract around normalized XCP payload packets
   - `Transport` should hand upper layers decoded XCP payload packets one at a time, regardless of how the underlying transport aggregates or frames them.
   - For UDP this likely requires an internal receive queue so one datagram containing multiple XCP frames can still be surfaced as multiple normalized packets.

3. Make routed messages transport-agnostic
   - Remove `counter` from `XcpMessage`.
   - Keep only generic fields needed by upper layers, such as packet classification, PID, and payload bytes.

4. Remove UDP-only concepts from the generic transport abstraction
   - Remove `current_counter()` from `Transport` unless a proven transport-agnostic use case appears.
   - Avoid forcing placeholder or future transports to fabricate UDP semantics.

5. Keep upper layers payload-only
   - Preserve the existing payload-only behavior in `protocol::responses`, `slave.rs`, and `daq.rs`.
   - Re-check for any remaining transport-offset assumptions or transport-specific logging paths after the message shape changes.

## Todos

1. Update the plan implementation target: `udp.rs` must own all XCP-on-UDP header parsing, datagram splitting, and counter handling.
2. Refactor `Transport` receive APIs so upper layers consume normalized XCP payload packets instead of raw UDP datagrams.
3. Remove `counter` from `XcpMessage` and simplify `MessageRouter` to pure CTO/DTO classification on payload bytes.
4. Move or eliminate router-level counter tracking so UDP sequence handling stays inside `udp.rs`.
5. Remove `current_counter()` from the generic `Transport` trait and clean up affected transport implementations.
6. Re-verify `slave.rs`, `daq.rs`, and protocol parsers against the new transport-agnostic routed message shape.
7. Update tests first for the revised boundaries, then implement the refactor and run the existing Rust test suite.

## Revision status

- Previous work correctly moved protocol response parsing to payload-only handling.
- Previous work did **not** fully complete the transport-boundary refactor, because UDP framing knowledge still exists in `message_router.rs` and the shared `Transport`/`XcpMessage` abstractions.
- This revised plan supersedes the earlier "implemented" status and focuses on removing those remaining boundary violations.

## Latest implementation status

- Implemented:
  - `src/transport/udp.rs` is now the only layer that parses XCP-on-UDP headers, splits UDP datagrams, and tracks incoming UDP counters.
  - `src/transport/message_router.rs` now routes one normalized payload packet at a time and no longer owns UDP parsing or counter tracking.
  - `XcpMessage` now carries only generic XCP message data (`packet_type`, `pid`, `payload`) and no longer exposes a UDP counter.
  - `Transport::current_counter()` was removed from the generic transport trait, so non-UDP transports no longer need to fake UDP semantics.

- Validation:
  - `cargo build` passes.
  - `cargo test` still has the same pre-existing 13 failures caused by missing `abc.a2l` / `xcp_dft.a2l` fixtures; no new failures were introduced by this refactor.

## Notes / review points

- The key correction is architectural: a packet counter is not generic XCP message metadata, it is UDP transport metadata.
- A clean end state is: `udp.rs` receives datagrams and emits normalized XCP payload packets; `message_router.rs` only classifies payloads; upper layers never see UDP header fields.
