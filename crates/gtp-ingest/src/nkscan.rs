//! The Nikon Coolscan LS-50 through the `nkscan` command line (H-02, ADR 0003).
//!
//! The spike (R-01) found the CLI stable and the crate's API in flux, so the adapter runs the CLI,
//! pinned to [`NKSCAN_VERSION`], and reads what it writes: `<basename>_<n>.tiff` (16-bit RGB) and
//! `<basename>_<n>_IR.tiff` (16-bit infrared). nkscan numbers its files from 1 whatever frames were
//! asked for, so file `n` is the `n`-th frame of the scan. It does not make the output folder, and
//! it ejects the film at the end unless told not to; the adapter makes a folder of its own for each
//! scan and never ejects (see [`NkscanSource::eject`]).

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gtp_core::{AppError, ErrorCode};
use gtp_io::{read_tiff16, split_rgbi, Limits};

use crate::scanner::{CancelToken, DeviceInfo, RawScan, ScanMeta, ScanParams, ScannerSource};

/// The nkscan release the adapter was written and tested against.
pub const NKSCAN_VERSION: &str = "0.17.0";
/// The environment variable that names the nkscan program to run.
pub const NKSCAN_ENV: &str = "GTP_NKSCAN";
/// The prefix of the files a scan writes.
const BASENAME: &str = "frame";
/// How often a running scan is looked at.
const POLL: Duration = Duration::from_millis(100);
/// How many lines of nkscan's messages are kept to explain a failure.
const STDERR_LINES: usize = 20;

/// Scans with the `nkscan` command line.
#[derive(Debug, Clone)]
pub struct NkscanSource {
    program: PathBuf,
    work_dir: PathBuf,
    env: Vec<(OsString, OsString)>,
}

impl NkscanSource {
    /// Runs `program` (a path, or a name looked up on `PATH`); scans are written under the system's
    /// temporary folder until they are read.
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            work_dir: std::env::temp_dir(),
            env: Vec::new(),
        }
    }

    /// The program named by `GTP_NKSCAN`, or `nkscan` from `PATH`.
    pub fn from_env() -> Self {
        Self::new(std::env::var_os(NKSCAN_ENV).unwrap_or_else(|| "nkscan".into()))
    }

    /// Keeps the scans of a roll under `dir` instead of the temporary folder (a frame is 190 MB).
    pub fn with_work_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.work_dir = dir.into();
        self
    }

    /// Sets an environment variable for every run of the program (the tests steer a stand-in
    /// program with it).
    pub fn with_env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Ejects the film. A scan never does, so that a roll can be scanned in parts.
    pub fn eject(&self) -> Result<(), AppError> {
        self.check_version()?;
        let output = self.run(&["eject"])?;
        if output.status.success() {
            Ok(())
        } else {
            Err(failure(&String::from_utf8_lossy(&output.stderr)))
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.envs(self.env.iter().map(|(k, v)| (k, v)));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window flashing up from the app.
            command.creation_flags(0x0800_0000);
        }
        command
    }

    fn run(&self, args: &[&str]) -> Result<std::process::Output, AppError> {
        self.command()
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| not_started(&self.program, &err))
    }

    /// `ScannerUnavailable` unless the program is the pinned release.
    fn check_version(&self) -> Result<(), AppError> {
        let output = self.run(&["--version"])?;
        let text = String::from_utf8_lossy(&output.stdout);
        let version = text.split_whitespace().nth(1).unwrap_or_default();
        if output.status.success() && version == NKSCAN_VERSION {
            Ok(())
        } else {
            Err(AppError::new(
                ErrorCode::ScannerUnavailable,
                format!("The scanner program is not nkscan {NKSCAN_VERSION}."),
            )
            .with_details(format!("{} said {:?}", self.program.display(), text.trim())))
        }
    }
}

fn not_started(program: &Path, err: &std::io::Error) -> AppError {
    AppError::new(
        ErrorCode::ScannerUnavailable,
        "The scanner program (nkscan) could not be started.",
    )
    .with_details(format!("{}: {err}", program.display()))
}

/// The error for a nkscan run that failed, from what it said on stderr: a scanner that cannot be
/// reached or used is `ScannerUnavailable`, anything else `ScanFailed`.
fn failure(stderr: &str) -> AppError {
    let lower = stderr.to_lowercase();
    let details = stderr.trim().to_owned();
    if lower.contains("incompatible driver") {
        return AppError::new(
            ErrorCode::ScannerUnavailable,
            "The scanner has the wrong driver. On Windows nkscan needs the WinUSB driver (see the scanner setup notes).",
        )
        .with_details(details);
    }
    let unreachable = [
        "no scanner",
        "no devices",
        "device not found",
        "no such device",
        "access denied",
        "permission denied",
        "in use",
    ];
    if unreachable.iter().any(|needle| lower.contains(needle)) {
        return AppError::new(
            ErrorCode::ScannerUnavailable,
            "The scanner cannot be reached. Check that it is on and connected, and not in use by another program.",
        )
        .with_details(details);
    }
    AppError::new(ErrorCode::ScanFailed, "The scanner stopped with an error.").with_details(details)
}

