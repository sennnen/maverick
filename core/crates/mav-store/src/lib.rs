//! Append-only storage over SQLite (rusqlite, WAL). The model and its three tiers are in
//! docs/storage.md; the short version is that raw evidence is inserted and never revised, and
//! everything derived is a computation over it that can be dropped and redone.
//!
//! This crate carries the Milestone 1 slice of that model: the append-only sample table with an
//! idempotent natural key (so a re-sync lands once), the provenance table that the walk-back
//! requirement reads, and the durable error journal that is the ring log's persistent sibling.
//! The decoded-frame tier and the recomputed rollups arrive with the milestones that need them.
#![forbid(unsafe_code)]

mod migrations;
mod store;

pub use store::{InsertOutcome, JournalEntry, Provenance, Store};
