//! The standard-library traits a system implements.

use std::collections::btree_map;

use super::{Sequence, SequenceSystem};

impl<S: Into<Sequence>> FromIterator<S> for SequenceSystem {
    /// Collect sequences into a system. Where two share an attribute, the later
    /// one is kept.
    fn from_iter<I: IntoIterator<Item = S>>(sequences: I) -> Self {
        let mut system = Self::new();
        system.extend(sequences);

        system
    }
}

impl<S: Into<Sequence>> Extend<S> for SequenceSystem {
    /// Insert every sequence. Where an attribute is already recorded, the
    /// newcomer replaces what was there.
    fn extend<I: IntoIterator<Item = S>>(&mut self, sequences: I) {
        for sequence in sequences {
            self.insert(sequence);
        }
    }
}

impl<'a> IntoIterator for &'a SequenceSystem {
    type Item = (&'a String, &'a Sequence);
    type IntoIter = btree_map::Iter<'a, String, Sequence>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for SequenceSystem {
    type Item = (String, Sequence);
    type IntoIter = btree_map::IntoIter<String, Sequence>;

    /// Consume the system, yielding each sequence with its attribute, in attribute order.
    fn into_iter(self) -> Self::IntoIter {
        self.sequences.into_iter()
    }
}
