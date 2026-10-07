use chronoloom::sequences::{Attribute, SetOperation};

/// A named attribute, for tests where the name is incidental.
fn name(name: &str) -> Attribute {
    Attribute::name(name)
}

/// `left` and `right` combined by `operation`.
fn combine(operation: SetOperation, left: Attribute, right: Attribute) -> Attribute {
    Attribute::combined(operation, left, right)
}

#[test]
fn a_name_renders_as_itself() {
    assert_eq!(name("uptime").to_string(), "uptime");
}

#[test]
fn a_transform_follows_the_name_it_moves() {
    assert_eq!(name("A").transformed(1, 2).to_string(), "A[1, 2]");
}

#[test]
fn negative_shifts_keep_their_sign() {
    assert_eq!(name("A").transformed(-2, 2).to_string(), "A[-2, 2]");
}

#[test]
fn a_zero_transform_is_not_recorded() {
    let alerts = name("alerts");

    assert_eq!(alerts.clone().transformed(0, 0), alerts);
}

#[test]
fn every_operation_has_its_own_symbol() {
    let symbols: Vec<&str> = [
        SetOperation::Union,
        SetOperation::Intersection,
        SetOperation::Difference,
        SetOperation::SymmetricDifference,
    ]
    .into_iter()
    .map(SetOperation::symbol)
    .collect();

    assert_eq!(symbols, ["∪", "∩", "\\", "△"]);
}

#[test]
fn only_the_difference_is_not_associative() {
    assert!(SetOperation::Union.is_associative());
    assert!(SetOperation::Intersection.is_associative());
    assert!(SetOperation::SymmetricDifference.is_associative());
    assert!(!SetOperation::Difference.is_associative());
}

#[test]
fn operands_sit_either_side_of_the_symbol() {
    let either = combine(SetOperation::Union, name("A"), name("B"));

    assert_eq!(either.to_string(), "A ∪ B");
}

#[test]
fn a_transformed_operand_needs_no_grouping() {
    let either = combine(SetOperation::Union, name("A").transformed(1, 2), name("B"));

    assert_eq!(either.to_string(), "A[1, 2] ∪ B");
}

#[test]
fn a_chain_of_one_associative_operation_stays_flat() {
    let union = combine(SetOperation::Union, name("A"), name("B"));
    let chain = combine(SetOperation::Union, union, name("C"));

    assert_eq!(chain.to_string(), "A ∪ B ∪ C");
}

#[test]
fn a_chain_stays_flat_from_either_side() {
    let union = combine(SetOperation::Union, name("B"), name("C"));
    let chain = combine(SetOperation::Union, name("A"), union);

    assert_eq!(chain.to_string(), "A ∪ B ∪ C");
}

#[test]
fn mixing_two_operations_groups_the_inner_one() {
    let union = combine(SetOperation::Union, name("A"), name("B"));
    let mixed = combine(SetOperation::Intersection, union, name("C"));

    assert_eq!(mixed.to_string(), "(A ∪ B) ∩ C");
}

#[test]
fn a_difference_groups_both_of_its_operands() {
    let left = combine(SetOperation::Difference, name("A"), name("B"));
    let right = combine(SetOperation::Union, name("C"), name("D"));
    let nested = combine(SetOperation::Difference, left, right);

    assert_eq!(nested.to_string(), "(A \\ B) \\ (C ∪ D)");
}

#[test]
fn a_transform_groups_the_combination_it_moves() {
    let union = combine(SetOperation::Union, name("A"), name("B"));

    assert_eq!(union.transformed(1, 2).to_string(), "(A ∪ B)[1, 2]");
}

#[test]
fn grouping_a_transformed_combination_survives_another_operation() {
    let union = combine(SetOperation::Union, name("A"), name("B"));
    let mixed = combine(
        SetOperation::Intersection,
        union.transformed(1, 2),
        name("C"),
    );

    assert_eq!(mixed.to_string(), "(A ∪ B)[1, 2] ∩ C");
}
