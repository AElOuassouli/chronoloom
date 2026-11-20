//! The standard-library traits a system implements.

use std::collections::btree_map;

use super::{Key, Member, Sequence, SequenceSystem};

impl<K: Into<Key>, S: Into<Sequence>> FromIterator<(K, S)> for SequenceSystem {
    /// Collect `(key, sequence)` pairs into a system. Where two share a key, the
    /// later one is kept.
    fn from_iter<I: IntoIterator<Item = (K, S)>>(sequences: I) -> Self {
        let mut system = Self::new();
        system.extend(sequences);

        system
    }
}

impl<K: Into<Key>, S: Into<Sequence>> Extend<(K, S)> for SequenceSystem {
    /// Insert every `(key, sequence)` pair. Where a key is already recorded, the
    /// newcomer replaces what was there.
    fn extend<I: IntoIterator<Item = (K, S)>>(&mut self, sequences: I) {
        for (key, sequence) in sequences {
            self.insert(key, sequence);
        }
    }
}

impl<'a> IntoIterator for &'a SequenceSystem {
    type Item = (&'a Key, &'a Sequence);
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for SequenceSystem {
    type Item = (Key, Sequence);
    type IntoIter = IntoIter;

    /// Consume the system, yielding each sequence with its key, in key order.
    /// The metadata is dropped.
    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self.members.into_iter())
    }
}

/// Walks a system's sequences with their keys, in key order, by reference.
#[derive(Debug, Clone)]
pub struct Iter<'a>(pub(super) btree_map::Iter<'a, Key, Member>);

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a Key, &'a Sequence);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(key, member)| (key, &member.sequence))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for Iter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0
            .next_back()
            .map(|(key, member)| (key, &member.sequence))
    }
}

impl ExactSizeIterator for Iter<'_> {}

/// Walks a system's sequences with their keys, in key order, by value.
#[derive(Debug)]
pub struct IntoIter(btree_map::IntoIter<Key, Member>);

impl Iterator for IntoIter {
    type Item = (Key, Sequence);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(key, member)| (key, member.sequence))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for IntoIter {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0
            .next_back()
            .map(|(key, member)| (key, member.sequence))
    }
}

impl ExactSizeIterator for IntoIter {}