/// The command line of a scan of `p` writing `<basename>_<n>.tiff`.
fn scan_args(p: &ScanParams, basename: &Path) -> Vec<OsString> {
    let frames: Vec<String> = p.frames.clone().map(|f| f.to_string()).collect();
    let mut args: Vec<OsString> = vec![
        "scan".into(),
        p.device_id.clone().into(),
        "--basename".into(),
        basename.into(),
        "--frames".into(),
        frames.join(",").into(),
        "--dpi".into(),
        p.dpi.to_string().into(),
        "--samples".into(),
        p.samples.to_string().into(),
        // One exposure for the whole roll, as the spike used it, and the film stays in for the next part.
        "--lock-ae".into(),
        "--no-eject".into(),
    ];
    if p.ir {
        args.push("--ir".into());
    }
    args
}

/// The scanners `nkscan list` shows. A line looks like `Nikon LS-50 ED (usb:01-1.4)`; the id is the
/// `usb:` token when there is one and the whole line otherwise. Scanners marked "in use" (another
/// driver holds them) are left out.
fn parse_list(stdout: &str) -> Vec<DeviceInfo> {
    let mut devices = Vec::new();
    let mut listing = false;
    for line in stdout.lines() {
        let line = line.trim();
        if line.to_lowercase().starts_with("attached scanners") {
            listing = true;
            continue;
        }
        if !listing || line.is_empty() || line.to_lowercase().contains("(in use)") {
            continue;
        }
        let name = line.trim_start_matches(['-', '*', ' ']).trim();
        let id = name
            .split_whitespace()
            .find(|word| word.contains("usb:"))
            .map(|word| {
                let word = word.trim_matches(|c| "()[],".contains(c));
                word.to_owned()
            })
            .unwrap_or_else(|| name.to_owned());
        devices.push(DeviceInfo {
            id,
            name: name.to_owned(),
        });
    }
    devices
}

/// One output file of a scan: its frame number (counted from 1), and whether it is the infrared one.
fn parse_output_name(name: &str) -> Option<(u32, bool)> {
    let lower = name.to_lowercase();
    let stem = lower
        .strip_suffix(".tiff")
        .or_else(|| lower.strip_suffix(".tif"))?;
    let stem = stem.strip_prefix(BASENAME)?.strip_prefix('_')?;
    let (digits, ir) = match stem.strip_suffix("_ir") {
        Some(digits) => (digits, true),
        None => (stem, false),
    };
    Some((digits.parse().ok()?, ir))
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Output {
    rgb: Option<(PathBuf, u64)>,
    ir: Option<(PathBuf, u64)>,
}

/// The files of a scan folder by frame number, with their sizes.
fn find_outputs(dir: &Path) -> BTreeMap<u32, Output> {
    let mut found: BTreeMap<u32, Output> = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let Some((n, ir)) = entry.file_name().to_str().and_then(parse_output_name) else {
            continue;
        };
        let size = entry.metadata().map_or(0, |m| m.len());
        let slot = found.entry(n).or_default();
        let file = Some((entry.path(), size));
        if ir {
            slot.ir = file;
        } else {
            slot.rgb = file;
        }
    }
    found
}

/// A folder for one scan, removed when dropped.
struct ScanDir(PathBuf);

impl ScanDir {
    fn create(parent: &Path) -> Result<Self, AppError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = parent.join(format!("gtp-nkscan-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|err| {
            AppError::new(ErrorCode::Io, "The folder for the scan could not be made.")
                .with_details(format!("{}: {err}", dir.display()))
        })?;
        Ok(Self(dir))
    }
}

