//! The Scanlight V4 packets: `0xFE`, a header byte, the data length, then the data. No checksum,
//! no acknowledgement. The 16 and 32 bit fields are big-endian, and the version reply is the
//! hardware ID first, then the firmware ID (ADR 0004, amendment 3; both measured on a Mac, and
//! both the other way round from the control interface document). Source of the vectors in the
//! tests: docs/research/scanlight-capture.md.
//!
//! There is deliberately no `SET_TRIM`, no way to ask `SET_COLOR` for a save, and no `SET_FOCUS`
//! (the author's light is a rev A and does not have it).

/// First byte of every packet.
pub const START: u8 = 0xFE;

const H2D_SET_COLOR: u8 = 0;
const H2D_GET_FW_VERSION: u8 = 2;
const H2D_SHUTTER_PULSE: u8 = 3;

const D2H_LED_TEMP: u8 = 1;
const D2H_VBUS: u8 = 2;
const D2H_FW_VERSION: u8 = 3;

/// Data length of every device packet this decoder knows (a 32 bit word, or two 16 bit halves).
const WORD_LEN: usize = 4;
/// An unknown packet longer than this is taken for noise that looked like a start byte.
const MAX_UNKNOWN_LEN: usize = 16;
/// Bytes in a packet before its data: start, header, length.
const PREFIX_LEN: usize = 3;

/// A level as a byte: anything below 0 is 0 and anything above 255 is 255.
pub fn clamp_level(level: i32) -> u8 {
    u8::try_from(level.clamp(0, 255)).unwrap_or(u8::MAX)
}

/// `SET_COLOR` for red, green and blue; white and IR are 0 and **the save flag is always 0**
/// (saving writes the memory of the light, which wears out).
pub fn set_color(rgb: [u8; 3]) -> Vec<u8> {
    vec![START, H2D_SET_COLOR, 6, rgb[0], rgb[1], rgb[2], 0, 0, 0]
}

/// All channels off (a `SET_COLOR` of zeros, never saved).
pub fn all_off() -> Vec<u8> {
    set_color([0, 0, 0])
}

/// `GET_FW_VERSION`, no data.
pub fn get_fw_version() -> Vec<u8> {
    vec![START, H2D_GET_FW_VERSION, 0]
}

/// `SHUTTER_PULSE`, the length in units of 10 ms.
pub fn shutter_pulse(tens_of_ms: u8) -> Vec<u8> {
    vec![START, H2D_SHUTTER_PULSE, 1, tens_of_ms]
}

/// A packet from the device. Packets with a header this does not know are dropped by the decoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packet {
    /// Millidegrees Celsius, every 200 ms.
    LedTemp { milli_c: i32 },
    /// Millivolts, every 200 ms.
    Vbus { mv: i32 },
    /// Answer to `GET_FW_VERSION`.
    FwVersion { hardware_id: u16, firmware_id: u16 },
}

/// Turns the bytes read from the port into packets. Bytes before a start byte are dropped, a
/// packet split over several reads is put together, unknown headers are dropped whole, and a
/// packet with a wrong length is skipped one byte at a time to find the next real one. It never
/// panics and holds at most one unfinished packet.
#[derive(Debug, Default)]
pub struct Decoder {
    buffer: Vec<u8>,
}

impl Decoder {
    /// Adds bytes and returns the packets completed by them.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Packet> {
        self.buffer.extend_from_slice(bytes);
        let mut packets = Vec::new();
        let mut at = 0;
        while let Some(rest) = self.buffer.get(at..) {
            let Some(skip) = rest.iter().position(|&b| b == START) else {
                at = self.buffer.len();
                break;
            };
            at += skip;
            let rest = self.buffer.get(at..).unwrap_or_default();
            let (Some(&header), Some(&length)) = (rest.get(1), rest.get(2)) else {
                break; // the header and length are not all here yet
            };
            let length = usize::from(length);
            let known = matches!(header, D2H_LED_TEMP | D2H_VBUS | D2H_FW_VERSION);
            // A known packet with another length, or a long unknown one, was not a packet.
            if (known && length != WORD_LEN) || (!known && length > MAX_UNKNOWN_LEN) {
                at += 1;
                continue;
            }
            let Some(data) = rest.get(PREFIX_LEN..PREFIX_LEN + length) else {
                break; // the data is not all here yet
            };
            if let Some(packet) = parse(header, data) {
                packets.push(packet);
            } // else: an unknown header, dropped
            at += PREFIX_LEN + length;
        }
        self.buffer.drain(..at.min(self.buffer.len()));
        packets
    }
}

