//! Driver and probe tests against a fake port: no hardware.

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gtp_core::ErrorCode;

use super::port::{probe, LightPort, PROBE_TIMEOUT};
use super::protocol::START;
use super::ScanlightV4;
use crate::light::LightSource;

#[derive(Default)]
struct Shared {
    written: Vec<u8>,
    /// Chunks the next reads return, one per read.
    replies: VecDeque<Vec<u8>>,
    /// Pushed onto `replies` each time `GET_FW_VERSION` is written. Empty: a silent port.
    answer_to_version: Vec<u8>,
    fail_writes: bool,
}

/// A port that records what is written and plays back scripted replies.
#[derive(Clone, Default)]
struct FakePort(Arc<Mutex<Shared>>);

impl FakePort {
    fn answering(hardware_id: u16, firmware_id: u16) -> Self {
        let port = Self::default();
        let [h0, h1] = hardware_id.to_be_bytes();
        let [f0, f1] = firmware_id.to_be_bytes();
        port.0.lock().unwrap().answer_to_version = vec![START, 3, 4, h0, h1, f0, f1];
        port
    }

    fn silent() -> Self {
        Self::default()
    }

    fn queue(&self, bytes: &[u8]) {
        self.0.lock().unwrap().replies.push_back(bytes.to_vec());
    }

    fn written(&self) -> Vec<u8> {
        self.0.lock().unwrap().written.clone()
    }
}

impl LightPort for FakePort {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        let mut shared = self.0.lock().unwrap();
        if shared.fail_writes {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "write timed out"));
        }
        shared.written.extend_from_slice(bytes);
        if bytes == [START, 2, 0] && !shared.answer_to_version.is_empty() {
            let answer = shared.answer_to_version.clone();
            shared.replies.push_back(answer);
        }
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let reply = self.0.lock().unwrap().replies.pop_front();
        match reply {
            Some(bytes) => {
                buf[..bytes.len()].copy_from_slice(&bytes);
                Ok(bytes.len())
            }
            None => {
                // What a real port does: wait out its timeout and report nothing.
                std::thread::sleep(Duration::from_millis(2));
                Ok(0)
            }
        }
    }

    fn try_clone(&self) -> io::Result<Box<dyn LightPort>> {
        Ok(Box::new(self.clone()))
    }
}

/// Collects the messages of `warn` events, for the thread that installs it.
mod capture {
    use std::sync::{Arc, Mutex};
    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Level, Metadata, Subscriber};

    #[derive(Clone, Default)]
    pub struct Warnings(pub Arc<Mutex<Vec<String>>>);

    struct Message(String);

    impl Visit for Message {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.0 = format!("{value:?}");
            }
        }
    }

    impl Subscriber for Warnings {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, event: &Event<'_>) {
            if *event.metadata().level() == Level::WARN {
                let mut message = Message(String::new());
                event.record(&mut message);
                self.0.lock().unwrap().push(message.0);
            }
        }
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn driver() -> ScanlightV4 {
    ScanlightV4::new().with_probe_timeout(Duration::from_millis(300))
}

#[test]
fn probe_accepts_hardware_ids_1_and_2_without_a_warning() {
    for id in [1, 2] {
        let warnings = capture::Warnings::default();
        let info = tracing::subscriber::with_default(warnings.clone(), || {
            driver().attach("fake", Box::new(FakePort::answering(id, 0)))
        })
        .unwrap();
        assert_eq!((info.hardware_id, info.firmware_id), (id, 0));
        assert!(info.recognised);
        assert!(warnings.0.lock().unwrap().is_empty(), "id {id}");
    }
}

#[test]
fn probe_warns_about_another_id_and_still_connects() {
    for id in [0, 3, 0xFFFF] {
        let warnings = capture::Warnings::default();
        let info = tracing::subscriber::with_default(warnings.clone(), || {
            driver().attach("fake", Box::new(FakePort::answering(id, 7)))
        })
        .unwrap();
        assert_eq!((info.hardware_id, info.firmware_id), (id, 7));
        assert!(!info.recognised, "id {id}");
        let warnings = warnings.0.lock().unwrap();
        assert_eq!(warnings.len(), 1, "id {id}: {warnings:?}");
        assert!(warnings[0].contains("does not know"), "{warnings:?}");
    }
}

