//! Helpers shared by the tests of the commands.

use gtp_app::AppContext;
use tauri::{App, Manager, Runtime};

/// Releases the app context (and with it the open library database) when dropped.
///
/// Tauri's mock app never drops the state it manages, so without this the library stays open until
/// the process ends and Windows will not let the test folder go. Declare it after the app and
/// after the folder guard: it then runs first, and the folder is removed last.
pub(crate) struct ReleaseContext<'a, R: Runtime>(pub(crate) &'a App<R>);

impl<R: Runtime> Drop for ReleaseContext<'_, R> {
    fn drop(&mut self) {
        release_context(self.0);
    }
}

/// Drops the managed app context. `unmanage` is deprecated because references to the state taken
/// earlier would dangle; the tests call this only at the end, when none is left.
#[allow(deprecated)]
pub(crate) fn release_context<R: Runtime>(app: &App<R>) {
    drop(app.unmanage::<AppContext>());
}
