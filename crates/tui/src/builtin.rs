//! Lifetime of an in-process editor/viewer, including temporary body files.
use crate::{message::Callback, util::TempFile};
use slumber_console::editor::FileEditor;
use std::{fmt, path::PathBuf};

pub struct BuiltinFile {
    pub editor: FileEditor,
    pub on_close: Option<(TempFile, Callback<TempFile>)>,
    /// Keep read-only temporary files alive until the viewer is closed.
    pub _view_file: Option<TempFile>,
    /// Canonical edited source path, if this is a collection editor.
    pub collection: Option<PathBuf>,
}
impl fmt::Debug for BuiltinFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuiltinFile").finish_non_exhaustive()
    }
}
