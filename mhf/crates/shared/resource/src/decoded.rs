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

#[cfg(test)]
mod tests {
    use super::Decoded;

    struct Resource {
        bytes: Box<[u8]>,
    }

    impl Resource {
        fn len(&self) -> usize {
            self.bytes.len()
        }
    }

    #[test]
    fn decoded_exposes_owned_fields_and_methods_without_copying() {
        let decoded = Decoded::new(
            "ecd",
            Resource {
                bytes: Box::new([1, 2, 3]),
            },
        );

        assert_eq!(&*decoded.bytes, &[1, 2, 3]);
        assert_eq!(decoded.len(), 3);
        assert!(std::ptr::eq(decoded.as_ref(), &*decoded));
    }

    #[test]
    fn nested_wrappers_deref_to_the_resource() {
        let decoded = Decoded::new(
            "ecd",
            Decoded::new(
                "jkr",
                Resource {
                    bytes: Box::new([5, 8]),
                },
            ),
        );
        let resource: &Resource = &decoded;

        assert_eq!(decoded.encoding, "ecd");
        assert_eq!(decoded.as_ref().encoding, "jkr");
        assert_eq!(decoded.len(), 2);
        assert!(std::ptr::eq(resource, decoded.as_ref().as_ref()));
    }

    #[test]
    fn mapping_moves_each_part_and_preserves_the_other_allocation() {
        struct Encoding {
            name: Box<str>,
        }

        let decoded = Decoded::new(Box::<str>::from("ecd"), Box::<[u8]>::from([2, 4, 8]));
        let encoding_ptr = decoded.encoding.as_ptr();
        let payload_ptr = decoded.as_ref().as_ptr();

        let decoded = decoded.map_inner(|bytes| Resource { bytes });
        assert_eq!(decoded.encoding.as_ptr(), encoding_ptr);
        assert_eq!(decoded.bytes.as_ptr(), payload_ptr);

        let decoded = decoded.map_encoding(|name| Encoding { name });
        assert_eq!(decoded.encoding.name.as_ptr(), encoding_ptr);
        assert_eq!(decoded.bytes.as_ptr(), payload_ptr);

        let resource = decoded.into_inner();
        assert_eq!(resource.bytes.as_ptr(), payload_ptr);
        assert_eq!(&*resource.bytes, &[2, 4, 8]);
    }
}