#[test]
fn probe_reads_the_ids_in_the_order_hardware_then_firmware() {
    // RECORDED data from the real light: 00 01 00 00 is hardware ID 1, firmware ID 0.
    let mut port = FakePort::default();
    port.queue(&[START, 3, 4, 0x00, 0x01, 0x00, 0x00]);
    let found = probe(&mut port, Duration::from_millis(300)).unwrap();
    assert_eq!((found.hardware_id, found.firmware_id), (1, 0));
    assert_eq!(port.written(), [START, 2, 0], "GET_FW_VERSION was sent");
}

#[test]
fn probe_ignores_streamed_packets_and_noise_before_the_answer() {
    let mut port = FakePort::default();
    port.queue(&[
        0x00, START, 1, 4, 0, 0, 0x74, 0x04, START, 2, 4, 0, 0, 0x14, 0x27,
    ]);
    port.queue(&[START, 3, 4]);
    port.queue(&[0, 2, 0, 9]);
    let found = probe(&mut port, Duration::from_millis(300)).unwrap();
    assert_eq!((found.hardware_id, found.firmware_id), (2, 9));
}

#[test]
fn a_silent_port_times_out_with_a_clear_error_and_does_not_hang() {
    // The author's Windows PC: the port opens and never replies.
    let started = Instant::now();
    let mut port = FakePort::silent();
    let err = probe(&mut port, Duration::from_millis(250)).unwrap_err();
    assert_eq!(err.code, ErrorCode::Io);
    assert!(err.message.contains("did not answer"), "{}", err.message);
    assert!(started.elapsed() < Duration::from_secs(2));
    let sent = port.written();
    assert!(!sent.is_empty() && sent.chunks(3).all(|c| c == [START, 2, 0]));

    let mut light = driver();
    let err = light
        .attach("silent", Box::new(FakePort::silent()))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Io);
    assert_eq!(light.status(), crate::LightStatus::default());
    assert!(
        PROBE_TIMEOUT <= Duration::from_secs(5),
        "the default is bounded too"
    );
}

#[test]
fn a_port_that_cannot_be_written_fails_fast() {
    let mut port = FakePort::default();
    port.0.lock().unwrap().fail_writes = true;
    let err = probe(&mut port, Duration::from_secs(5)).unwrap_err();
    assert_eq!(err.code, ErrorCode::Io);
}

#[test]
fn commands_are_sent_as_exact_packets_and_the_save_flag_is_always_zero() {
    let port = FakePort::answering(1, 0);
    let mut light = driver();
    light.attach("fake", Box::new(port.clone())).unwrap();
    light.set_levels([44, 22, 2]).unwrap();
    light.shutter_pulse(10).unwrap();
    light.set_levels([255, 255, 255]).unwrap();
    let written = port.written();
    let mut expected = vec![START, 2, 0];
    expected.extend([START, 0, 6, 44, 22, 2, 0, 0, 0]);
    expected.extend([START, 3, 1, 10]);
    expected.extend([START, 0, 6, 255, 255, 255, 0, 0, 0]);
    assert_eq!(written, expected);
}

#[test]
fn commands_before_connect_fail_and_dropping_switches_the_light_off() {
    let mut light = driver();
    assert_eq!(light.set_levels([1, 2, 3]).unwrap_err().code, ErrorCode::Io);

    let port = FakePort::answering(1, 0);
    let mut light = driver();
    light.attach("fake", Box::new(port.clone())).unwrap();
    light.set_levels([0, 255, 0]).unwrap();
    drop(light);
    let written = port.written();
    assert_eq!(
        written[written.len() - 9..],
        [START, 0, 6, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn the_reader_keeps_the_status_current() {
    let port = FakePort::answering(1, 0);
    let mut light = driver();
    light.attach("fake", Box::new(port.clone())).unwrap();
    assert!(light.status().connected);
    assert_eq!(light.status().temp_c, None);
    // HAND-DERIVED: 25000 m°C; RECORDED: VBUS 5159 mV.
    port.queue(&[START, 1, 4, 0, 0, 0x61, 0xA8, START, 2, 4, 0, 0, 0x14, 0x27]);
    wait_for("the status", || light.status().vbus_mv.is_some());
    assert_eq!(light.status().temp_c, Some(25.0));
    assert_eq!(light.status().vbus_mv, Some(5159));
}

#[test]
fn a_failed_write_marks_the_light_disconnected() {
    let port = FakePort::answering(1, 0);
    let mut light = driver();
    light.attach("fake", Box::new(port.clone())).unwrap();
    port.0.lock().unwrap().fail_writes = true;
    let err = light.set_levels([1, 0, 0]).unwrap_err();
    assert_eq!(err.code, ErrorCode::Io);
    assert!(!light.status().connected);
}
