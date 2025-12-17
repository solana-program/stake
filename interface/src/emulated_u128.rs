use core::cmp::Ordering;

#[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
pub struct U128 {
    pub hi: u64,
    pub lo: u64,
}

impl U128 {
    pub const ZERO: Self = Self { hi: 0, lo: 0 };
    pub const MAX: Self = Self {
        hi: u64::MAX,
        lo: u64::MAX,
    };

    pub const fn from_u64(x: u64) -> Self {
        Self { hi: 0, lo: x }
    }

    pub const fn is_zero(self) -> bool {
        self.hi == 0 && self.lo == 0
    }

    pub fn mul_u64(lhs: u64, rhs: u64) -> Self {
        mul_u64_wide(lhs, rhs)
    }

    pub fn checked_mul_u64(self, rhs: u64) -> Option<Self> {
        if rhs == 0 || self.is_zero() {
            return Some(Self::ZERO);
        }

        // self * rhs = (self.lo * rhs) + (self.hi * rhs) << 64
        let lo_prod = mul_u64_wide(self.lo, rhs); // 128-bit
        let hi_prod = mul_u64_wide(self.hi, rhs); // 128-bit

        // Shifting hi_prod left by 64 would discard hi_prod.hi beyond 128 -> overflow
        if hi_prod.hi != 0 {
            return None;
        }

        // new_hi = lo_prod.hi + hi_prod.lo
        // overflow => exceed 128 bits
        let (new_hi, overflow) = lo_prod.hi.overflowing_add(hi_prod.lo);
        if overflow {
            return None;
        }

        Some(Self {
            hi: new_hi,
            lo: lo_prod.lo,
        })
    }

    pub fn saturating_mul_u64(self, rhs: u64) -> Self {
        self.checked_mul_u64(rhs).unwrap_or(Self::MAX)
    }

    /// Some magic copied from the internet
    pub fn div_floor_u64_clamped(numer: Self, denom: Self, clamp: u64) -> u64 {
        if clamp == 0 || numer.is_zero() || denom.is_zero() {
            return 0;
        }
        if numer < denom {
            return 0;
        }

        if numer.hi == 0 && denom.hi == 0 {
            return core::cmp::min(numer.lo / denom.lo, clamp);
        }

        // Fast path: if denom*clamp <= numer, clamp
        if let Some(prod) = denom.checked_mul_u64(clamp) {
            if prod <= numer {
                return clamp;
            }
        }

        let mut lo: u64 = 0;
        let mut hi: u64 = clamp;

        for _ in 0..64 {
            if lo == hi {
                break;
            }

            let diff = hi - lo;
            let mid = lo + (diff / 2) + (diff & 1);

            let ok = match denom.checked_mul_u64(mid) {
                Some(prod) => prod <= numer,
                None => false,
            };

            if ok {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }

        lo
    }
}

impl Ord for U128 {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.hi.cmp(&other.hi) {
            Ordering::Equal => self.lo.cmp(&other.lo),
            ord => ord,
        }
    }
}

impl PartialOrd for U128 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// In order to multiply a u64*u64 you need to use 32-bit halves
fn mul_u64_wide(a: u64, b: u64) -> U128 {
    const MASK32: u64 = 0xFFFF_FFFF;

    let a0 = a & MASK32;
    let a1 = a >> 32;
    let b0 = b & MASK32;
    let b1 = b >> 32;

    let w0 = a0 * b0; // 64-bit
    let t = a1 * b0 + (w0 >> 32); // fits in u64
    let w1 = t & MASK32;
    let w2 = t >> 32;

    let t = a0 * b1 + w1; // fits in u64
    let lo = (t << 32) | (w0 & MASK32);
    let hi = a1 * b1 + w2 + (t >> 32);

    U128 { hi, lo }
}