impl Drop for ScanDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Reads one frame of a scan; the infrared plane only when `p.ir` asks for it.
fn read_frame(output: &Output, p: &ScanParams) -> Result<RawScan, AppError> {
    let failed = |message: &str, details: String| {
        AppError::new(ErrorCode::ScanFailed, message).with_details(details)
    };
    let Some((rgb_path, _)) = &output.rgb else {
        return Err(failed("The scanner wrote no picture.", String::new()));
    };
    let limits = Limits::default();
    let image = read_tiff16(rgb_path, &limits)?;
    let (rgb, mut ir) = match image.channels {
        3 => (image, None),
        4 => {
            let (rgb, ir) = split_rgbi(&image)?;
            (rgb, Some(ir))
        }
        channels => {
            return Err(failed(
                "The scanner wrote a picture that is not colour.",
                format!("{}: {channels} channels", rgb_path.display()),
            ))
        }
    };
    if p.ir && ir.is_none() {
        if let Some((path, _)) = &output.ir {
            ir = Some(read_tiff16(path, &limits)?);
        }
    }
    let ir16 = if p.ir {
        let plane = ir.ok_or_else(|| {
            failed(
                "The scan has no infrared plane.",
                rgb_path.display().to_string(),
            )
        })?;
        if plane.channels != 1 || (plane.width, plane.height) != (rgb.width, rgb.height) {
            return Err(failed(
                "The infrared plane does not match its picture.",
                rgb_path.display().to_string(),
            ));
        }
        Some(plane.data)
    } else {
        None
    };
    Ok(RawScan {
        width: rgb.width,
        height: rgb.height,
        rgb16: rgb.data,
        ir16,
        dpi: p.dpi,
        meta: ScanMeta {
            device_id: p.device_id.clone(),
            samples: p.samples,
            autofocus: p.autofocus,
        },
    })
}

