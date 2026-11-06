//! The Scanlight V4 over USB CDC serial: [`ScanlightV4`] implements [`LightSource`].
//! `protocol` has the packets, `port` the serial port and the probe.
//!
//! Proven on macOS only. On the author's Windows PC the port opens and never replies, so
//! `connect` fails there with a clear error (ADR 0004, amendment 10).

pub mod port;
pub mod protocol;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use gtp_core::{AppError, ErrorCode};

use crate::light::{LightInfo, LightSource, LightStatus};
use port::{probe, Firmware, LightPort, SerialLightPort, PROBE_TIMEOUT};
use protocol::{all_off, set_color, shutter_pulse, Decoder, Packet};

/// The RP2040 vendor ID of the V4's Pico firmware. The control interface defines no VID/PID, so
/// this only decides which port is tried first.
const LIGHT_VENDOR_ID: u16 = 0x2e8a;

/// A connected light: the writing port, the reader thread that keeps the status current.
struct Link {
    port: Box<dyn LightPort>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

/// The Scanlight V4. Dropping it switches the light off.
pub struct ScanlightV4 {
    link: Option<Link>,
    status: Arc<Mutex<LightStatus>>,
    probe_timeout: Duration,
}

impl Default for ScanlightV4 {
    fn default() -> Self {
        Self::new()
    }
}

impl ScanlightV4 {
    /// A driver that is not connected yet.
    pub fn new() -> Self {
        Self {
            link: None,
            status: Arc::new(Mutex::new(LightStatus::default())),
            probe_timeout: PROBE_TIMEOUT,
        }
    }

    /// Changes how long each port gets to answer the probe.
    #[must_use]
    pub fn with_probe_timeout(mut self, timeout: Duration) -> Self {
        self.probe_timeout = timeout;
        self
    }

    /// Probes `port`, and when the light answers, keeps it and starts the reader. A hardware ID
    /// other than 1 or 2 is accepted with a warning in the log and `recognised` false.
    pub fn attach(
        &mut self,
        name: &str,
        mut port: Box<dyn LightPort>,
    ) -> Result<LightInfo, AppError> {
        let found = probe(port.as_mut(), self.probe_timeout)?;
        self.start(name, port, found)
    }

    fn start(
        &mut self,
        name: &str,
        port: Box<dyn LightPort>,
        found: Firmware,
    ) -> Result<LightInfo, AppError> {
        if !found.recognised() {
            tracing::warn!(
                hardware_id = found.hardware_id,
                firmware_id = found.firmware_id,
                "the light reports a hardware ID this app does not know (expected 1 or 2); using it anyway"
            );
        }
        self.disconnect();
        let reader_port = port.try_clone().map_err(|err| {
            AppError::new(
                ErrorCode::Io,
                "The light's port could not be shared for reading.",
            )
            .with_details(err.to_string())
        })?;
        set_status(&self.status, |status| {
            *status = LightStatus {
                connected: true,
                ..LightStatus::default()
            }
        });
        let stop = Arc::new(AtomicBool::new(false));
        let reader = {
            let (stop, status) = (Arc::clone(&stop), Arc::clone(&self.status));
            std::thread::Builder::new()
                .name("scanlight-reader".into())
                .spawn(move || read_loop(reader_port, &stop, &status))
                .map_err(|err| {
                    AppError::new(ErrorCode::Internal, "The light's reader could not start.")
                        .with_details(err.to_string())
                })?
        };
        self.link = Some(Link {
            port,
            stop,
            reader: Some(reader),
        });
        Ok(LightInfo {
            port: name.to_owned(),
            hardware_id: found.hardware_id,
            firmware_id: found.firmware_id,
            recognised: found.recognised(),
        })
    }

    /// Switches the light off (best effort) and stops the reader.
    fn disconnect(&mut self) {
        let Some(mut link) = self.link.take() else {
            return;
        };
        // Whatever happened, the light does not stay on. A failure cannot be handled here.
        if let Err(err) = link.port.write_all(&all_off()) {
            tracing::debug!("switching the light off on close failed: {err}");
        }
        link.stop.store(true, Ordering::SeqCst);
        if let Some(reader) = link.reader.take() {
            // The reader polls with the port timeout, so this returns promptly.
            let _ = reader.join();
        }
        set_status(&self.status, |status| status.connected = false);
    }

