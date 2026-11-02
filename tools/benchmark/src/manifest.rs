//! The manifest of reference pairs (Q-02): each pair is the three exposures of a camera triplet and
//! the render of the same frame made by Nikon Scan 4 or Trichroma Lab. The manifest is a JSON file
//! next to the (private) samples; its paths are relative to its own folder, or absolute.

use std::path::{Path, PathBuf};

use gtp_core::{AppError, ErrorCode, FilmFormat};
use serde::Deserialize;

/// One reference pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub name: String,
    /// The exposures lit red, green and blue.
    pub exposures: [PathBuf; 3],
    /// The reference render (a 16-bit sRGB TIFF).
    pub reference: PathBuf,
}

/// The pairs of a roll and what is known about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub format: FilmFormat,
    /// The roll's film base when it is known; measured on the frames the way the app does when not.
    pub film_base: Option<[f32; 3]>,
    pub pairs: Vec<Pair>,
}

#[derive(Deserialize)]
struct RawPair {
    name: String,
    red: String,
    green: String,
    blue: String,
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
/// distinct, and every file must exist.
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
    let raw: RawManifest = serde_json::from_str(text)
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
    let mut pairs = Vec::new();
    for pair in raw.pairs {
        if pairs.iter().any(|p: &Pair| p.name == pair.name) {
            return Err(invalid("Two pairs have the same name.", pair.name));
        }
        let entry = Pair {
            exposures: [
                resolve(&pair.red),
                resolve(&pair.green),
                resolve(&pair.blue),
            ],
            reference: resolve(&pair.reference),
            name: pair.name,
        };
        for file in entry.exposures.iter().chain([&entry.reference]) {
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
        assert_eq!(manifest.pairs[0].exposures[2], dir.join("b.nef"));

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
