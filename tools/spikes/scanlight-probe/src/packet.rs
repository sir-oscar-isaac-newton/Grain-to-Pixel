//! The Scanlight V4's serial packets, from its control interface document (firmware 1.2):
//! `0xFE`, a header byte, a length byte, then that many data bytes; a length of 0 ends the packet.
//! There is no checksum and no acknowledgement. The document does not give the byte order of the
//! 16 and 32 bit fields, so a decoded word keeps its four bytes and offers both readings: the probe
//! prints both, and the recorded packets settle which is right.

pub const START: u8 = 0xFE;

pub const H2D_SET_COLOR: u8 = 0;
pub const H2D_GET_FW_VERSION: u8 = 2;
pub const H2D_SHUTTER_PULSE: u8 = 3;
pub const H2D_SET_FOCUS: u8 = 7;

pub const D2H_LED_TEMP: u8 = 1;
pub const D2H_VBUS: u8 = 2;
pub const D2H_FW_VERSION: u8 = 3;

/// The data length of each device packet this knows.
const WORD_LEN: usize = 4;
/// An unknown packet longer than this is taken for garbage that looked like a start byte.
const MAX_UNKNOWN_LEN: usize = 16;

/// SET_COLOR with the save flag **always 0**: the levels are never written to the device's memory,
/// which has a limited number of write cycles. There is no way to ask this for a save.
pub fn set_color(red: u8, green: u8, blue: u8, white: u8, infrared: u8) -> Vec<u8> {
    vec![
        START,
        H2D_SET_COLOR,
        6,
        red,
        green,
        blue,
        white,
        infrared,
        0,
    ]
}

/// All channels off.
pub fn all_off() -> Vec<u8> {
    set_color(0, 0, 0, 0, 0)
}

pub fn get_fw_version() -> Vec<u8> {
    vec![START, H2D_GET_FW_VERSION, 0]
}

/// SHUTTER_PULSE; the length is in units of 10 ms.
pub fn shutter_pulse(units: u8) -> Vec<u8> {
    vec![START, H2D_SHUTTER_PULSE, 1, units]
}

/// SET_FOCUS: the focus signal on (1) or off (0). Only PCB rev 26a902b has it.
pub fn set_focus(on: bool) -> Vec<u8> {
    vec![START, H2D_SET_FOCUS, 1, u8::from(on)]
}

/// Four data bytes of a device packet, readable either way round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Word(pub [u8; 4]);

impl Word {
    pub fn le(self) -> i32 {
        i32::from_le_bytes(self.0)
    }

    pub fn be(self) -> i32 {
        i32::from_be_bytes(self.0)
    }

    /// The two 16 bit halves, little-endian each, in order.
    pub fn halves_le(self) -> (u16, u16) {
        (
            u16::from_le_bytes([self.0[0], self.0[1]]),
            u16::from_le_bytes([self.0[2], self.0[3]]),
        )
    }

    /// The two 16 bit halves, big-endian each, in order.
    pub fn halves_be(self) -> (u16, u16) {
        (
            u16::from_be_bytes([self.0[0], self.0[1]]),
            u16::from_be_bytes([self.0[2], self.0[3]]),
        )
    }
}

/// A packet from the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    /// Millidegrees Celsius, every 200 ms.
    LedTemp(Word),
    /// Millivolts, every 200 ms.
    Vbus(Word),
    /// Firmware version ID and hardware version ID (16 bits each).
    FwVersion(Word),
    /// A header this does not know, with its data.
    Unknown { header: u8, data: Vec<u8> },
}

/// Turns the bytes read from the port into packets. Bytes before a start byte are dropped, a packet
/// split over several reads is put together, and anything that does not parse is skipped one byte at
/// a time: it never panics and never stops on garbage.
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
        loop {
            // Drop what comes before the next start byte.
            match self.buffer[at..].iter().position(|&b| b == START) {
                Some(skip) => at += skip,
                None => {
                    at = self.buffer.len();
                    break;
                }
            }
            let rest = &self.buffer[at..];
            if rest.len() < 3 {
                break; // the header and length are not all here yet
            }
            let (header, length) = (rest[1], usize::from(rest[2]));
            let known = matches!(header, D2H_LED_TEMP | D2H_VBUS | D2H_FW_VERSION);
            // A known packet with another length, or a long unknown one, was not a packet.
            if (known && length != WORD_LEN) || (!known && length > MAX_UNKNOWN_LEN) {
                at += 1;
                continue;
            }
            if rest.len() < 3 + length {
                break; // the data is not all here yet
            }
            let data = &rest[3..3 + length];
            packets.push(match header {
                D2H_LED_TEMP => Packet::LedTemp(word(data)),
                D2H_VBUS => Packet::Vbus(word(data)),
                D2H_FW_VERSION => Packet::FwVersion(word(data)),
                _ => Packet::Unknown {
                    header,
                    data: data.to_vec(),
                },
            });
            at += 3 + length;
        }
        self.buffer.drain(..at);
        packets
    }
}

