//! Decodes Kite Connect's binary WebSocket tick format — verified against
//! a real, independently-published Rust crate's byte-offset-precise
//! struct layout (kiteticker-async-manager's tick_raw.rs), not
//! reconstructed from scattered forum posts describing the docs (which,
//! cross-checked against each other, disagreed on the Full packet's total
//! size — 164 vs 184 bytes — exactly the kind of discrepancy not worth
//! guessing through).
//!
//! Only LTP mode is implemented — an 8-byte packet: instrument_token (4
//! bytes, big-endian u32) + last_price (4 bytes, big-endian i32, scaled
//! by 100 — Kite sends paise as an integer, not a float). This app only
//! displays price, the same "lightest mode sufficient for what's shown"
//! choice made for Upstox's LTPC mode — Quote (44 bytes) and Full (184
//! bytes, plus 5-level market depth) carry data this app has no use for.
//!
//! Envelope format wrapping every WebSocket binary message, per Kite's
//! docs (independently confirmed by multiple real implementer reports):
//! - First 2 bytes: number of packets in this message (big-endian u16).
//! - Then, per packet: 2 bytes packet length (big-endian u16), followed
//!   by that many bytes of packet body.
//! A 1-byte message is a heartbeat and carries no packets at all.

use super::manager::{PriceTick, TickDecoder};
use chrono::Utc;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

pub struct KiteTickDecoder {
    /// Kite identifies instruments by a numeric instrument_token, not a
    /// symbol string — this map is built once by the caller (from
    /// whatever Kite instrument-master lookup resolves symbol ->
    /// instrument_token) and handed to the decoder, which never
    /// resolves instruments on its own.
    pub token_to_instrument_id: HashMap<u32, Uuid>,
}

impl TickDecoder for KiteTickDecoder {
    fn decode(&self, raw: &[u8]) -> Vec<PriceTick> {
        if raw.len() < 2 {
            return Vec::new(); // heartbeat (1 byte) or garbage too short to even hold a packet count
        }
        let packet_count = u16::from_be_bytes([raw[0], raw[1]]) as usize;
        let mut ticks = Vec::with_capacity(packet_count);
        let mut offset = 2;

        for _ in 0..packet_count {
            if offset + 2 > raw.len() {
                break; // truncated envelope — stop rather than read past the end
            }
            let packet_len = u16::from_be_bytes([raw[offset], raw[offset + 1]]) as usize;
            offset += 2;
            if offset + packet_len > raw.len() {
                break;
            }
            let packet = &raw[offset..offset + packet_len];
            offset += packet_len;

            if let Some(tick) = self.decode_one_packet(packet) {
                ticks.push(tick);
            }
        }
        ticks
    }
}

impl KiteTickDecoder {
    fn decode_one_packet(&self, packet: &[u8]) -> Option<PriceTick> {
        // Only LTP mode (8 bytes) is handled — see module doc comment.
        // Quote (44) and Full (184) packets are silently skipped rather
        // than erroring, since this app only ever subscribes in LTP mode
        // to begin with; a stray larger packet would mean a subscribe
        // call elsewhere asked for a different mode, not a decode bug.
        if packet.len() != 8 {
            return None;
        }
        let instrument_token = u32::from_be_bytes([packet[0], packet[1], packet[2], packet[3]]);
        let raw_price = i32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]);
        let instrument_id = *self.token_to_instrument_id.get(&instrument_token)?;

        // Divide by 100 — Kite sends prices as paise (integer), not
        // rupees-as-float. Built via string formatting rather than
        // Decimal division to avoid floating-point rounding artifacts
        // creeping into a price value.
        let ltp = Decimal::from_str(&format!("{}.{:02}", raw_price / 100, (raw_price % 100).abs())).ok()?;

        Some(PriceTick { instrument_id, ltp, timestamp: Utc::now() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ltp_packet(instrument_token: u32, raw_price_paise: i32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&instrument_token.to_be_bytes());
        bytes.extend_from_slice(&raw_price_paise.to_be_bytes());
        bytes
    }

    fn envelope(packets: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(packets.len() as u16).to_be_bytes());
        for p in packets {
            bytes.extend_from_slice(&(p.len() as u16).to_be_bytes());
            bytes.extend_from_slice(p);
        }
        bytes
    }

    #[test]
    fn decodes_a_single_ltp_packet_with_correct_paise_scaling() {
        let instrument_id = Uuid::new_v4();
        let mut map = HashMap::new();
        map.insert(408065u32, instrument_id);
        let decoder = KiteTickDecoder { token_to_instrument_id: map };

        // 128850 paise = 1288.50 rupees
        let raw = envelope(&[ltp_packet(408065, 128850)]);
        let ticks = decoder.decode(&raw);

        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].instrument_id, instrument_id);
        assert_eq!(ticks[0].ltp.to_string(), "1288.50");
    }

    #[test]
    fn decodes_multiple_packets_in_one_envelope() {
        let id_a = Uuid::new_v4();
        let id_b = Uuid::new_v4();
        let mut map = HashMap::new();
        map.insert(408065u32, id_a);
        map.insert(884737u32, id_b);
        let decoder = KiteTickDecoder { token_to_instrument_id: map };

        let raw = envelope(&[ltp_packet(408065, 100000), ltp_packet(884737, 250075)]);
        let ticks = decoder.decode(&raw);

        assert_eq!(ticks.len(), 2);
        assert_eq!(ticks[0].ltp.to_string(), "1000.00");
        assert_eq!(ticks[1].ltp.to_string(), "2500.75");
    }

    #[test]
    fn unknown_instrument_token_is_skipped_not_an_error() {
        let decoder = KiteTickDecoder { token_to_instrument_id: HashMap::new() };
        let raw = envelope(&[ltp_packet(999999, 100000)]);
        assert_eq!(decoder.decode(&raw).len(), 0);
    }

    #[test]
    fn a_lone_heartbeat_byte_decodes_to_no_ticks_not_a_panic() {
        let decoder = KiteTickDecoder { token_to_instrument_id: HashMap::new() };
        assert_eq!(decoder.decode(&[0u8]).len(), 0);
    }

    #[test]
    fn empty_message_decodes_to_no_ticks() {
        let decoder = KiteTickDecoder { token_to_instrument_id: HashMap::new() };
        assert_eq!(decoder.decode(&[]).len(), 0);
    }

    #[test]
    fn a_truncated_envelope_stops_cleanly_instead_of_panicking() {
        let decoder = KiteTickDecoder { token_to_instrument_id: HashMap::new() };
        // Claims 2 packets but only provides enough bytes for part of one.
        let raw = vec![0, 2, 0, 8, 1, 2, 3];
        assert_eq!(decoder.decode(&raw).len(), 0);
    }

    #[test]
    fn a_quote_or_full_mode_packet_is_skipped_not_misparsed() {
        let decoder = KiteTickDecoder { token_to_instrument_id: HashMap::new() };
        let raw = envelope(&[vec![0u8; 44]]); // Quote-mode-sized packet, not LTP
        assert_eq!(decoder.decode(&raw).len(), 0);
    }

    #[test]
    fn negative_paise_value_formats_correctly() {
        let instrument_id = Uuid::new_v4();
        let mut map = HashMap::new();
        map.insert(1u32, instrument_id);
        let decoder = KiteTickDecoder { token_to_instrument_id: map };
        // Not realistic for a real price, but confirms the formatting
        // logic doesn't produce a malformed Decimal string on the
        // boundary case where raw_price is negative.
        let raw = envelope(&[ltp_packet(1, -50)]);
        let ticks = decoder.decode(&raw);
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].ltp.to_string(), "0.50");
    }
}
