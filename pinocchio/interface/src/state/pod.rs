//! Alignment-1 ("pod") primitives for zero-copy deserialization.
//!
//! # Why alignment-1?
//!
//! Solana account data is a raw `&[u8]` with no alignment guarantees.
//! Standard Rust primitives like `u64` require 8-byte alignment, so you can't
//! safely cast `&[u8]` to `&u64` without risking undefined behavior.
//!
//! These "pod" (plain old data) types wrap byte arrays and provide safe
//! get/set methods that handle little-endian conversion. They have alignment 1,
//! so they can be safely referenced from any byte offset.
//!
//! # Layout
//!
//! All types use `#[repr(transparent)]` to ensure the struct has the same
//! memory layout as its inner byte array.

use {
    solana_address::Address,
    wincode::{SchemaRead, SchemaWrite},
};

/// Macro to define an alignment-1 little-endian integer wrapper.
///
/// Generates:
/// - `#[repr(transparent)]` wrapper over `[u8; N]`
/// - `Default` (all zeros)
/// - `const fn from_primitive`
/// - `get/set/as_bytes`
/// - `From<prim> for Pod*` and `From<Pod*> for prim`
#[macro_export]
macro_rules! impl_pod_int_le {
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
            /// Creates from a native primitive value, stored as little-endian bytes.
            #[inline(always)]
            pub const fn from_primitive(v: $prim) -> Self {
                Self(v.to_le_bytes())
            }

            /// Reads the value as a native primitive.
            #[inline(always)]
            pub fn get(self) -> $prim {
                <$prim>::from_le_bytes(self.0)
            }

            /// Writes a native primitive value.
            #[inline(always)]
            pub fn set(&mut self, v: $prim) {
                self.0 = v.to_le_bytes();
            }

            /// Returns the raw little-endian bytes.
            #[inline(always)]
            pub fn as_bytes(&self) -> &[u8; $n] {
                &self.0
            }

            /// Returns the raw little-endian bytes as a mutable slice.
            #[inline(always)]
            pub fn as_mut_slice(&mut self) -> &mut [u8] {
                &mut self.0
            }
        }

        impl From<$prim> for $name {
            #[inline(always)]
            fn from(v: $prim) -> Self {
                Self::from_primitive(v)
            }
        }

        impl From<$name> for $prim {
            #[inline(always)]
            fn from(v: $name) -> Self {
                v.get()
            }
        }
    };
}

impl_pod_int_le!(
    /// A `u32` stored as 4 little-endian bytes with alignment 1.
    PodU32, u32, 4
);

impl_pod_int_le!(
    /// A `u64` stored as 8 little-endian bytes with alignment 1.
    PodU64, u64, 8
);

impl_pod_int_le!(
    /// An `i64` stored as 8 little-endian bytes with alignment 1.
    PodI64, i64, 8
);

/// An `Address` (pubkey) stored as 32 bytes with alignment 1.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, SchemaWrite, SchemaRead)]
#[wincode(assert_zero_copy)]
pub struct PodAddress(pub [u8; 32]);

impl PodAddress {
    /// Converts to a native `Address`.
    #[inline(always)]
    pub fn to_address(self) -> Address {
        Address::new_from_array(self.0)
    }

    /// Returns the raw bytes.
    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Creates from raw bytes.
    #[inline(always)]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Creates from a native `Address`.
    #[inline(always)]
    pub fn from_address(addr: Address) -> Self {
        Self(addr.to_bytes())
    }
}

impl From<Address> for PodAddress {
    #[inline(always)]
    fn from(addr: Address) -> Self {
        Self::from_address(addr)
    }
}

impl From<PodAddress> for Address {
    #[inline(always)]
    fn from(p: PodAddress) -> Self {
        p.to_address()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pod_u32_le_layout() {
        let p = PodU32::from_primitive(1);
        assert_eq!(p.0, [1, 0, 0, 0]);
        assert_eq!(p.get(), 1);

        let x: u32 = p.into();
        assert_eq!(x, 1);

        let p2: PodU32 = 42u32.into();
        assert_eq!(p2.get(), 42);
    }

    #[test]
    fn pod_u64_le_layout() {
        let p = PodU64::from_primitive(1);
        assert_eq!(p.0, [1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(p.get(), 1);

        let x: u64 = p.into();
        assert_eq!(x, 1);

        let mut p2: PodU64 = 0u64.into();
        p2.set(u64::MAX);
        assert_eq!(p2.get(), u64::MAX);
    }

    #[test]
    fn pod_i64_le_layout() {
        let p = PodI64::from_primitive(-1);
        assert_eq!(p.get(), -1);

        let x: i64 = p.into();
        assert_eq!(x, -1);

        let mut p2: PodI64 = 0i64.into();
        p2.set(i64::MIN);
        assert_eq!(p2.get(), i64::MIN);
    }

    #[test]
    fn pod_address_roundtrip() {
        let bytes = [7u8; 32];
        let p = PodAddress::from_bytes(bytes);
        assert_eq!(p.as_bytes(), &bytes);

        let addr = p.to_address();
        let p2 = PodAddress::from_address(addr);
        assert_eq!(p2.as_bytes(), &bytes);

        let addr2: Address = p2.into();
        let p3: PodAddress = addr2.into();
        assert_eq!(p3.as_bytes(), &bytes);
    }
}
