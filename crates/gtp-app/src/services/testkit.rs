//! A small library with synthetic frames, shared by the tests of the services that render.

use std::path::PathBuf;
use std::sync::Arc;

use gtp_core::testing::synthetic::{self, Params, Pattern};
use gtp_core::{Frame, FrameId, FrameKind, Recipe, RollId, SourceFile, SourceRole};
use gtp_io::{from_f32_linear, write_tiff16, Compression};

use crate::library_fs::originals_rel;
use crate::services::develop::DevelopService;
use crate::services::library::Library;
use crate::services::roll_frame::{roll_create, RollInput};

pub(crate) struct Fixture {
    pub(crate) dir: PathBuf,
    pub(crate) library: Arc<Library>,
    pub(crate) roll: RollId,
    pub(crate) service: Arc<DevelopService>,
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
    let scan = synthetic::negative(
        u64::from(index),
        &Params {
            width,
            height,
            rebate: [width / 14; 4],
            pattern,
            ..Params::default()
        },
    )
    .unwrap();
    let id = FrameId::new();
    let rel = originals_rel(roll, id).join("scan.tif");
    let path = library.resolve(&rel).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    write_tiff16(&path, &from_f32_linear(&scan.image), Compression::None).unwrap();
    library
        .write(|c| {
            c.insert_frame(&Frame {
                id,
                roll_id: roll,
                index_in_roll: index,
                kind: FrameKind::ScanRgbIr,
                width,
                height,
                has_ir: false,
                note: String::new(),
                created_at: String::new(),
                edited_at: None,
            })?;
            c.insert_source(&SourceFile {
                frame_id: id,
                role: SourceRole::Rgb,
                rel_path: rel.to_string_lossy().replace('\\', "/"),
                sha256: String::new(),
                bytes: 0,
            })
        })
        .unwrap();
    id
}

impl Fixture {
    pub(crate) fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gtp-develop-{label}-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
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

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
