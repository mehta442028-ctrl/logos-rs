//! Core (non-UI) library for logos-rs: canonical book data, the Bible data
//! model over SQLite, reference parsing, the data importer, the advanced
//! search engine, and the saved session.

pub mod bible;
pub mod canon;
pub mod importer;
pub mod morph;
pub mod reference;
pub mod search;
pub mod session;

pub mod personal_import;

pub mod auto_align;

pub mod study;
