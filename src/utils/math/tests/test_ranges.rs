use std::num::NonZeroUsize;
use std::ops::Range;

use math::ranges::sub_ranges;
use pretty_assertions::assert_eq;

#[rstest::rstest]
#[case(0, 1, vec![])]
#[case(0, 100, vec![])]
#[case(1, 1, vec![0..1])]
#[case(1, 10, vec![0..1])]
#[case(3, 1, vec![0..1, 1..2, 2..3])]
#[case(5, 4, vec![0..4, 4..5])]
#[case(5, 5, vec![0..5])]
#[case(5, 6, vec![0..5])]
#[case(7, 2, vec![0..2, 2..4, 4..6, 6..7])]
#[case(9, 3, vec![0..3, 3..6, 6..9])]
#[case(10, 3, vec![0..3, 3..6, 6..9, 9..10])]
fn sub_ranges_returns_expected_batches(
    #[case] len: usize,
    #[case] batch_size: usize,
    #[case] expected: Vec<Range<usize>>,
) {
    let batch_size = NonZeroUsize::new(batch_size).unwrap();
    let actual = sub_ranges(len, batch_size).into_iter().collect::<Vec<_>>();

    assert_eq!(expected, actual);
}
