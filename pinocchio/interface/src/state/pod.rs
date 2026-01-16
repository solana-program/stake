//! Alignment-1 ("pod") primitives for zero-copy deserialization.
//!
//! Solana account data is a raw `&[u8]` with no alignment guarantees.
//! Standard Rust primitives like `u64` require 8-byte alignment, so you can't
//! safely cast `&[u8]` to `&u64` without risking undefined behavior.
//!
//! These "pod" (plain old data) types wrap byte arrays and provide safe
//! get/set methods that handle little-endian conversion. They have alignment 1,
//! so they can be safely referenced from any byte offset.
use wincode::{SchemaRead, SchemaWrite};

/// Macro to define an alignment-1 little-endian integer wrapper.
#[macro_export]
macro_rules! impl_pod_int {
    (
        $(#[$meta:meta])*
        $name:ident, $prim:ty, $n:expr
    ) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Default, SchemaWrite, SchemaRead)]
        #[wincode(assert_zero_copy)]
        pub struct $name(pub [u8; $n]);

        impl $name {
            #[inline(always)]
            pub const fn from_primitive(v: $prim) -> Self {
                Self(v.to_le_bytes())
            }

            #[inline(always)]
            pub const fn get(self) -> $prim {
                <$prim>::from_le_bytes(self.0)
            }

            pub fn set(&mut self, v: $prim) {
                self.0 = v.to_le_bytes();
            }

            #[inline(always)]
            pub fn as_bytes(&self) -> &[u8; $n] {
                &self.0
            }

            #[inline(always)]
            pub fn as_slice_mut(&mut self) -> &mut [u8] {
                &mut self.0
            }
        }

        impl From<$prim> for $name {
            fn from(v: $prim) -> Self {
                Self::from_primitive(v)
            }
        }

        impl From<$name> for $prim {
            fn from(v: $name) -> Self {
                v.get()
            }
        }
    };
}

impl_pod_int!(PodU32, u32, 4);
impl_pod_int!(PodU64, u64, 8);
impl_pod_int!(PodI64, i64, 8);

/// An `Address` stored as 32 bytes with alignment 1.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
pub struct PodAddress(pub [u8; 32]);

impl PodAddress {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}
