//! R-02: talks to the Scanlight V4 over USB serial, to measure what the desk research only reads.
//! See docs/research/scanlight-capture.md. Every command ends with all channels off, also on Ctrl-C and
//! on errors, and the probe has no way to save a colour to the device (the save flag is always 0).
//!
//! ```text
//! ports                                         which port answers, the firmware and hardware IDs
//! listen  [--port P] [--seconds 3]              sends nothing; shows what the port says by itself
//! colors  [--port P] [--level 64] [--seconds 3] each of red, green, blue alone
//! temp    [--port P] [--level 64] [--cycles 5] [--seconds 10] [--abort-c 60]
//!                                               the three colours in turn, logging LED_TEMP and VBUS
//! shutter [--port P] [--focus] [--lengths 2,5,10,20,30,50]
//!                                               pulses of those lengths (units of 10 ms), 3 s apart
//! settle  --pulse N [--port P] [--colour red] [--level 64] [--delays 0,50,100,200,500]
//!                                               set the colour, wait, fire the shutter: one shot per delay
//! levels  <files>                               the mean colour of each file's embedded JPEG preview
//! ```
//!
//! Every run that talks to the device also writes the packets it sent and received, with times, to
//! `probe-out/scanlight-<command>-<time>.jsonl`.

mod packet;

use std::error::Error;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use packet::{Decoder, Packet};
use serialport::SerialPort;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

static STOP: AtomicBool = AtomicBool::new(false);

fn stopped() -> bool {
    STOP.load(Ordering::SeqCst)
}

