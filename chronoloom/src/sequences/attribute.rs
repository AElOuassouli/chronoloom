//! What a timeline describes, and how that description composes.

use core::fmt;

use crate::primitives::Timestamp;

/// What a timeline describes, as the expression that produced it.
///
/// Every [`TimeIntervalSequence`] is named, so it always says which state it is
/// the timeline of. A name is where every attribute starts; the operations
/// build the rest, so a derived timeline carries the expression it came from
/// rather than a name someone had to invent for it.
///
/// ```
/// use chronoloom::primitives::TimeIntervalEvent;
/// use chronoloom::sequences::TimeIntervalSequence;
///
/// let up = TimeIntervalSequence::from_spans("up", vec![TimeIntervalEvent::span(0, 100)?]);
/// let busy = TimeIntervalSequence::from_spans("busy", vec![TimeIntervalEvent::span(10, 20)?]);
///
/// assert_eq!(up.attribute().to_string(), "up");
/// assert_eq!(up.difference(&busy).attribute().to_string(), "up \\ busy");
/// # Ok::<(), chronoloom::primitives::IntervalError>(())
/// ```
///
/// # Rendering
///
/// [`Display`] writes the expression in the notation the documentation uses:
/// `A[alpha, beta]` for a transform, and the set symbols `∪ ∩ \ △` for the
/// operations. Parentheses appear exactly where dropping them would change what
/// the expression means, so a chain of one associative operation stays flat
/// while a mix of two does not.
///
/// ```
/// use chronoloom::sequences::{Attribute, SetOperation};
///
/// let a = || Attribute::name("A");
/// let union = Attribute::combined(SetOperation::Union, a(), Attribute::name("B"));
///
/// // Same operation throughout, so no parentheses are needed.
/// let chain = Attribute::combined(SetOperation::Union, union.clone(), Attribute::name("C"));
/// assert_eq!(chain.to_string(), "A ∪ B ∪ C");
///
/// // A different operation on the outside, so the inner one is grouped.
/// let mixed = Attribute::combined(SetOperation::Intersection, union, Attribute::name("C"));
/// assert_eq!(mixed.to_string(), "(A ∪ B) ∩ C");
/// ```
///
/// [`Display`]: core::fmt::Display
/// [`TimeIntervalSequence`]: crate::sequences::TimeIntervalSequence
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attribute {
    /// A state named outright, the leaf every expression is built from.
    Name(String),
    /// A timeline whose bounds were moved, written `A[alpha, beta]`.
    Transformed {
        /// What was transformed.
        of: Box<Attribute>,
        /// The shift applied to every lower bound.
        alpha: Timestamp,
        /// The shift applied to every upper bound.
        beta: Timestamp,
    },
    /// Two timelines put through one of the set operations.
    Combined {
        /// Which operation combined them.
        operation: SetOperation,
        /// The left operand.
        left: Box<Attribute>,
        /// The right operand.
        right: Box<Attribute>,
    },
}

impl Attribute {
    /// Name a state outright.
    #[must_use]
    pub fn name(name: impl Into<String>) -> Self {
        Self::Name(name.into())
    }

    /// Record that this timeline's bounds were moved by `alpha` and `beta`.
    ///
    /// `A[0, 0]` is `A`, so a zero transform is not recorded at all — there is
    /// nothing for it to say, and saying it anyway would leave two spellings of
    /// the same description.
    #[must_use]
    pub fn transformed(self, alpha: Timestamp, beta: Timestamp) -> Self {
        if (alpha, beta) == (0, 0) {
            return self;
        }

        Self::Transformed {
            of: Box::new(self),
            alpha,
            beta,
        }
    }

    /// Record that two timelines were combined by `operation`.
    #[must_use]
    pub fn combined(operation: SetOperation, left: Self, right: Self) -> Self {
        Self::Combined {
            operation,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    /// Whether this is an operand that needs grouping when it appears inside
    /// `parent`.
    ///
    /// A name and a transform both read as a single unit already — `A[1, 2] ∪ B`
    /// can only be parsed one way — so only a combination is ever at risk. It
    /// keeps its parentheses unless the operation around it is the same
    /// associative one, where the grouping genuinely does not matter.
    fn needs_parentheses_inside(&self, parent: SetOperation) -> bool {
        match self {
            Self::Combined { operation, .. } => !parent.is_associative() || *operation != parent,
            Self::Name(_) | Self::Transformed { .. } => false,
        }
    }

    /// Write `self` as an operand of `parent`, grouping it if it needs it.
    fn write_operand(&self, parent: SetOperation, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.needs_parentheses_inside(parent) {
            write!(f, "({self})")
        } else {
            write!(f, "{self}")
        }
    }
}

impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(name) => f.write_str(name),
            Self::Transformed { of, alpha, beta } => {
                // A transform binds to a single timeline, so a combination
                // under one has to be grouped: `(A ∪ B)[1, 2]` moves both
                // operands, `A ∪ B[1, 2]` only the second.
                if matches!(**of, Self::Combined { .. }) {
                    write!(f, "({of})")?;
                } else {
                    write!(f, "{of}")?;
                }

                write!(f, "[{alpha}, {beta}]")
            }
            Self::Combined {
                operation,
                left,
                right,
            } => {
                left.write_operand(*operation, f)?;
                write!(f, " {} ", operation.symbol())?;
                right.write_operand(*operation, f)
            }
        }
    }
}

/// One of the four ways two timelines combine.
///
/// Names the operation inside an [`Attribute`], and carries the symbol it is
/// written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetOperation {
    /// [`TimeIntervalSequence::union`], written `∪`.
    ///
    /// [`TimeIntervalSequence::union`]: crate::sequences::TimeIntervalSequence::union
    Union,
    /// [`TimeIntervalSequence::intersection`], written `∩`.
    ///
    /// [`TimeIntervalSequence::intersection`]: crate::sequences::TimeIntervalSequence::intersection
    Intersection,
    /// [`TimeIntervalSequence::difference`], written `\`.
    ///
    /// [`TimeIntervalSequence::difference`]: crate::sequences::TimeIntervalSequence::difference
    Difference,
    /// [`TimeIntervalSequence::symmetric_difference`], written `△`.
    ///
    /// [`TimeIntervalSequence::symmetric_difference`]: crate::sequences::TimeIntervalSequence::symmetric_difference
    SymmetricDifference,
}

impl SetOperation {
    /// How the operation is written in a label.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Union => "∪",
            Self::Intersection => "∩",
            Self::Difference => "\\",
            Self::SymmetricDifference => "△",
        }
    }

    /// Whether grouping is free to move: `(A ∘ B) ∘ C` and `A ∘ (B ∘ C)` cover
    /// the same instants.
    ///
    /// True of every operation but the difference, which is the one that reads
    /// its operands asymmetrically. This is what lets a label drop parentheses
    /// it does not need.
    #[must_use]
    pub const fn is_associative(self) -> bool {
        !matches!(self, Self::Difference)
    }
}
