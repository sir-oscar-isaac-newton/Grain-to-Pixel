//! The serial side: a small port trait (so tests need no hardware), the real implementation over
//! the `serialport` crate, and the probe that asks a port `GET_FW_VERSION`.
//!
//! Every read and write has a timeout, and nothing flushes: on Windows a flush waits for the
//! device to drain its buffer, and in the R-02 probe a device that did not read held it there
//! until the cable was pulled.
//!
//! The driver is proven on macOS only. On the author's Windows PC the port opens and never
//! replies; the probe then fails with a clear error after its timeout. That is not diagnosed or
//! worked around here (ADR 0004, amendment 10).

use std::io;
use std::time::{Duration, Instant};

use gtp_core::{AppError, ErrorCode};

use super::protocol::{get_fw_version, Decoder, Packet};

/// The baud rate means nothing to a USB CDC device, but the API wants one.
const BAUD: u32 = 115_200;
/// One timeout for reads and writes. Reads poll with it, so the reader thread notices a stop
/// request within this time, and a write to a device that does not read fails after it. It is a
/// single value because on Windows a cloned handle shares the timeouts of the port.
pub const IO_TIMEOUT: Duration = Duration::from_millis(100);
/// How long a port gets to answer `GET_FW_VERSION` (the light answers in about 4 ms).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// `GET_FW_VERSION` is sent again this often while waiting, in case the first was lost.
const PROBE_RESEND: Duration = Duration::from_millis(500);
/// A USB CDC device often answers only while the host holds DTR.
const DTR_SETTLE: Duration = Duration::from_millis(100);

/// Hardware IDs this app knows: 1 is the V4 rev A (PCB 26a901a), 2 the rev B.
pub const KNOWN_HARDWARE_IDS: [u16; 2] = [1, 2];

/// A byte pipe to the light with timeouts built in. Never blocks for longer than [`IO_TIMEOUT`].
pub trait LightPort: Send {
    /// Writes all the bytes, or fails (a timeout is an error).
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// Reads what has arrived. `Ok(0)` means nothing came within the timeout.
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    /// A second handle to the same port, for the reader thread.
    fn try_clone(&self) -> io::Result<Box<dyn LightPort>>;
}

/// A real serial port.
pub struct SerialLightPort {
    port: Box<dyn serialport::SerialPort>,
}

impl SerialLightPort {
    /// Opens the port and raises DTR and RTS.
    pub fn open(name: &str) -> Result<Self, AppError> {
        let mut port = serialport::new(name, BAUD)
            .timeout(IO_TIMEOUT)
            .open()
            .map_err(|err| {
                AppError::new(
                    ErrorCode::Io,
                    "The light's serial port could not be opened. Is another program using it?",
                )
                .with_details(format!("{name}: {err}"))
            })?;
        // Failing to raise them is not fatal: some drivers do not support it.
        let _ = port.write_data_terminal_ready(true);
        let _ = port.write_request_to_send(true);
        std::thread::sleep(DTR_SETTLE);
        Ok(Self { port })
    }
}

impl LightPort for SerialLightPort {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        io::Write::write_all(&mut self.port, bytes)
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match io::Read::read(&mut self.port, buf) {
            Err(err) if err.kind() == io::ErrorKind::TimedOut => Ok(0),
            other => other,
        }
    }

    fn try_clone(&self) -> io::Result<Box<dyn LightPort>> {
        let port = self.port.try_clone().map_err(io::Error::other)?;
        Ok(Box::new(Self { port }))
    }
}

/// The IDs a light reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Firmware {
    pub hardware_id: u16,
    pub firmware_id: u16,
}

impl Firmware {
    /// True when the hardware ID is one this app knows.
    pub fn recognised(self) -> bool {
        KNOWN_HARDWARE_IDS.contains(&self.hardware_id)
    }
}

/// Sends `GET_FW_VERSION` (again every half second) and waits up to `timeout` for the answer.
/// Other packets that arrive meanwhile are ignored. Always returns within `timeout` plus one
/// port timeout, so a silent port (the author's Windows PC) is an error, not a hang.
pub fn probe(port: &mut dyn LightPort, timeout: Duration) -> Result<Firmware, AppError> {
    let started = Instant::now();
    let mut last_sent: Option<Instant> = None;
    let mut decoder = Decoder::default();
    let mut buffer = [0u8; 256];
    loop {
        if last_sent.is_none_or(|at| at.elapsed() >= PROBE_RESEND) {
            port.write_all(&get_fw_version()).map_err(|err| {
                AppError::new(
                    ErrorCode::Io,
                    "The light did not accept a command. Unplug it and plug it in again.",
                )
                .with_details(format!("writing GET_FW_VERSION: {err}"))
            })?;
            last_sent = Some(Instant::now());
        }
        let count = match port.read(&mut buffer) {
            Ok(count) => count,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => 0,
            Err(err) => {
                return Err(AppError::new(
                    ErrorCode::Io,
                    "The connection to the light was lost. Unplug it and plug it in again.",
                )
                .with_details(format!("reading the version: {err}")))
            }
        };
        for packet in decoder.push(buffer.get(..count).unwrap_or_default()) {
            if let Packet::FwVersion {
                hardware_id,
                firmware_id,
            } = packet
            {
                return Ok(Firmware {
                    hardware_id,
                    firmware_id,
                });
            }
        }
        if started.elapsed() >= timeout {
            return Err(AppError::new(
                ErrorCode::Io,
                "The port opened but the light did not answer. Check the cable and the USB port, and that no other program is using the light.",
            )
            .with_details(format!("no FW_VERSION within {} ms", timeout.as_millis())));
        }
    }
}
