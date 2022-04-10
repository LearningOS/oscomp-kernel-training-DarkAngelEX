use core::future::Future;

pub trait AsyncIter<T> {
    type Item<'a>: Future<Output = Option<T>> + 'a
    where
        Self: 'a;
    fn next(&mut self) -> Self::Item<'_>;
}
