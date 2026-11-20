//! What a change to a stream system did.

use std::collections::BTreeSet;

use crate::sequence_system::Key;

/// What a change to a [`StreamSystem`] did to its derived streams.
///
/// [`StreamSystem`]: super::StreamSystem
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub(super) changed: BTreeSet<Key>,
}

impl Report {
    /// The derived streams whose content changed, appeared or disappeared.
    #[must_use]
    pub const fn changed(&self) -> &BTreeSet<Key> {
        &self.changed
    }

    /// Whether no derived stream changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty()
    }
}
