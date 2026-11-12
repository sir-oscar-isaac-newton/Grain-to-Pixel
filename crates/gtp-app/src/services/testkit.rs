//! A small library with synthetic frames, shared by the tests of the services that render.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use gtp_core::testing::synthetic::{self, Params, Pattern};
use gtp_core::{Frame, FrameId, FrameKind, Recipe, RollId, SourceFile, SourceRole};
use gtp_io::{from_f32_linear, write_tiff16, Compression};

use crate::library_fs::originals_rel;
use crate::services::develop::DevelopService;
use crate::services::library::Library;
use crate::services::roll_frame::{roll_create, RollInput};

/// A folder in the system temporary folder that no other test or run can share and that is
/// removed when the guard is dropped, also when the test fails by panic.
///
/// The name is the label, the process id, a counter of this process and a random id: process ids
/// are reused (Windows does it quickly), so a name made of the process id alone can meet a folder
/// an earlier run left behind. Removal errors are ignored. Close a database inside the folder
/// first, since Windows will not delete an open file.
pub(crate) struct TempDir(PathBuf);

static TEMP_DIRS: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    /// Creates the folder, named `gtp-<label>-<process id>-<count>-<random>`.
    pub(crate) fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gtp-{label}-{}-{}-{}",
            std::process::id(),
            TEMP_DIRS.fetch_add(1, Ordering::Relaxed),
            FrameId::new()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) struct Fixture {
    pub(crate) dir: PathBuf,
    pub(crate) library: Arc<Library>,
    pub(crate) roll: RollId,
    pub(crate) service: Arc<DevelopService>,
    /// Last, so the folder goes after the library has closed its database.
    _temp: TempDir,
}

/// Adds a frame to a roll whose original is a synthetic negative of the given size.
pub(crate) fn add_frame(
    library: &Library,
    roll: RollId,
    index: u32,
    width: u32,
    height: u32,
) -> FrameId {
    add_frame_with(library, roll, index, width, height, Pattern::default())
}

/// [`add_frame`] with a chosen picture (the default one is a smooth gradient).
pub(crate) fn add_frame_with(
    library: &Library,
    roll: RollId,
    index: u32,
    width: u32,
    height: u32,
    pattern: Pattern,
) -> FrameId {
    let params = Params {
        width,
        height,
        rebate: [width / 14; 4],
        pattern,
        ..Params::default()
    };
    add_frame_params(library, roll, index, &params)
}

/// A frame whose original is the synthetic negative of `params`, with its infrared plane (as a
/// file of its own, like the LS-50 import makes) when the parameters ask for one.
pub(crate) fn add_frame_params(
    library: &Library,
    roll: RollId,
    index: u32,
    params: &Params,
) -> FrameId {
    let (width, height) = (params.width, params.height);
    let scan = synthetic::negative(u64::from(index), params).unwrap();
    let id = FrameId::new();
    let rel = originals_rel(roll, id).join("scan.tif");
    let path = library.resolve(&rel).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    write_tiff16(&path, &from_f32_linear(&scan.image), Compression::None).unwrap();
    let ir_rel = originals_rel(roll, id).join("ir.tif");
    if let Some(ir) = &scan.ir {
        let ir_path = library.resolve(&ir_rel).unwrap();
        write_tiff16(&ir_path, &from_f32_linear(ir), Compression::None).unwrap();
    }
    library
        .write(|c| {
            c.insert_frame(&Frame {
                id,
                roll_id: roll,
                index_in_roll: index,
                kind: FrameKind::ScanRgbIr,
                width,
                height,
                has_ir: scan.ir.is_some(),
                note: String::new(),
                flag: gtp_core::FrameFlag::None,
                created_at: String::new(),
                edited_at: None,
            })?;
            c.insert_source(&SourceFile {
                frame_id: id,
                role: SourceRole::Rgb,
                rel_path: rel.to_string_lossy().replace('\\', "/"),
                sha256: String::new(),
                bytes: 0,
            })?;
            if scan.ir.is_some() {
                c.insert_source(&SourceFile {
                    frame_id: id,
                    role: SourceRole::Ir,
                    rel_path: ir_rel.to_string_lossy().replace('\\', "/"),
                    sha256: String::new(),
                    bytes: 0,
                })?;
            }
            Ok(())
        })
        .unwrap();
    id
}

impl Fixture {
    pub(crate) fn new(label: &str) -> Self {
        let temp = TempDir::new(&format!("develop-{label}"));
        let dir = temp.path().to_owned();
        let library = Arc::new(Library::create(&dir, "Develop").unwrap());
        let roll = roll_create(
            &library,
            RollInput {
                name: "Roll".into(),
                ..RollInput::default()
            },
        )
        .unwrap()
        .id;
        Self {
            dir,
            library,
            roll,
            service: Arc::new(DevelopService::default()),
            _temp: temp,
        }
    }

    /// A frame whose original is a synthetic negative of the given size.
    pub(crate) fn frame(&self, index: u32, width: u32, height: u32) -> FrameId {
        add_frame(&self.library, self.roll, index, width, height)
    }

    /// A frame whose picture is `pattern` (steps, blobs, ...), for tests that need edges.
    pub(crate) fn frame_with(
        &self,
        index: u32,
        width: u32,
        height: u32,
        pattern: Pattern,
    ) -> FrameId {
        add_frame_with(&self.library, self.roll, index, width, height, pattern)
    }

    /// A frame whose original and infrared plane are the synthetic negative of `params`.
    pub(crate) fn frame_params(&self, index: u32, params: &Params) -> FrameId {
        add_frame_params(&self.library, self.roll, index, params)
    }

    /// A frame with no original file, enough for saving and copying recipes.
    pub(crate) fn bare(&self, index: u32) -> FrameId {
        let id = FrameId::new();
        self.library
            .write(|c| {
                c.insert_frame(&Frame {
                    id,
                    roll_id: self.roll,
                    index_in_roll: index,
                    kind: FrameKind::ScanRgbIr,
                    width: 10,
                    height: 10,
                    has_ir: false,
                    note: String::new(),
                    flag: gtp_core::FrameFlag::None,
                    created_at: String::new(),
                    edited_at: None,
                })
            })
            .unwrap();
        id
    }

    pub(crate) fn stored(&self, frame: FrameId) -> Option<Recipe> {
        self.library.read(|c| c.load_recipe(frame)).unwrap()
    }

    pub(crate) fn edited(&self, frame: FrameId) -> bool {
        self.library
            .read(|c| c.get_frame(frame))
            .unwrap()
            .edited_at
            .is_some()
    }
}
