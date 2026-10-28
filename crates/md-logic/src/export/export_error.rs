//! Port of md.win `Export/ExportException.cs` (`ExportException`, `PdfPaginationException`) as one
//! error enum, plus the two things C# expresses with exceptions md.win catches elsewhere: a
//! cancelled export (`OperationCanceledException`, silent) and any other failure whose message the
//! alert shows (an `IOException` from the disk, the renderer's own error).

use std::fmt;
use std::io;

use md_core::export::EpubError;

use crate::exec::Cancelled;
use crate::seams::RenderError;
use crate::strings::exports;

/// Why an export did not finish. The pipeline shows `title = "Could not export …"` and
/// `message = error.to_string()` (§7.9) for every variant but [`ExportError::Cancelled`], so the
/// text each variant carries is user-visible and lives in [`strings::exports`](crate::strings::exports),
/// never spelled at the place that fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportError {
    /// The window closed under the export (C#'s `OperationCanceledException`): there is no one left
    /// to tell, so nothing is said.
    Cancelled,
    /// md.win's `ExportException`: an export that cannot finish, carrying the alert body verbatim.
    Export(String),
    /// md.win's `PdfPaginationException`: the browser produced no PDF pages — its own verdict
    /// rather than ours, the macOS `PDFPaginationError`.
    PdfPagination,
    /// Anything else (an `io::Error` writing the file, a renderer that failed, an archive that did
    /// not fit): its own message, as C#'s `e.Message` would be.
    Failed(String),
}

impl ExportError {
    /// The alert body (§7.9). Empty for [`ExportError::Cancelled`], which is never shown.
    pub fn message(&self) -> String {
        match self {
            ExportError::Cancelled => String::new(),
            ExportError::Export(message) | ExportError::Failed(message) => message.clone(),
            ExportError::PdfPagination => exports::PDF_PAGES_NOT_PRODUCED.to_string(),
        }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Cancelled => f.write_str("the export was cancelled"),
            other => f.write_str(&other.message()),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<Cancelled> for ExportError {
    fn from(_: Cancelled) -> Self {
        ExportError::Cancelled
    }
}

/// A cancelled surface is a cancelled export; a failed one says what failed — md.win's
/// `ExportRenderer` throws `ExportException("The rendered page could not be captured.")` for a
/// navigation that failed, which arrives here as that message.
impl From<RenderError> for ExportError {
    fn from(error: RenderError) -> Self {
        match error {
            RenderError::Cancelled => ExportError::Cancelled,
            RenderError::Failed(message) => ExportError::Failed(message),
        }
    }
}

impl From<io::Error> for ExportError {
    fn from(error: io::Error) -> Self {
        ExportError::Failed(error.to_string())
    }
}

impl From<EpubError> for ExportError {
    fn from(error: EpubError) -> Self {
        ExportError::Failed(error.to_string())
    }
}
