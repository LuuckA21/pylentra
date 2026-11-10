// Used from Task 3 on.
#![allow(dead_code)]

pub(crate) mod names;

use serde::Serialize;
use ts_rs::TS;

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub enum EntryKind {
    File,
    Dir,
    Link,
    Other,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, TS)]
#[ts(export)]
pub enum Direction {
    Up,
    Down,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    /// Unix seconds.
    #[ts(type = "number | null")]
    pub modified: Option<u64>,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct Listing {
    pub dir: String,
    pub entries: Vec<Entry>,
    pub truncated: bool,
}
