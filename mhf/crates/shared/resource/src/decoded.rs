//! Owned decoded values keep their encoding metadata alongside the payload.

use std::ops::Deref;

#[derive(Clone, Debug)]
pub struct Decoded<H, T> {
    pub encoding: H,
    inner: T,
}

impl<H, T> Decoded<H, T> {
    pub fn new(encoding: H, inner: T) -> Self {
        Self { encoding, inner }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }

    pub fn map_inner<U>(self, map: impl FnOnce(T) -> U) -> Decoded<H, U> {
        Decoded::new(self.encoding, map(self.inner))
    }

    pub fn map_encoding<K>(self, map: impl FnOnce(H) -> K) -> Decoded<K, T> {
        Decoded::new(map(self.encoding), self.inner)
    }
}

impl<H, T> Deref for Decoded<H, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<H, T> AsRef<T> for Decoded<H, T> {
    fn as_ref(&self) -> &T {
        &self.inner
    }
}
