//! The canonical model — Phase 1 onward.
//!
//! Reads and writes `archiva-model.sqlite` only. Nothing in here touches the
//! v1 database; `model_db::open_for_backfill` is the single door between them.
//!
//! The pipeline is walk → observe → resolve → apply, and the judgement is
//! concentrated in one pure function so it can be tested without a filesystem:
//!
//!   content_type  what kind of thing a file is, and what that conforms to
//!   capabilities  what an item can do right now — type grant AND instance check
//!   backfill      carrying an existing v1 library across, once
//!   extract       measuring files and making the copies views draw
//!   mutations     the write path — link, unlink, reorder, accept, dismiss
//!   projections   p_detail — one node and its links, in one call
//!   signals       gathering the eight observations (the only I/O)
//!   reconcile     the thirteen rules (pure, exhaustively tested)
//!   scan          carrying files to the ladder and writing down the answer

pub mod backfill;
pub mod capabilities;
pub mod content_type;
pub mod extract;
pub mod mutations;
pub mod projections;
pub mod reconcile;
pub mod scan;
pub mod search;
pub mod signals;
