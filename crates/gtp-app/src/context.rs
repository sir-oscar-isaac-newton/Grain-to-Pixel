//! What the whole app shares: the settings and the open library (at most one).

use std::path::Path;
use std::sync::{Arc, PoisonError, RwLock};

use gtp_core::{AppError, ErrorCode, FrameId};

use crate::jobs::LibraryEvents;
use crate::services::develop::DevelopService;
use crate::services::library::{Library, LibraryDetails};
use crate::services::settings::SettingsStore;
use crate::services::thumbnails::ThumbnailService;

pub struct AppContext {
    settings: SettingsStore,
    library: RwLock<Option<Arc<Library>>>,
    develop: Arc<DevelopService>,
    thumbnails: ThumbnailService,
}

impl AppContext {
    /// A context with the settings loaded from `settings_file` (see [`SettingsStore::load`] for
    /// the returned note) and no library open.
    pub fn new(settings_file: &Path) -> (Self, Option<String>) {
        let (settings, note) = SettingsStore::load(settings_file);
        let develop = DevelopService::default();
        develop.set_dust_removal(settings.get().flags.dust_removal);
        (
            Self {
                settings,
                library: RwLock::new(None),
                develop: Arc::new(develop),
                thumbnails: ThumbnailService::default(),
            },
            note,
        )
    }

    pub fn settings(&self) -> &SettingsStore {
        &self.settings
    }

    /// The frames open for editing in the Develop view.
    pub fn develop(&self) -> &DevelopService {
        &self.develop
    }

    /// Queues the thumbnails of these frames for regeneration from their saved recipes and returns
    /// at once (after a recipe was saved or applied). `events` hears of the changed frames when
    /// they are done.
    pub fn regenerate_thumbnails(
        &self,
        events: Arc<dyn LibraryEvents>,
        frames: Vec<FrameId>,
    ) -> Result<(), AppError> {
        self.thumbnails
            .request(self.library()?, Arc::clone(&self.develop), events, frames);
        Ok(())
    }

    /// A frame's thumbnail as bytes; a missing file is rebuilt from the saved recipe first.
    pub fn thumbnail(&self, frame: FrameId) -> Result<Vec<u8>, AppError> {
        self.thumbnails
            .thumbnail(&self.develop, &*self.library()?, frame)
    }

    /// The open library, or `NotFound` when none is open.
    pub fn library(&self) -> Result<Arc<Library>, AppError> {
        self.library
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No library is open."))
    }

    /// Details of the open library, if any.
    pub fn library_details(&self) -> Result<Option<LibraryDetails>, AppError> {
        match self.library() {
            Ok(library) => library.details().map(Some),
            Err(_) => Ok(None),
        }
    }

    /// Creates a library and makes it the open one.
    pub fn create_library(&self, path: &Path, name: &str) -> Result<LibraryDetails, AppError> {
        self.make_current(Library::create(path, name)?)
    }

    /// Opens a library and makes it the open one; the previous one closes once no job uses it.
    pub fn open_library(&self, path: &Path) -> Result<LibraryDetails, AppError> {
        self.make_current(Library::open(path)?)
    }

    fn make_current(&self, library: Library) -> Result<LibraryDetails, AppError> {
        let details = library.details()?;
        *self.library.write().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(library));
        // The proxies belong to the library that was open.
        self.develop.clear();
        self.settings.set_last_library(&details.path)?;
        Ok(details)
    }
}

#[cfg(test)]
mod tests {
    use gtp_core::FrameId;

    use super::*;

    #[test]
    fn opening_a_library_makes_it_current_and_remembers_it() {
        let dir = std::env::temp_dir().join(format!(
            "gtp-context-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let (ctx, _) = AppContext::new(&dir.join("config").join("settings.json"));
        assert_eq!(ctx.library().err().unwrap().code, ErrorCode::NotFound);
        assert_eq!(ctx.library_details().unwrap(), None);

        // Compared by path and name: free space changes between calls.
        let current = |ctx: &AppContext| ctx.library_details().unwrap().map(|d| (d.path, d.name));
        let created = ctx.create_library(&dir.join("lib"), "Film").unwrap();
        assert_eq!(
            current(&ctx),
            Some((created.path.clone(), created.name.clone()))
        );
        assert_eq!(
            ctx.settings().get().last_library,
            Some(created.path.clone())
        );

        let reopened = ctx.open_library(&dir.join("lib")).unwrap();
        assert_eq!(reopened.name, "Film");
        let err = ctx.open_library(&dir.join("nothing")).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(
            current(&ctx),
            Some((reopened.path, reopened.name)),
            "a failed open keeps the old one"
        );
        drop(ctx);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
