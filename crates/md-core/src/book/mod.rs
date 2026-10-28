//! Port of md.win `Md.Core.Book`: writer mode's folder-as-a-book model, ordering, renumbering,
//! compile, the in-place article session and writing statistics.

pub mod article_file_system;
pub mod book_article_session;
pub mod book_compiler;
pub mod book_folder;
pub mod book_model;
pub mod book_naming;
pub mod book_order;
pub mod book_paths;
pub mod destination;
pub mod renumber_plan;
pub mod structured_book;
pub mod writing_stats;
