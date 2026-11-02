//! The manifest of reference pairs (Q-02, Q-09): each pair is a frame of the author's and the render
//! of the same frame made by Nikon Scan 4 or Trichroma Lab. The frame is either the three exposures
//! of a camera triplet or one LS-50 scan (a TIFF with the infrared plane). The manifest is a JSON
//! file next to the (private) samples; its paths are relative to its own folder, or absolute.

use std::path::{Path, PathBuf};

use gtp_core::{AppError, ErrorCode, FilmFormat};
use serde::Deserialize;

/// The roll the pairs belong to when the manifest names none.
pub const DEFAULT_ROLL: &str = "all";

/// Where the frame of a pair comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// The exposures lit red, green and blue.
    Triplet([PathBuf; 3]),
    /// One scan (16-bit RGB, or RGB and infrared).
    Scan(PathBuf),
}

/// One reference pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub name: String,
    /// The roll (or film stock) the frame is from: the film base is measured per roll.
    pub roll: String,
    pub source: Source,
    /// The reference render (a 16-bit sRGB TIFF).
    pub reference: PathBuf,
}

impl Pair {
    /// The files the pair is made of.
    pub fn files(&self) -> Vec<&Path> {
        let mut files: Vec<&Path> = match &self.source {
            Source::Triplet(exposures) => exposures.iter().map(PathBuf::as_path).collect(),
            Source::Scan(scan) => vec![scan.as_path()],
        };
        files.push(self.reference.as_path());
        files
    }
}

/// The pairs and what is known about them.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub format: FilmFormat,
    /// The film base when it is known (for every roll); measured on the frames of each roll the way
    /// the app does when not.
    pub film_base: Option<[f32; 3]>,
    pub pairs: Vec<Pair>,
}

#[derive(Deserialize)]
struct RawPair {
    name: String,
    #[serde(default)]
    roll: Option<String>,
    #[serde(default)]
    red: Option<String>,
    #[serde(default)]
    green: Option<String>,
    #[serde(default)]
    blue: Option<String>,
    #[serde(default)]
    scan: Option<String>,
    reference: String,
}

#[derive(Deserialize)]
struct RawManifest {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    film_base: Option<[f32; 3]>,
    pairs: Vec<RawPair>,
}

fn invalid(message: &str, details: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message).with_details(details.into())
}

/// Reads and checks a manifest: the format must be known, the base finite and above 0, the names
/// distinct, every pair either a triplet or a scan (all the pairs of a roll of one kind), and every
/// file must exist.
pub fn read(path: &Path) -> Result<Manifest, AppError> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        AppError::new(ErrorCode::NotFound, "The manifest could not be read.")
            .with_details(format!("{}: {err}", path.display()))
    })?;
    let folder = path.parent().unwrap_or(Path::new("."));
    parse(&text, folder)
}

