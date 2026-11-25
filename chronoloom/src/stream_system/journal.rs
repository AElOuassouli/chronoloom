//! What a live update changed, so that a failed update can be undone without
//! copying the system.

use std::collections::BTreeMap;

use crate::primitives::Timestamp;
use crate::store::{Key, Tail};

use super::mode::LiveNode;
use super::{Live, StreamSystem};

/// The record of one live update: the part of each stream it recorded into,
/// the frontiers it moved, the running state of each processor it stepped
/// and what it gave to the derived streams. Each is the previous state of
/// what was touched only, so keeping the record costs what the update
/// changes and not what the system holds.
#[derive(Debug, Default)]
pub(super) struct Journal {
    streams: Vec<(Key, Tail)>,
    frontiers: Vec<(Key, Timestamp)>,
    nodes: BTreeMap<usize, LiveNode>,
    derived: Vec<Derived>,
}

/// How a derived stream was changed.
#[derive(Debug)]
enum Derived {
    /// It did not exist before.
    Created(Key),
    /// A chunk was appended to it; the tail is what it held before.
    Appended(Key, Tail),
}

impl Journal {
    /// A stream is about to be recorded into; `tail` is what it holds.
    pub(super) fn recorded(&mut self, key: Key, tail: Tail) {
        self.streams.push((key, tail));
    }

    /// The frontier of the stream under `key`, which was `frontier`, moved.
    pub(super) fn moved(&mut self, key: Key, frontier: Timestamp) {
        self.frontiers.push((key, frontier));
    }

    /// The processor at `index` is about to be stepped. Only the first call
    /// for an index keeps its state.
    pub(super) fn stepped(&mut self, index: usize, node: &LiveNode) {
        self.nodes.entry(index).or_insert_with(|| node.clone());
    }

    /// The derived stream under `key` was made.
    pub(super) fn created(&mut self, key: Key) {
        self.derived.push(Derived::Created(key));
    }

    /// A chunk is about to be appended to the derived stream under `key`;
    /// `tail` is what it holds.
    pub(super) fn appended(&mut self, key: Key, tail: Tail) {
        self.derived.push(Derived::Appended(key, tail));
    }

    /// Put `system` back as it was before what this records.
    pub(super) fn undo(self, system: &mut StreamSystem<Live>) {
        for change in self.derived.into_iter().rev() {
            match change {
                Derived::Created(key) => {
                    system.members.remove(key.as_str());
                }
                Derived::Appended(key, tail) => system.members.restore_tail(key.as_str(), tail),
            }
        }
        for (index, node) in self.nodes {
            system.mode.nodes[index] = node;
        }
        for (key, frontier) in self.frontiers.into_iter().rev() {
            system.mode.frontiers.insert(key, frontier);
        }
        for (key, tail) in self.streams.into_iter().rev() {
            system.members.restore_tail(key.as_str(), tail);
        }
    }
}