    fn send(&mut self, packet: &[u8]) -> Result<(), AppError> {
        let Some(link) = self.link.as_mut() else {
            return Err(AppError::new(ErrorCode::Io, "The light is not connected."));
        };
        link.port.write_all(packet).map_err(|err| {
            set_status(&self.status, |status| status.connected = false);
            AppError::new(
                ErrorCode::Io,
                "The light did not accept a command. Unplug it and plug it in again.",
            )
            .with_details(err.to_string())
        })
    }
}

impl Drop for ScanlightV4 {
    fn drop(&mut self) {
        self.disconnect();
    }
}

impl LightSource for ScanlightV4 {
    fn connect(&mut self, port: Option<&str>) -> Result<LightInfo, AppError> {
        let candidates = match port {
            Some(name) => vec![name.to_owned()],
            None => candidate_ports()?,
        };
        let mut silent = Vec::new();
        for name in &candidates {
            let opened =
                SerialLightPort::open(name).and_then(|opened| self.attach(name, Box::new(opened)));
            match opened {
                Ok(info) => return Ok(info),
                // With a port named by the caller, say exactly what went wrong.
                Err(err) if port.is_some() => return Err(err),
                Err(err) => silent.push(format!("{name}: {}", err.details.unwrap_or(err.message))),
            }
        }
        Err(AppError::new(
            ErrorCode::NotFound,
            "No Scanlight answered on any serial port. Check that it is plugged in and that no other program (such as its web app) is using it.",
        )
        .with_details(silent.join("; ")))
    }

    fn set_levels(&mut self, rgb: [u8; 3]) -> Result<(), AppError> {
        self.send(&set_color(rgb))
    }

    fn shutter_pulse(&mut self, tens_of_ms: u8) -> Result<(), AppError> {
        self.send(&shutter_pulse(tens_of_ms))
    }

    fn status(&self) -> LightStatus {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn set_status(status: &Mutex<LightStatus>, change: impl FnOnce(&mut LightStatus)) {
    change(&mut status.lock().unwrap_or_else(PoisonError::into_inner));
}

/// Reads until told to stop, keeping the status current. A read error ends it with `connected`
/// false.
fn read_loop(mut port: Box<dyn LightPort>, stop: &AtomicBool, status: &Mutex<LightStatus>) {
    let mut decoder = Decoder::default();
    let mut buffer = [0u8; 256];
    while !stop.load(Ordering::SeqCst) {
        let count = match port.read(&mut buffer) {
            Ok(count) => count,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => 0,
            Err(err) => {
                tracing::warn!("the light stopped sending: {err}");
                set_status(status, |status| status.connected = false);
                return;
            }
        };
        for packet in decoder.push(buffer.get(..count).unwrap_or_default()) {
            set_status(status, |status| match packet {
                Packet::LedTemp { milli_c } => status.temp_c = Some(milli_c as f32 / 1000.0),
                Packet::Vbus { mv } => status.vbus_mv = Some(mv),
                Packet::FwVersion { .. } => {}
            });
        }
    }
}

/// The USB serial ports, the Pico's vendor first. macOS lists each port twice (`cu.` and
/// `tty.`); only the `cu.` one is tried.
fn candidate_ports() -> Result<Vec<String>, AppError> {
    let ports = serialport::available_ports().map_err(|err| {
        AppError::new(ErrorCode::Io, "The serial ports could not be listed.")
            .with_details(err.to_string())
    })?;
    let mut usb: Vec<(bool, String)> = ports
        .into_iter()
        .filter_map(|info| match info.port_type {
            serialport::SerialPortType::UsbPort(usb) => {
                Some((usb.vid == LIGHT_VENDOR_ID, info.port_name))
            }
            _ => None,
        })
        .filter(|(_, name)| !name.starts_with("/dev/tty."))
        .collect();
    usb.sort_by_key(|(is_light, _)| !is_light);
    Ok(usb.into_iter().map(|(_, name)| name).collect())
}

#[cfg(test)]
mod tests;