/// The first four bytes of `data` (the caller has checked there are four).
fn word(data: &[u8]) -> Word {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&data[..4]);
    Word(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_packets_are_byte_exact_and_set_color_never_saves() {
        assert_eq!(
            set_color(10, 20, 30, 0, 0),
            [0xFE, 0, 6, 10, 20, 30, 0, 0, 0],
            "the last byte is the save flag"
        );
        assert_eq!(all_off(), [0xFE, 0, 6, 0, 0, 0, 0, 0, 0]);
        assert_eq!(get_fw_version(), [0xFE, 2, 0]);
        assert_eq!(shutter_pulse(10), [0xFE, 3, 1, 10]);
        assert_eq!(set_focus(true), [0xFE, 7, 1, 1]);
        assert_eq!(set_focus(false), [0xFE, 7, 1, 0]);
        // Whatever the levels, the save flag stays 0.
        for level in [0, 1, 127, 255] {
            assert_eq!(set_color(level, level, level, level, level)[8], 0);
        }
    }

    #[test]
    fn device_packets_decode_and_a_word_reads_both_ways() {
        let mut decoder = Decoder::default();
        let packets = decoder.push(&[
            0xFE, 1, 4, 0xA8, 0x61, 0x00, 0x00, // LED_TEMP 25000 little-endian
            0xFE, 2, 4, 0x88, 0x13, 0x00, 0x00, // VBUS 5000
            0xFE, 3, 4, 0x02, 0x01, 0x0B, 0x00, // firmware 0x0102, hardware 0x000B
        ]);
        assert_eq!(packets.len(), 3);
        let Packet::LedTemp(temp) = packets[0] else {
            panic!("{:?}", packets[0])
        };
        assert_eq!(temp.le(), 25_000);
        assert_ne!(temp.be(), 25_000);
        let Packet::Vbus(vbus) = packets[1] else {
            panic!("{:?}", packets[1])
        };
        assert_eq!(vbus.le(), 5_000);
        let Packet::FwVersion(fw) = packets[2] else {
            panic!("{:?}", packets[2])
        };
        assert_eq!(fw.halves_le(), (0x0102, 0x000B));
        assert_eq!(fw.halves_be(), (0x0201, 0x0B00));
    }

    #[test]
    fn a_packet_split_over_reads_is_put_together() {
        let whole = [0xFE, 1, 4, 1, 2, 3, 4];
        for cut in 1..whole.len() {
            let mut decoder = Decoder::default();
            assert!(decoder.push(&whole[..cut]).is_empty(), "cut at {cut}");
            assert_eq!(decoder.push(&whole[cut..]).len(), 1, "cut at {cut}");
        }
    }

    #[test]
    fn garbage_before_and_between_packets_is_dropped() {
        let mut decoder = Decoder::default();
        let packets = decoder.push(&[
            0x00, 0x13, 0x37, // noise
            0xFE, 2, 4, 1, 0, 0, 0, // VBUS
            0xFE, 1, 9, // a start byte of noise: LED_TEMP with a wrong length
            0xFE, 1, 4, 2, 0, 0, 0, // LED_TEMP
        ]);
        assert_eq!(packets.len(), 2);
        assert!(matches!(packets[0], Packet::Vbus(_)));
        assert!(matches!(packets[1], Packet::LedTemp(_)));
    }

    #[test]
    fn an_unknown_header_is_kept_unless_it_is_too_long_to_be_real() {
        let mut decoder = Decoder::default();
        assert_eq!(
            decoder.push(&[0xFE, 9, 2, 7, 8]),
            [Packet::Unknown {
                header: 9,
                data: vec![7, 8]
            }]
        );
        // A zero-length packet is the last byte it has: three bytes in all.
        assert_eq!(
            decoder.push(&[0xFE, 9, 0]),
            [Packet::Unknown {
                header: 9,
                data: vec![]
            }]
        );
        assert!(decoder.push(&[0xFE, 9, 200]).is_empty());
    }

    #[test]
    fn truncated_input_waits_and_does_not_lose_the_next_packet() {
        let mut decoder = Decoder::default();
        assert!(decoder.push(&[0xFE]).is_empty());
        assert!(decoder.push(&[1]).is_empty());
        assert!(decoder.push(&[4, 1]).is_empty());
        let packets = decoder.push(&[2, 3, 4, 0xFE, 2, 4, 9, 9, 9, 9]);
        assert_eq!(packets.len(), 2);
    }

    #[test]
    fn random_bytes_never_panic_and_never_leave_an_unbounded_buffer() {
        let mut decoder = Decoder::default();
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut count = 0;
        for _ in 0..2_000 {
            let chunk: Vec<u8> = (0..37)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    // A lot of start bytes, so the decoder has something to chew on.
                    if state.is_multiple_of(5) {
                        START
                    } else {
                        (state >> 24) as u8
                    }
                })
                .collect();
            count += decoder.push(&chunk).len();
        }
        assert!(count > 0);
        assert!(decoder.buffer.len() <= 3 + 255, "{}", decoder.buffer.len());
    }
}