/// The baud rate means nothing to a USB CDC device, but the API wants one.
const BAUD: u32 = 115_200;
/// Reads poll with this timeout, so a delay is kept to a few milliseconds.
const READ_TIMEOUT: Duration = Duration::from_millis(20);
/// A write may take this long: a USB serial device that is not ready holds it up.
const WRITE_TIMEOUT: Duration = Duration::from_millis(1000);
/// How many received bytes `ports` and `listen` keep to show raw.
const RAW_KEPT: usize = 256;
/// A level above this lights the LEDs hard: say so.
const BRIGHT: u8 = 128;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, b| {
        if !out.is_empty() {
            out.push(' ');
        }
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// The Scanlight on a serial port. Dropping it switches every channel off.
struct Light {
    port: Box<dyn SerialPort>,
    decoder: Decoder,
    started: Instant,
    log: Vec<String>,
    /// The first bytes received, as they came, whether or not they decoded.
    raw: Vec<u8>,
}

impl Light {
    fn open(name: &str) -> Result<Self> {
        let mut port = serialport::new(name, BAUD)
            .timeout(READ_TIMEOUT)
            .open()
            .map_err(|e| format!("{name} could not be opened: {e}"))?;
        // A USB CDC device often sends and accepts data only while the host holds DTR; without it a
        // write on Windows times out ("The semaphore timeout period has expired").
        let _ = port.write_data_terminal_ready(true);
        let _ = port.write_request_to_send(true);
        std::thread::sleep(Duration::from_millis(100));
        Ok(Self {
            port,
            decoder: Decoder::default(),
            started: Instant::now(),
            log: Vec::new(),
            raw: Vec::new(),
        })
    }

    fn ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    fn record(&mut self, dir: &str, bytes: &[u8]) {
        let line = format!(
            r#"{{"t_ms":{},"dir":"{dir}","hex":"{}"}}"#,
            self.ms(),
            hex(bytes)
        );
        self.log.push(line);
    }

    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.record("tx", bytes);
        self.port.set_timeout(WRITE_TIMEOUT)?;
        // No flush: on Windows it waits for the device to drain its buffer, and a device that does not
        // read would hold the probe there for good.
        let written = self.port.write_all(bytes);
        self.port.set_timeout(READ_TIMEOUT)?;
        Ok(written?)
    }

    /// Reads for `duration` (or until Ctrl-C) and returns the packets that arrived, with the time of each.
    fn pump(&mut self, duration: Duration) -> Result<Vec<(u128, Packet)>> {
        let until = Instant::now() + duration;
        let mut packets = Vec::new();
        let mut buffer = [0u8; 256];
        while Instant::now() < until && !stopped() {
            match self.port.read(&mut buffer) {
                Ok(0) => {}
                Ok(n) => {
                    self.record("rx", &buffer[..n]);
                    if self.raw.len() < RAW_KEPT {
                        self.raw.extend_from_slice(&buffer[..n]);
                    }
                    let at = self.ms();
                    packets.extend(self.decoder.push(&buffer[..n]).into_iter().map(|p| (at, p)));
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(packets)
    }

    fn off(&mut self) -> Result<()> {
        self.send(&packet::all_off())
    }

    fn save(&self, command: &str) -> Result<()> {
        std::fs::create_dir_all("probe-out")?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let path = format!("probe-out/scanlight-{command}-{stamp}.jsonl");
        std::fs::write(&path, self.log.join("\n") + "\n")?;
        println!("wrote {path}");
        Ok(())
    }
}

impl Drop for Light {
    fn drop(&mut self) {
        // Whatever happened, the light does not stay on.
        let _ = self.port.set_timeout(WRITE_TIMEOUT);
        let _ = self.port.write_all(&packet::all_off());
    }
}

fn describe(packet: &Packet) -> String {
    match packet {
        Packet::LedTemp(w) => format!(
            "LED_TEMP raw {} -> {:.1} °C ({} m°C big-endian; read little-endian {})",
            hex(&w.0),
            f64::from(w.value()) / 1000.0,
            w.value(),
            w.le()
        ),
        Packet::Vbus(w) => format!(
            "VBUS raw {} -> {} mV (big-endian; read little-endian {})",
            hex(&w.0),
            w.value(),
            w.le()
        ),
        Packet::FwVersion(w) => format!(
            "FW_VERSION raw {} -> firmware {} hardware {} (big-endian; read little-endian {} and {})",
            hex(&w.0),
            w.halves().0,
            w.halves().1,
            w.halves_le().0,
            w.halves_le().1
        ),
        Packet::Unknown { header, data } => format!("unknown header {header}: {}", hex(data)),
    }
}

/// The value of `--name`, if given.
fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn number<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> Result<T> {
    match option(args, name) {
        None => Ok(default),
        Some(text) => text
            .parse()
            .map_err(|_| format!("{name} wants a number, not {text:?}").into()),
    }
}

fn list<T: std::str::FromStr + Clone>(
    args: &[String],
    name: &str,
    default: &[T],
) -> Result<Vec<T>> {
    match option(args, name) {
        None => Ok(default.to_vec()),
        Some(text) => text
            .split(',')
            .map(|part| {
                part.trim().parse().map_err(|_| {
                    format!("{name} wants numbers separated by commas, not {text:?}").into()
                })
            })
            .collect(),
    }
}

/// Asks `name` for its firmware version; true when a FW_VERSION packet comes back.
fn answers(name: &str, show: bool) -> Result<bool> {
    let mut light = Light::open(name)?;
    light.send(&packet::get_fw_version())?;
    let packets = light.pump(Duration::from_millis(2000))?;
    let mut found = false;
    for (at, p) in &packets {
        if show {
            println!("  {at:>5} ms  {}", describe(p));
        }
        found |= matches!(p, Packet::FwVersion(_));
    }
    if show && !found {
        if light.raw.is_empty() {
            println!("  nothing at all came back in 2 s (not even the temperature the Scanlight sends by itself)");
        } else {
            println!(
                "  {} bytes came back that did not make a firmware version: {}",
                light.raw.len(),
                hex(&light.raw[..light.raw.len().min(48)])
            );
        }
    }
    Ok(found)
}

/// The port to use: the one named, or the first that answers GET_FW_VERSION.
fn find_port(args: &[String]) -> Result<String> {
    if let Some(name) = option(args, "--port") {
        return Ok(name.to_owned());
    }
    for info in serialport::available_ports()? {
        if answers(&info.port_name, false).unwrap_or(false) {
            return Ok(info.port_name);
        }
    }
    Err("no serial port answered: is the Scanlight plugged in and on? (try `ports`)".into())
}

fn ports() -> Result<()> {
    let found = serialport::available_ports()?;
    if found.is_empty() {
        println!("no serial ports");
        return Ok(());
    }
    for info in found {
        println!("{} ({:?})", info.port_name, info.port_type);
        match answers(&info.port_name, true) {
            Ok(true) => println!("  -> this is the Scanlight"),
            Ok(false) => println!("  -> no firmware version came back"),
            Err(e) => println!("  -> {e}"),
        }
    }
    Ok(())
}

/// Sends nothing and shows what the port says by itself for a few seconds: the Scanlight sends its
/// temperature and USB voltage every 200 ms, so a silent port is not the Scanlight (or not ready).
fn listen(args: &[String]) -> Result<()> {
    let seconds: f64 = number(args, "--seconds", 3.0)?;
    let name = match option(args, "--port") {
        Some(name) => name.to_owned(),
        None => serialport::available_ports()?
            .first()
            .map(|p| p.port_name.clone())
            .ok_or("no serial ports")?,
    };
    println!("listening on {name} for {seconds} s, sending nothing");
    let mut light = Light::open(&name)?;
    let packets = light.pump(Duration::from_secs_f64(seconds))?;
    for (at, p) in &packets {
        println!("  {at:>5} ms  {}", describe(p));
    }
    println!(
        "{} packets, {} bytes; first bytes: {}",
        packets.len(),
        light.raw.len(),
        hex(&light.raw[..light.raw.len().min(48)])
    );
    light.save("listen")
}

fn announce_level(level: u8) {
    if level > BRIGHT {
        println!("level {level}: that is bright; the device limits power on a weak supply");
    }
}

fn channel(index: usize, level: u8) -> Vec<u8> {
    let mut rgb = [0u8; 3];
    rgb[index] = level;
    packet::set_color(rgb[0], rgb[1], rgb[2], 0, 0)
}

const NAMES: [&str; 3] = ["red", "green", "blue"];

fn colors(args: &[String]) -> Result<()> {
    let level: u8 = number(args, "--level", 64)?;
    let seconds: f64 = number(args, "--seconds", 3.0)?;
    announce_level(level);
    let mut light = Light::open(&find_port(args)?)?;
    light.send(&packet::get_fw_version())?;
    light.pump(Duration::from_millis(500))?;
    for (index, name) in NAMES.iter().enumerate() {
        if stopped() {
            break;
        }
        println!("{name} on at {level}: look at the light");
        light.send(&channel(index, level))?;
        let packets = light.pump(Duration::from_secs_f64(seconds))?;
        for (at, p) in packets.iter().rev().take(2).rev() {
            println!("  {at:>6} ms  {}", describe(p));
        }
        light.off()?;
        light.pump(Duration::from_millis(1000))?;
    }
    light.off()?;
    light.save("colors")
}

fn temp(args: &[String]) -> Result<()> {
    let level: u8 = number(args, "--level", 64)?;
    let cycles: u32 = number(args, "--cycles", 5)?;
    let seconds: f64 = number(args, "--seconds", 10.0)?;
    let abort_c: f64 = number(args, "--abort-c", 60.0)?;
    announce_level(level);
    let mut light = Light::open(&find_port(args)?)?;
    let mut csv = String::from("t_ms,lit,led_temp_mC,vbus_mV\n");
    let mut temps: Vec<(u128, i32)> = Vec::new();
    let mut vbus = 0;
    'run: for cycle in 1..=cycles {
        for (index, name) in NAMES.iter().enumerate() {
            if stopped() {
                break 'run;
            }
            println!("cycle {cycle}/{cycles}: {name} for {seconds} s");
            light.send(&channel(index, level))?;
            let until = Instant::now() + Duration::from_secs_f64(seconds);
            while Instant::now() < until && !stopped() {
                for (at, p) in light.pump(Duration::from_millis(200))? {
                    match p {
                        Packet::LedTemp(w) => {
                            temps.push((at, w.value()));
                            let _ = writeln!(csv, "{at},{name},{},{vbus}", w.value());
                        }
                        Packet::Vbus(w) => vbus = w.value(),
                        _ => {}
                    }
                }
                if let Some(&(_, t)) = temps.last() {
                    if f64::from(t) / 1000.0 > abort_c {
                        light.off()?;
                        println!(
                            "ABORT: {:.1} °C is above {abort_c} °C; the light is off",
                            f64::from(t) / 1000.0
                        );
                        break 'run;
                    }
                }
            }
        }
    }
    light.off()?;
    if let (Some(&(t0, first)), Some(&(t1, last))) = (temps.first(), temps.last()) {
        let peak = temps.iter().map(|t| t.1).max().unwrap_or(last);
        let minutes = (t1 - t0) as f64 / 60_000.0;
        println!(
            "LED_TEMP: start {:.1} °C, peak {:.1} °C, end {:.1} °C, over {minutes:.1} min ({:.1} °C per minute); VBUS now {vbus} mV",
            f64::from(first) / 1000.0,
            f64::from(peak) / 1000.0,
            f64::from(last) / 1000.0,
            f64::from(last - first) / 1000.0 / minutes.max(0.01)
        );
    } else {
        println!("no LED_TEMP packet came in: this unit may not report it");
    }
    std::fs::create_dir_all("probe-out")?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let path = format!("probe-out/scanlight-temp-{stamp}.csv");
    std::fs::write(&path, csv)?;
    println!("wrote {path}");
    light.save("temp")
}

fn shutter(args: &[String]) -> Result<()> {
    let focus = args.iter().any(|a| a == "--focus");
    let lengths: Vec<u8> = list(args, "--lengths", &[2, 5, 10, 20, 30, 50])?;
    let mut light = Light::open(&find_port(args)?)?;
    println!(
        "{} pulses, 3 s apart, {}. Watch the camera and count the frames it takes.",
        lengths.len(),
        if focus {
            "with the focus signal"
        } else {
            "without the focus signal"
        }
    );
    for (i, units) in lengths.iter().enumerate() {
        if stopped() {
            break;
        }
        if focus {
            light.send(&packet::set_focus(true))?;
            light.pump(Duration::from_millis(300))?;
        }
        println!(
            "pulse {} of {}: {} ms",
            i + 1,
            lengths.len(),
            u32::from(*units) * 10
        );
        light.send(&packet::shutter_pulse(*units))?;
        light.pump(Duration::from_millis(u64::from(*units) * 10 + 200))?;
        if focus {
            light.send(&packet::set_focus(false))?;
        }
        light.pump(Duration::from_secs(3))?;
    }
    light.off()?;
    println!("done: which pulses took a picture?");
    light.save("shutter")
}

fn settle(args: &[String]) -> Result<()> {
    let pulse: u8 = option(args, "--pulse")
        .ok_or(
            "pass --pulse <units of 10 ms>: the length that fired the camera in the shutter test",
        )?
        .parse()?;
    let level: u8 = number(args, "--level", 64)?;
    let colour = option(args, "--colour").unwrap_or("red");
    let index = NAMES
        .iter()
        .position(|n| *n == colour)
        .ok_or("--colour is red, green or blue")?;
    let delays: Vec<u64> = list(args, "--delays", &[0, 50, 100, 200, 500])?;
    announce_level(level);
    let mut light = Light::open(&find_port(args)?)?;
    println!(
        "one shot per delay, {colour} at {level}; keep the room dark and the camera in manual mode"
    );
    for (i, delay) in delays.iter().enumerate() {
        if stopped() {
            break;
        }
        light.off()?;
        light.pump(Duration::from_millis(1500))?;
        println!(
            "shot {} of {}: shutter {delay} ms after the light is switched on",
            i + 1,
            delays.len()
        );
        light.send(&channel(index, level))?;
        light.pump(Duration::from_millis(*delay))?;
        light.send(&packet::shutter_pulse(pulse))?;
        light.pump(Duration::from_millis(u64::from(pulse) * 10 + 2500))?;
    }
    light.off()?;
    println!("done: the shots are in the order of the delays {delays:?}; run `levels` on them");
    light.save("settle")
}

/// The embedded JPEG preview with the most pixels (at least a quarter of a megapixel), as an offset.
fn largest_preview(bytes: &[u8]) -> Option<usize> {
    let mut best: Option<(u64, usize)> = None;
    let mut at = 0;
    while let Some(found) = bytes[at..].windows(3).position(|w| w == [0xFF, 0xD8, 0xFF]) {
        let start = at + found;
        let mut decoder = jpeg_decoder::Decoder::new(&bytes[start..]);
        if decoder.read_info().is_ok() {
            if let Some(info) = decoder.info() {
                let pixels = u64::from(info.width) * u64::from(info.height);
                if pixels >= 250_000 && best.is_none_or(|(most, _)| pixels > most) {
                    best = Some((pixels, start));
                }
            }
        }
        at = start + 1;
    }
    best.map(|(_, start)| start)
}

/// The mean red, green and blue (0 to 255) of the preview of a camera file.
fn preview_means(path: &Path) -> Result<[f64; 3]> {
    let bytes = std::fs::read(path)?;
    let start = largest_preview(&bytes).ok_or("no embedded preview")?;
    let mut decoder = jpeg_decoder::Decoder::new(&bytes[start..]);
    decoder.read_info()?;
    let info = decoder.info().ok_or("no picture info")?;
    decoder.scale(info.width / 8, info.height / 8)?;
    let pixels = decoder.decode()?;
    let (triples, _) = pixels.as_chunks::<3>();
    if triples.is_empty() {
        return Err("an empty preview".into());
    }
    let mut sums = [0.0f64; 3];
    for pixel in triples {
        for (sum, value) in sums.iter_mut().zip(pixel) {
            *sum += f64::from(*value);
        }
    }
    Ok(sums.map(|s| s / triples.len() as f64))
}

fn levels(files: &[String]) -> Result<()> {
    if files.is_empty() {
        return Err("usage: levels <file> [<file> ...], in the order the shots were taken".into());
    }
    println!(
        "{:<4} {:>7} {:>7} {:>7}  file",
        "shot", "red", "green", "blue"
    );
    for (i, file) in files.iter().enumerate() {
        match preview_means(Path::new(file)) {
            Ok(m) => println!(
                "{:<4} {:>7.1} {:>7.1} {:>7.1}  {file}",
                i + 1,
                m[0],
                m[1],
                m[2]
            ),
            Err(e) => println!("{:<4} {e}  {file}", i + 1),
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    // The first Ctrl-C asks the commands to finish (the light goes off); a second one leaves at once, for
    // a device that does not answer.
    ctrlc::set_handler(|| {
        if STOP.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
    })?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or(&[]);
    match args.first().map(String::as_str) {
        Some("ports") => ports(),
        Some("listen") => listen(rest),
        Some("colors") => colors(rest),
        Some("temp") => temp(rest),
        Some("shutter") => shutter(rest),
        Some("settle") => settle(rest),
        Some("levels") => levels(rest),
        _ => Err(
            "usage: ports | listen | colors | temp | shutter | settle | levels (see the top of src/main.rs)"
                .into(),
        ),
    }
}
