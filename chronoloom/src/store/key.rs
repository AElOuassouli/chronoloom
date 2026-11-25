//! The identity of a stream.

use core::borrow::Borrow;
use core::fmt;

/// What identifies a sequence inside a [`StreamSystem`].
///
/// A key is the sequence's identity and its attribute is its description. The
/// two are kept apart because they behave differently: an identity is stable
/// and unique within a system (a sensor's uuid, say), while a description may
/// change over time and be shared by several members.
///
/// A key is a string with a name of its own, so the one is never mistaken for
/// the other. Anything that is a `&str` or a `String` converts into one, and a
/// system's methods look members up by `&str`.
///
/// ```
/// use chronoloom::Key;
///
/// let key = Key::from("sensor-3");
/// assert_eq!(key.as_str(), "sensor-3");
/// assert_eq!(key.to_string(), "sensor-3");
/// ```
///
/// [`StreamSystem`]: crate::StreamSystem
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Key(String);

impl Key {
    /// The key as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Key {
    fn from(key: &str) -> Self {
        Self(key.to_owned())
    }
}

impl From<String> for Key {
    fn from(key: String) -> Self {
        Self(key)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Key {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for Key {
    /// Lets a map keyed by `Key` be searched with a `&str`, without allocating.
    /// Sound because `Key` orders and hashes exactly as its string does.
    fn borrow(&self) -> &str {
        &self.0
    }
}
