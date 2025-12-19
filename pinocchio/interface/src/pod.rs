//! Alignment-1 primitives for zero-copy deserialization

// TODO: Document why this is necessary

use solana_pubkey::Pubkey;
use wincode::{SchemaRead, SchemaWrite};

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
pub struct PodU32(pub [u8; 4]);

impl PodU32 {
    #[inline(always)]
    pub fn get(self) -> u32 {
        u32::from_le_bytes(self.0)
    }
    #[inline(always)]
    pub fn set(&mut self, v: u32) {
        self.0 = v.to_le_bytes();
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
pub struct PodU64(pub [u8; 8]);

impl PodU64 {
    #[inline(always)]
    pub fn get(self) -> u64 {
        u64::from_le_bytes(self.0)
    }
    #[inline(always)]
    pub fn set(&mut self, v: u64) {
        self.0 = v.to_le_bytes();
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
pub struct PodI64(pub [u8; 8]);

impl PodI64 {
    #[inline(always)]
    pub fn get(self) -> i64 {
        i64::from_le_bytes(self.0)
    }
    #[inline(always)]
    pub fn set(&mut self, v: i64) {
        self.0 = v.to_le_bytes();
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, SchemaWrite, SchemaRead)]
pub struct PodPubkey(pub [u8; 32]);

impl PodPubkey {
    #[inline(always)]
    pub fn to_pubkey(self) -> Pubkey {
        Pubkey::new_from_array(self.0)
    }
    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    #[inline(always)]
    pub fn from_pubkey(pk: Pubkey) -> Self {
        Self(pk.to_bytes())
    }
}