/// Whether the files of frame `n` are all there and have stopped growing since `last` was taken.
fn is_ready(output: &Output, wants_ir: bool, last: Option<&Output>, exited: bool) -> bool {
    let complete = output.rgb.as_ref().is_some_and(|(_, size)| *size > 0)
        && (!wants_ir || output.ir.as_ref().is_some_and(|(_, size)| *size > 0));
    complete && (exited || last == Some(output))
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

impl ScannerSource for NkscanSource {
    fn list_devices(&self) -> Result<Vec<DeviceInfo>, AppError> {
        self.check_version()?;
        let output = self.run(&["list"])?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            return Err(failure(&stderr));
        }
        let devices = parse_list(&stdout);
        if devices.is_empty() {
            let held = stdout.to_lowercase().contains("(in use)");
            return Err(AppError::new(
                ErrorCode::ScannerUnavailable,
                if held {
                    "The scanner is held by another driver. On Windows nkscan needs the WinUSB driver (see the scanner setup notes)."
                } else {
                    "No scanner was found. Check that it is on and connected."
                },
            )
            .with_details(stdout.trim().to_owned()));
        }
        Ok(devices)
    }

    fn scan(
        &self,
        p: &ScanParams,
        on_frame: &mut dyn FnMut(u32, RawScan) -> Result<(), AppError>,
        progress: &dyn Fn(f32),
        cancel: &CancelToken,
    ) -> Result<(), AppError> {
        p.validate()?;
        cancel.check()?;
        self.check_version()?;
        let dir = ScanDir::create(&self.work_dir)?;
        let basename = dir.0.join(BASENAME);

        let mut child = self
            .command()
            .args(scan_args(p, &basename))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| not_started(&self.program, &err))?;
        let tail: Arc<Mutex<VecDeque<String>>> = Arc::default();
        let reader = child.stderr.take().map(|stderr| {
            let tail = Arc::clone(&tail);
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Ok(mut lines) = tail.lock() {
                        if lines.len() == STDERR_LINES {
                            lines.pop_front();
                        }
                        lines.push_back(line);
                    }
                }
            })
        });
        let said = || -> String {
            tail.lock()
                .map(|lines| Vec::from_iter(lines.iter().cloned()).join("\n"))
                .unwrap_or_default()
        };

        let wanted: Vec<u32> = p.frames.clone().collect();
        let mut delivered = 0usize;
        let mut last: BTreeMap<u32, Output> = BTreeMap::new();
        progress(0.0);
        loop {
            if cancel.is_cancelled() {
                stop(&mut child);
                return cancel.check();
            }
            let status: Option<ExitStatus> = child.try_wait().map_err(|err| {
                AppError::new(ErrorCode::ScanFailed, "The scanner program was lost.")
                    .with_details(err.to_string())
            })?;
            let exited = status.is_some();
            let outputs = find_outputs(&dir.0);
            while let Some(&number) = wanted.get(delivered) {
                // nkscan counts from 1, whatever frame numbers were asked for.
                let n = u32::try_from(delivered + 1).unwrap_or(u32::MAX);
                let ready = outputs
                    .get(&n)
                    .filter(|o| is_ready(o, p.ir, last.get(&n), exited));
                let Some(output) = ready else { break };
                let result = read_frame(output, p).and_then(|scan| on_frame(number, scan));
                if let Err(err) = result {
                    stop(&mut child);
                    return Err(err);
                }
                for file in [&output.rgb, &output.ir].into_iter().flatten() {
                    let _ = std::fs::remove_file(&file.0);
                }
                delivered += 1;
                progress(delivered as f32 / wanted.len() as f32);
            }
            last = outputs;
            if let Some(status) = status {
                if let Some(reader) = reader {
                    let _ = reader.join();
                }
                if !status.success() {
                    return Err(failure(&said()));
                }
                if delivered < wanted.len() {
                    return Err(AppError::new(
                        ErrorCode::ScanFailed,
                        "The scanner finished without all the frames.",
                    )
                    .with_details(format!(
                        "{delivered} of {} frames\n{}",
                        wanted.len(),
                        said()
                    )));
                }
                progress(1.0);
                return Ok(());
            }
            std::thread::sleep(POLL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(p: &ScanParams) -> Vec<String> {
        scan_args(p, Path::new("out/frame"))
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_scan_becomes_one_command_line_for_the_whole_range() {
        let mut p = ScanParams::new("usb:01-1.4", 2..=4);
        p.dpi = 2000;
        p.samples = 4;
        assert_eq!(
            args(&p),
            [
                "scan",
                "usb:01-1.4",
                "--basename",
                "out/frame",
                "--frames",
                "2,3,4",
                "--dpi",
                "2000",
                "--samples",
                "4",
                "--lock-ae",
                "--no-eject",
                "--ir"
            ]
        );
        p.ir = false;
        assert!(!args(&p).contains(&"--ir".to_string()));
        assert_eq!(
            args(&ScanParams::new("x", 7..=7))
                .iter()
                .skip_while(|a| *a != "--frames")
                .nth(1),
            Some(&"7".to_string())
        );
    }

    #[test]
    fn the_scanner_list_gives_ids_and_leaves_out_scanners_in_use() {
        let listing = "Attached scanners:\n  Nikon LS-50 ED (usb:01-1.4)\n  LS-50 (in use)\n";
        assert_eq!(
            parse_list(listing),
            [DeviceInfo {
                id: "usb:01-1.4".into(),
                name: "Nikon LS-50 ED (usb:01-1.4)".into()
            }]
        );
        let plain = parse_list("Attached scanners:\n- Nikon LS-50 ED\n");
        assert_eq!(
            (plain[0].id.as_str(), plain[0].name.as_str()),
            ("Nikon LS-50 ED", "Nikon LS-50 ED")
        );
        assert!(parse_list("Attached scanners:\n").is_empty());
        assert!(parse_list("licence text that is not a listing").is_empty());
    }

    #[test]
    fn output_files_are_told_apart_by_number_and_plane() {
        assert_eq!(parse_output_name("frame_1.tiff"), Some((1, false)));
        assert_eq!(parse_output_name("frame_12_IR.tiff"), Some((12, true)));
        assert_eq!(parse_output_name("Frame_03.TIF"), Some((3, false)));
        assert_eq!(parse_output_name("frame_1_thumbnail.tiff"), None);
        assert_eq!(parse_output_name("frame_1.jpg"), None);
        assert_eq!(parse_output_name("other_1.tiff"), None);
    }

    #[test]
    fn failures_are_told_by_what_nkscan_said() {
        let code = |text: &str| failure(text).code;
        assert_eq!(
            code("Error: incompatible driver is installed for this device"),
            ErrorCode::ScannerUnavailable
        );
        assert_eq!(code("No scanner found"), ErrorCode::ScannerUnavailable);
        assert_eq!(code("device is in use"), ErrorCode::ScannerUnavailable);
        assert_eq!(code("focus failed on frame 3"), ErrorCode::ScanFailed);
        assert_eq!(
            failure("focus failed").details.as_deref(),
            Some("focus failed")
        );
    }

    #[test]
    fn a_frame_is_ready_when_its_files_are_whole_and_have_stopped_growing() {
        let file = |size| Some((PathBuf::from("x"), size));
        let out = |rgb, ir| Output {
            rgb: file(rgb),
            ir: file(ir),
        };
        assert!(!is_ready(&out(10, 0), true, Some(&out(10, 0)), false));
        assert!(!is_ready(&out(10, 5), true, None, false));
        assert!(!is_ready(&out(10, 5), true, Some(&out(8, 5)), false));
        assert!(is_ready(&out(10, 5), true, Some(&out(10, 5)), false));
        assert!(
            is_ready(&out(10, 5), true, None, true),
            "the process has ended"
        );
        assert!(is_ready(&out(10, 0), false, Some(&out(10, 0)), false));
        assert!(!is_ready(&Output::default(), false, None, true));
    }
}