fn parse(header: u8, data: &[u8]) -> Option<Packet> {
    let word: [u8; WORD_LEN] = data.try_into().ok()?;
    match header {
        D2H_LED_TEMP => Some(Packet::LedTemp {
            milli_c: i32::from_be_bytes(word),
        }),
        D2H_VBUS => Some(Packet::Vbus {
            mv: i32::from_be_bytes(word),
        }),
        D2H_FW_VERSION => Some(Packet::FwVersion {
            hardware_id: u16::from_be_bytes([word[0], word[1]]),
            firmware_id: u16::from_be_bytes([word[2], word[3]]),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // Provenance of the vectors (docs/research/scanlight-capture.md, R-02):
    // - VBUS `fe 02 04 00 00 14 27` = 5159 mV: RECORDED from the real light on a Mac (the note
    //   quotes it verbatim).
    // - FW_VERSION: the note records the data `00 01 00 00` (hardware ID 1, firmware ID 0) from
    //   the real light; the `fe 03 04` framing around it is from the document, so the full
    //   packet is HAND-ASSEMBLED around recorded data.
    // - LED_TEMP: the note records "29.7 C" but not the bytes; the vectors here are HAND-DERIVED
    //   (millidegrees, big-endian).
    // - Host packets: HAND-DERIVED from the packet table in the note (the probe sent exactly
    //   these on the Mac and the colours lit and the light answered `GET_FW_VERSION`, but no
    //   transmitted bytes were captured). SHUTTER_PULSE fired no camera, so it is unconfirmed.

    #[test]
    fn host_packets_are_byte_exact_and_the_save_flag_is_zero() {
        // HAND-DERIVED: SET_COLOR = FE, 0, length 6, R, G, B, W, IR, save.
        assert_eq!(set_color([10, 20, 30]), [0xFE, 0, 6, 10, 20, 30, 0, 0, 0]);
        assert_eq!(all_off(), [0xFE, 0, 6, 0, 0, 0, 0, 0, 0]);
        // HAND-DERIVED: GET_FW_VERSION = FE, 2, length 0.
        assert_eq!(get_fw_version(), [0xFE, 2, 0]);
        // HAND-DERIVED: SHUTTER_PULSE = FE, 3, length 1, units of 10 ms.
        assert_eq!(shutter_pulse(10), [0xFE, 3, 1, 10]);
    }

    #[test]
    fn set_color_never_saves_and_never_lights_white_or_ir() {
        for level in 0..=255u8 {
            let packet = set_color([level, level, level]);
            assert_eq!(packet.len(), 9);
            assert_eq!(packet[6..], [0, 0, 0], "white, IR and the save flag");
        }
    }

    #[test]
    fn levels_are_clamped_to_a_byte() {
        assert_eq!(clamp_level(-1), 0);
        assert_eq!(clamp_level(i32::MIN), 0);
        assert_eq!(clamp_level(0), 0);
        assert_eq!(clamp_level(128), 128);
        assert_eq!(clamp_level(255), 255);
        assert_eq!(clamp_level(256), 255);
        assert_eq!(clamp_level(i32::MAX), 255);
    }

    #[test]
    fn device_packets_decode_big_endian() {
        let mut decoder = Decoder::default();
        // RECORDED: VBUS 5159 mV.
        assert_eq!(
            decoder.push(&[0xFE, 0x02, 0x04, 0x00, 0x00, 0x14, 0x27]),
            [Packet::Vbus { mv: 5159 }]
        );
        // Recorded data, hand-assembled framing: hardware ID 1, firmware ID 0.
        assert_eq!(
            decoder.push(&[0xFE, 0x03, 0x04, 0x00, 0x01, 0x00, 0x00]),
            [Packet::FwVersion {
                hardware_id: 1,
                firmware_id: 0
            }]
        );
        // HAND-DERIVED: 25000 m°C = 0x000061A8.
        assert_eq!(
            decoder.push(&[0xFE, 0x01, 0x04, 0x00, 0x00, 0x61, 0xA8]),
            [Packet::LedTemp { milli_c: 25_000 }]
        );
        // HAND-DERIVED: a negative reading, -5 °C = -5000 = 0xFFFFEC78.
        assert_eq!(
            decoder.push(&[0xFE, 0x01, 0x04, 0xFF, 0xFF, 0xEC, 0x78]),
            [Packet::LedTemp { milli_c: -5_000 }]
        );
    }

    #[test]
    fn a_packet_split_anywhere_is_put_together() {
        let whole = [0xFE, 2, 4, 0x00, 0x00, 0x14, 0x27];
        for cut in 1..whole.len() {
            let mut decoder = Decoder::default();
            assert!(decoder.push(&whole[..cut]).is_empty(), "cut at {cut}");
            assert_eq!(
                decoder.push(&whole[cut..]),
                [Packet::Vbus { mv: 5159 }],
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn unknown_headers_are_dropped_and_the_next_packet_survives() {
        let mut decoder = Decoder::default();
        let packets = decoder.push(&[
            0xFE, 9, 2, 7, 8, // unknown header 9 with two bytes
            0xFE, 9, 0, // unknown, no data
            0xFE, 2, 4, 0, 0, 0, 1, // VBUS 1
        ]);
        assert_eq!(packets, [Packet::Vbus { mv: 1 }]);
    }

    #[test]
    fn bad_lengths_are_skipped_without_losing_the_next_packet() {
        let mut decoder = Decoder::default();
        let packets = decoder.push(&[
            0xFE, 1, 9, // LED_TEMP with the wrong length
            0xFE, 2, 3, // VBUS too short
            0xFE, 9, 200, // an unknown header with a long length
            0xFE, 1, 4, 0, 0, 0, 2, // LED_TEMP 2
        ]);
        assert_eq!(packets, [Packet::LedTemp { milli_c: 2 }]);
    }

    #[test]
    fn noise_before_and_between_packets_is_dropped() {
        let mut decoder = Decoder::default();
        let packets = decoder.push(&[
            0x00, 0x13, 0x37, 0xFE, 2, 4, 0, 0, 0, 1, 0xAA, 0xBB, 0xFE, 1, 4, 0, 0, 0, 2,
        ]);
        assert_eq!(
            packets,
            [Packet::Vbus { mv: 1 }, Packet::LedTemp { milli_c: 2 }]
        );
    }

    #[test]
    fn truncated_input_waits_and_a_lone_start_byte_does_not_stick() {
        let mut decoder = Decoder::default();
        assert!(decoder.push(&[]).is_empty());
        assert!(decoder.push(&[0xFE]).is_empty());
        assert!(decoder.push(&[1]).is_empty());
        assert!(decoder.push(&[4, 0]).is_empty());
        assert_eq!(
            decoder.push(&[0, 0, 3, 0xFE, 2, 4, 0, 0, 0, 9]),
            [Packet::LedTemp { milli_c: 3 }, Packet::Vbus { mv: 9 }]
        );
    }

    #[test]
    fn oversized_input_is_handled_and_keeps_the_buffer_small() {
        let mut decoder = Decoder::default();
        let big = vec![START; 1_000_000];
        assert!(decoder.push(&big).is_empty());
        assert!(decoder.buffer.len() <= PREFIX_LEN + MAX_UNKNOWN_LEN);
        let zeros = vec![0u8; 1_000_000];
        assert!(decoder.push(&zeros).is_empty());
        assert!(decoder.buffer.is_empty());
    }

    proptest! {
        #[test]
        fn garbage_never_panics_and_the_buffer_stays_bounded(
            chunks in proptest::collection::vec(
                proptest::collection::vec(
                    prop_oneof![Just(START), Just(1u8), Just(2u8), Just(3u8), Just(4u8), any::<u8>()],
                    0..300,
                ),
                0..20,
            )
        ) {
            let mut decoder = Decoder::default();
            for chunk in &chunks {
                let _ = decoder.push(chunk);
                prop_assert!(decoder.buffer.len() <= PREFIX_LEN + MAX_UNKNOWN_LEN);
            }
        }

        #[test]
        fn a_real_packet_after_any_garbage_is_found(
            garbage in proptest::collection::vec(any::<u8>(), 0..200)
        ) {
            // Garbage may end in a start byte of an unknown header that swallows up to 19 bytes
            // of what follows; six copies are more than that, so one always gets through.
            let mut decoder = Decoder::default();
            let mut input = garbage;
            for _ in 0..6 {
                input.extend_from_slice(&[START, 2, 4, 0, 0, 0, 7]);
            }
            let packets = decoder.push(&input);
            let wanted = Packet::Vbus { mv: 7 };
            prop_assert!(packets.contains(&wanted));
        }

        #[test]
        fn splitting_the_stream_does_not_change_the_packets(
            input in proptest::collection::vec(any::<u8>(), 0..200),
            cut in 0usize..200,
        ) {
            let cut = cut.min(input.len());
            let whole = Decoder::default().push(&input);
            let mut split = Decoder::default();
            let mut parts = split.push(&input[..cut]);
            parts.extend(split.push(&input[cut..]));
            prop_assert_eq!(whole, parts);
        }
    }
}
