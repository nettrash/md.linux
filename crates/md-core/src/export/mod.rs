//! Port of md.win `Md.Core.Export`: zip, TextBundle, HTML, PDF page CSS, EPUB, LaTeX, standalone
//! diagram SVG and export file names. The browser half of every export lives in the app.
//!
//! | Module | md.win source |
//! | --- | --- |
//! | [`zip_writer`] | `ZipWriter.cs` (`ZipEntry`, `ZipWriter`, CRC-32) |
//! | [`zip_reader`] | `ZipReader.cs` |
//! | [`inflate`] | what `DeflateStream` does for `ZipReader.cs` |
//! | [`text_bundle`] | `TextBundle.cs` (`TextBundle`, `BundleWrapper`) |
//! | [`export_file_names`] | `ExportFileNames.cs` |

pub mod epub_export;
pub mod epub_metadata;
pub mod export_file_names;
pub mod inflate;
pub mod text_bundle;
pub mod zip_reader;
pub mod zip_writer;

pub use epub_export::{
    EpubError, EpubImage, EpubPlan, EpubSnapshotCountError, EpubUnit, EpubUnitPlan, NavEntry, RichElement, RichSnapshot,
};
pub use epub_metadata::{CoverReference, EpubCover, EpubMetadata};
pub use text_bundle::{Asset, BundleWrapper, Rewrite};
pub use zip_writer::{ZipEntry, ZipError, ZipWriter};
