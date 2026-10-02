// Regression for RUSTSEC-2024-0429; run with optimizations enabled.
use glib::{prelude::*, Variant};

#[test]
fn every_string_iterator_entry_point_receives_the_mutable_c_out_pointer() {
    let variant = Variant::array_from_iter::<String>(
        ["zero", "one", "two", "three", "four", "five"].map(|value| value.to_variant()),
    );
    let mut iter = variant.array_iter_str().unwrap();
    assert_eq!(iter.next(), Some("zero"));
    assert_eq!(iter.nth(1), Some("two"));
    assert_eq!(iter.next_back(), Some("five"));
    assert_eq!(iter.nth_back(0), Some("four"));
    assert_eq!(iter.last(), Some("three"));
}