/// [`read`] from the manifest's text; `folder` is what relative paths are taken from.
pub fn parse(text: &str, folder: &Path) -> Result<Manifest, AppError> {
    // Windows editors and PowerShell write a byte order mark that JSON does not allow.
    let raw: RawManifest = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|err| invalid("The manifest is not valid.", err.to_string()))?;
    let format = match raw.format.as_deref() {
        None => FilmFormat::Mf645,
        Some(text) => FilmFormat::parse(text).ok_or_else(|| {
            invalid(
                "The manifest names a film format that does not exist.",
                text,
            )
        })?,
    };
    if let Some(base) = raw.film_base {
        if !base.iter().all(|v| v.is_finite() && *v > 0.0) {
            return Err(invalid(
                "A film base needs three values above 0.",
                format!("{base:?}"),
            ));
        }
    }
    if raw.pairs.is_empty() {
        return Err(invalid("The manifest lists no pairs.", ""));
    }
    let resolve = |p: &str| {
        let p = PathBuf::from(p);
        if p.is_absolute() {
            p
        } else {
            folder.join(p)
        }
    };
    let mut pairs: Vec<Pair> = Vec::new();
    for pair in raw.pairs {
        if pairs.iter().any(|p| p.name == pair.name) {
            return Err(invalid("Two pairs have the same name.", pair.name));
        }
        let source = match (&pair.red, &pair.green, &pair.blue, &pair.scan) {
            (Some(r), Some(g), Some(b), None) => {
                Source::Triplet([resolve(r), resolve(g), resolve(b)])
            }
            (None, None, None, Some(scan)) => Source::Scan(resolve(scan)),
            _ => {
                return Err(invalid(
                    "A pair is either red, green and blue exposures or one scan.",
                    pair.name,
                ))
            }
        };
        let entry = Pair {
            roll: pair.roll.unwrap_or_else(|| DEFAULT_ROLL.to_owned()),
            reference: resolve(&pair.reference),
            name: pair.name,
            source,
        };
        if let Some(other) = pairs.iter().find(|p| {
            p.roll == entry.roll
                && std::mem::discriminant(&p.source) != std::mem::discriminant(&entry.source)
        }) {
            return Err(invalid(
                "The pairs of a roll must all be triplets or all be scans.",
                format!("{} and {} in {}", other.name, entry.name, entry.roll),
            ));
        }
        for file in entry.files() {
            if !file.is_file() {
                return Err(AppError::new(
                    ErrorCode::NotFound,
                    "A file of the manifest does not exist.",
                )
                .with_details(format!("{}: {}", entry.name, file.display())));
            }
        }
        pairs.push(entry);
    }
    Ok(Manifest {
        format,
        film_base: raw.film_base,
        pairs,
    })
}
/// A manifest for a folder of camera files and a folder of reference renders: the triplets in
/// capture order (the colour of each file read from its preview) matched to the references in file
/// name order, one to one. Pairs past the shorter list are left out and said so in the returned note.
pub fn build(
    triplets: &Path,
    references: &Path,
    format: &str,
) -> Result<(String, String), AppError> {
    let list = |folder: &Path, ext: &[&str]| -> Result<Vec<PathBuf>, AppError> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
            .map_err(|err| {
                AppError::new(ErrorCode::NotFound, "A folder could not be read.")
                    .with_details(format!("{}: {err}", folder.display()))
            })?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| ext.iter().any(|e| e.eq_ignore_ascii_case(x)))
            })
            .collect();
        files.sort();
        Ok(files)
    };
    let nefs = list(triplets, &["nef"])?;
    let renders = list(references, &["tif", "tiff"])?;
    let candidates = gtp_app::services::triplets::triplet_group(&nefs)?;
    let complete: Vec<_> = candidates
        .iter()
        .filter(|c| c.problem.is_none() && c.files.len() == 3)
        .collect();
    let count = complete.len().min(renders.len());
    let text = |p: &Path| p.display().to_string();
    let pairs: Vec<serde_json::Value> = complete
        .iter()
        .zip(&renders)
        .enumerate()
        .map(|(i, (c, reference))| {
            serde_json::json!({
                "name": format!("{:02}", i + 1),
                "red": c.files[0].path,
                "green": c.files[1].path,
                "blue": c.files[2].path,
                "reference": text(reference),
            })
        })
        .collect();
    let note = format!(
        "{} complete triplets and {} references: {count} pairs{}",
        complete.len(),
        renders.len(),
        if complete.len() == renders.len() {
            String::new()
        } else {
            " (the counts differ: check the order)".into()
        }
    );
    let json = serde_json::json!({ "format": format, "pairs": pairs });
    let text =
        serde_json::to_string_pretty(&json).map_err(|err| AppError::internal(err.to_string()))?;
    Ok((text, note))
}

/// The name of the positive that goes with a scan: "Vision3 250D ECN2 0001" is matched by
/// "Vision3 250D ECN2 Positive 0001" (Nikon Scan's naming). The stock, here "Vision3 250D ECN2", is
/// the roll. `None` for a name that does not end in a frame number.
pub fn positive_of(scan_stem: &str) -> Option<(String, String)> {
    let (stock, number) = scan_stem.trim().rsplit_once(' ')?;
    if stock.is_empty() || number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((stock.to_owned(), format!("{stock} Positive {number}")))
}

