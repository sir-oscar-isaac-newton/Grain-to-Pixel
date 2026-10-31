//! Grouping picked files into TIFF scans to import. Only names are looked at here; the import
//! reads and checks the pixels.
//!
//! A scan is a `.tif`/`.tiff` file. An infrared plane written as a separate file is paired with its
//! scan by name: `<stem>_ir.tif` (any case, either extension) belongs to `<stem>.tif`, which is how
//! nkscan writes them (`frame_01.tiff` + `frame_01_IR.tiff`). A 4-channel RGBI file needs no pair.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One frame to import: its scan and, if there is one, its separate infrared file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiffCandidate {
    pub scan: PathBuf,
    pub ir: Option<PathBuf>,
}

/// A picked file that will not be imported, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejected {
    /// Not a `.tif` or `.tiff` file.
    NotTiff(PathBuf),
    /// An `_ir` file whose scan was not picked.
    OrphanIr(PathBuf),
}

/// The candidates in file-name order, and the rejected files.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Grouped {
    pub candidates: Vec<TiffCandidate>,
    pub rejected: Vec<Rejected>,
}

fn is_tiff(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("tif") || e.eq_ignore_ascii_case("tiff"))
}

/// The lower-cased stem of an infrared file without its `_ir` suffix, or `None` for a scan.
fn ir_base(stem: &str) -> Option<String> {
    let lower = stem.to_lowercase();
    lower.strip_suffix("_ir").map(str::to_owned)
}

/// Groups `paths` (as picked; duplicates are ignored) into scans with their infrared files.
pub fn scan_tiff_candidates(paths: &[PathBuf]) -> Grouped {
    let mut grouped = Grouped::default();
    // Keyed by (folder, lower-case stem) so pairing ignores case but not folders.
    let mut scans: BTreeMap<(PathBuf, String), PathBuf> = BTreeMap::new();
    let mut irs: BTreeMap<(PathBuf, String), PathBuf> = BTreeMap::new();
    for path in paths {
        if !is_tiff(path) {
            if !grouped.rejected.contains(&Rejected::NotTiff(path.clone())) {
                grouped.rejected.push(Rejected::NotTiff(path.clone()));
            }
            continue;
        }
        let folder = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        match ir_base(&stem) {
            Some(base) => irs.entry((folder, base)).or_insert_with(|| path.clone()),
            None => scans
                .entry((folder, stem.to_lowercase()))
                .or_insert_with(|| path.clone()),
        };
    }
    for (key, scan) in scans {
        let ir = irs.remove(&key);
        grouped.candidates.push(TiffCandidate { scan, ir });
    }
    grouped
        .rejected
        .extend(irs.into_values().map(Rejected::OrphanIr));
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn scans_pair_with_their_infrared_files_by_name() {
        let grouped = scan_tiff_candidates(&[
            p("roll/frame_02.tiff"),
            p("roll/frame_01.tiff"),
            p("roll/frame_01_IR.tiff"),
            p("roll/frame_03.TIF"),
            p("roll/FRAME_03_ir.tif"),
        ]);
        assert_eq!(
            grouped.candidates,
            vec![
                TiffCandidate {
                    scan: p("roll/frame_01.tiff"),
                    ir: Some(p("roll/frame_01_IR.tiff"))
                },
                TiffCandidate {
                    scan: p("roll/frame_02.tiff"),
                    ir: None
                },
                TiffCandidate {
                    scan: p("roll/frame_03.TIF"),
                    ir: Some(p("roll/FRAME_03_ir.tif"))
                },
            ]
        );
        assert!(grouped.rejected.is_empty());
    }

    #[test]
    fn other_files_orphan_ir_and_duplicates_are_handled() {
        let grouped = scan_tiff_candidates(&[
            p("a/scan.tif"),
            p("a/scan.tif"),
            p("a/notes.txt"),
            p("a/notes.txt"),
            p("a/lonely_ir.tif"),
            p("b/scan_ir.tif"),
            p("a/photo.jpg"),
        ]);
        assert_eq!(
            grouped.candidates,
            vec![TiffCandidate {
                scan: p("a/scan.tif"),
                ir: None
            }],
            "an _ir file in another folder does not pair"
        );
        assert_eq!(
            grouped.rejected,
            vec![
                Rejected::NotTiff(p("a/notes.txt")),
                Rejected::NotTiff(p("a/photo.jpg")),
                Rejected::OrphanIr(p("a/lonely_ir.tif")),
                Rejected::OrphanIr(p("b/scan_ir.tif")),
            ]
        );
    }
}
