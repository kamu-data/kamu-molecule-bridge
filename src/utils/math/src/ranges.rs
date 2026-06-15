use std::num::NonZeroUsize;
use std::ops::Range;

pub fn sub_ranges(len: usize, batch_size: NonZeroUsize) -> impl IntoIterator<Item = Range<usize>> {
    let batch_size = batch_size.get();
    (0..len)
        .step_by(batch_size)
        .zip(
            (batch_size..len + batch_size)
                .step_by(batch_size)
                .map(move |end| end.min(len)),
        )
        .map(|(start, end)| start..end)
}