/// A manifest for a folder of LS-50 scans and a folder of Nikon Scan positives, matched by name (see
/// [`positive_of`]). The stock of each scan is its roll. A scan without a positive is left out and
/// said so in the returned note, as is a positive with no scan.
pub fn build_scans(
    scans: &Path,
    references: &Path,
    format: &str,
) -> Result<(String, String), AppError> {
    let list = |folder: &Path| -> Result<Vec<PathBuf>, AppError> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
            .map_err(|err| {
                AppError::new(ErrorCode::NotFound, "A folder could not be read.")
                    .with_details(format!("{}: {err}", folder.display()))
            })?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                    x.eq_ignore_ascii_case("tif") || x.eq_ignore_ascii_case("tiff")
                })
            })
            .collect();
        files.sort();
        Ok(files)
    };
    let stem = |p: &Path| {
        p.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let scan_files = list(scans)?;
    let renders = list(references)?;
    let mut pairs = Vec::new();
    let mut without = Vec::new();
    let mut matched = 0usize;
    for scan in &scan_files {
        let name = stem(scan);
        let reference = positive_of(&name).and_then(|(stock, positive)| {
            renders
                .iter()
                .find(|r| stem(r).eq_ignore_ascii_case(&positive))
                .map(|r| (stock, r))
        });
        match reference {
            Some((stock, reference)) => {
                matched += 1;
                pairs.push(serde_json::json!({
                    "name": name,
                    "roll": stock,
                    "scan": scan.display().to_string(),
                    "reference": reference.display().to_string(),
                }));
            }
            None => without.push(name),
        }
    }
    let note = format!(
        "{} scans and {} references: {matched} pairs{}",
        scan_files.len(),
        renders.len(),
        if without.is_empty() {
            String::new()
        } else {
            format!(" (no positive for {})", without.join(", "))
        }
    );
    let json = serde_json::json!({ "format": format, "pairs": pairs });
    let text =
        serde_json::to_string_pretty(&json).map_err(|err| AppError::internal(err.to_string()))?;
    Ok((text, note))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gtp-manifest-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["r.nef", "g.nef", "b.nef", "ref.tif"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        dir
    }

    fn pair(name: &str) -> String {
        format!(
            r#"{{"name":"{name}","red":"r.nef","green":"g.nef","blue":"b.nef","reference":"ref.tif"}}"#
        )
    }

    #[test]
    fn a_valid_manifest_resolves_paths_against_its_folder() {
        let dir = folder("ok");
        let text = format!(
            r#"{{"format":"6x7","film_base":[0.15,0.1,0.06],"pairs":[{}]}}"#,
            pair("01")
        );
        let manifest = parse(&text, &dir).unwrap();
        assert_eq!(manifest.format, FilmFormat::Mf6x7);
        assert_eq!(manifest.film_base, Some([0.15, 0.1, 0.06]));
        assert_eq!(manifest.pairs[0].reference, dir.join("ref.tif"));
        assert_eq!(
            manifest.pairs[0].source,
            Source::Triplet([dir.join("r.nef"), dir.join("g.nef"), dir.join("b.nef")])
        );
        assert_eq!(manifest.pairs[0].roll, DEFAULT_ROLL);

        // A byte order mark in front of the JSON, as Windows tools write it, is fine.
        let with_mark = format!("\u{feff}{{\"pairs\":[{}]}}", pair("01"));
        assert!(parse(&with_mark, &dir).is_ok());

        // Without a format the roll is 645, and without a base it is measured.
        let plain = parse(&format!(r#"{{"pairs":[{}]}}"#, pair("01")), &dir).unwrap();
        assert_eq!((plain.format, plain.film_base), (FilmFormat::Mf645, None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn building_a_manifest_needs_folders_and_leaves_out_what_cannot_be_paired() {
        let dir = folder("build");
        // Files that are not camera files cannot be grouped, so nothing is paired, and it says so.
        for name in ["a.NEF", "b.NEF", "c.NEF"] {
            std::fs::write(dir.join(name), b"not a camera file").unwrap();
        }
        let (text, note) = build(&dir, &dir, "645").unwrap();
        assert!(
            note.contains("0 complete triplets and 1 references: 0 pairs"),
            "{note}"
        );
        assert!(text.contains(r#""format": "645""#), "{text}");
        let missing = build(&dir.join("nope"), &dir, "645").unwrap_err();
        assert_eq!(missing.code, ErrorCode::NotFound);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_scan_pair_has_a_roll_and_a_roll_is_all_scans_or_all_triplets() {
        let dir = folder("scans");
        std::fs::write(dir.join("scan.tif"), b"x").unwrap();
        let scan = |name: &str, roll: &str| {
            format!(
                r#"{{"name":"{name}","roll":"{roll}","scan":"scan.tif","reference":"ref.tif"}}"#
            )
        };
        let ok = parse(
            &format!(
                r#"{{"format":"35mm","pairs":[{},{}]}}"#,
                scan("a", "Vision3 250D ECN2"),
                scan("b", "Vision3 500T")
            ),
            &dir,
        )
        .unwrap();
        assert_eq!(ok.format, FilmFormat::Mm35);
        assert_eq!(ok.pairs[0].source, Source::Scan(dir.join("scan.tif")));
        assert_eq!(ok.pairs[1].roll, "Vision3 500T");
        assert_eq!(ok.pairs[0].files().len(), 2);

        // Different rolls may be of different kinds; one roll may not mix them.
        let mixed_rolls = format!(
            r#"{{"pairs":[{},{}]}}"#,
            scan("a", "one"),
            pair("t").replace("}", r#","roll":"two"}"#)
        );
        assert!(parse(&mixed_rolls, &dir).is_ok());
        let mixed = format!(r#"{{"pairs":[{},{}]}}"#, scan("a", DEFAULT_ROLL), pair("t"));
        let err = parse(&mixed, &dir).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);

        // A pair is a triplet or a scan, nothing in between.
        let half =
            r#"{"pairs":[{"name":"x","red":"r.nef","scan":"scan.tif","reference":"ref.tif"}]}"#;
        assert_eq!(parse(half, &dir).unwrap_err().code, ErrorCode::InvalidInput);
        let none = r#"{"pairs":[{"name":"x","reference":"ref.tif"}]}"#;
        assert_eq!(parse(none, &dir).unwrap_err().code, ErrorCode::InvalidInput);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_positive_is_named_after_its_scan_and_matched_by_name() {
        assert_eq!(
            positive_of("Vision3 250D ECN2 0001"),
            Some((
                "Vision3 250D ECN2".to_owned(),
                "Vision3 250D ECN2 Positive 0001".to_owned()
            ))
        );
        assert_eq!(positive_of("Ektar100 C41 0007").unwrap().0, "Ektar100 C41");
        assert_eq!(positive_of("no number here"), None);
        assert_eq!(positive_of("0001"), None);
        assert_eq!(positive_of(""), None);

        let dir = std::env::temp_dir().join(format!("gtp-manifest-match-{}", std::process::id()));
        let scans = dir.join("scans");
        let refs = dir.join("refs");
        std::fs::create_dir_all(&scans).unwrap();
        std::fs::create_dir_all(&refs).unwrap();
        for name in [
            "Vision3 250D ECN2 0001.tif",
            "Vision3 250D ECN2 0002.tif",
            "Ektar100 C41 0001.tif",
        ] {
            std::fs::write(scans.join(name), b"x").unwrap();
        }
        for name in [
            "Vision3 250D ECN2 Positive 0001.tif",
            "Ektar100 C41 Positive 0001.TIF",
            "Vision3 500T Positive 0001.tif",
        ] {
            std::fs::write(refs.join(name), b"x").unwrap();
        }
        let (text, note) = build_scans(&scans, &refs, "35mm").unwrap();
        assert!(note.contains("3 scans and 3 references: 2 pairs"), "{note}");
        assert!(
            note.contains("no positive for Vision3 250D ECN2 0002"),
            "{note}"
        );
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let pairs = json["pairs"].as_array().unwrap();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0]["roll"], "Ektar100 C41");
        assert_eq!(pairs[1]["roll"], "Vision3 250D ECN2");
        // The manifest it writes is one the parser accepts.
        let written = dir.join("manifest.json");
        std::fs::write(&written, &text).unwrap();
        let manifest = read(&written).unwrap();
        assert_eq!(manifest.format, FilmFormat::Mm35);
        assert_eq!(manifest.pairs.len(), 2);
        assert_eq!(
            build_scans(&dir.join("nope"), &refs, "35mm")
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bad_manifest_is_refused_with_its_reason() {
        let dir = folder("bad");
        let code = |text: String| parse(&text, &dir).unwrap_err().code;
        assert_eq!(code("not json".into()), ErrorCode::InvalidInput);
        assert_eq!(code(r#"{"pairs":[]}"#.into()), ErrorCode::InvalidInput);
        assert_eq!(
            code(format!(r#"{{"format":"8x10","pairs":[{}]}}"#, pair("01"))),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            code(format!(
                r#"{{"film_base":[0,0.1,0.1],"pairs":[{}]}}"#,
                pair("01")
            )),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            code(format!(r#"{{"pairs":[{},{}]}}"#, pair("01"), pair("01"))),
            ErrorCode::InvalidInput,
            "names must be distinct"
        );
        let missing = r#"{"pairs":[{"name":"x","red":"nope.nef","green":"g.nef","blue":"b.nef","reference":"ref.tif"}]}"#;
        assert_eq!(code(missing.into()), ErrorCode::NotFound);
        assert_eq!(
            read(&dir.join("no-such.json")).unwrap_err().code,
            ErrorCode::NotFound
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
