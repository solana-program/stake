use p_stake_interface::pod::{Address, PodI64, PodU32, PodU64};

macro_rules! pod_int_tests {
    ($pod_ty:ty, $prim_ty:ty, $size:expr, $test_value:expr, $mod_name:ident) => {
        mod $mod_name {
            use {
                super::*,
                core::mem::{align_of, needs_drop, size_of},
            };

            #[test]
            fn from_primitive_and_get_roundtrip() {
                let pod = <$pod_ty>::from_primitive($test_value);
                assert_eq!(pod.get(), $test_value);
            }

            #[test]
            fn bytes_are_little_endian() {
                let pod = <$pod_ty>::from_primitive($test_value);
                assert_eq!(pod.0, ($test_value as $prim_ty).to_le_bytes());
                assert_eq!(pod.as_bytes(), &($test_value as $prim_ty).to_le_bytes());
            }

            #[test]
            fn from_trait_impls() {
                let pod: $pod_ty = ($test_value as $prim_ty).into();
                let value: $prim_ty = pod.into();
                assert_eq!(value, $test_value);
            }

            #[test]
            fn set_updates_value() {
                let mut pod = <$pod_ty>::from_primitive(0 as $prim_ty);
                pod.set($test_value);
                assert_eq!(pod.get(), $test_value);
            }

            #[test]
            fn default_is_zero() {
                let pod = <$pod_ty>::default();
                assert_eq!(pod.get(), 0 as $prim_ty);
            }

            #[test]
            fn boundary_values() {
                assert_eq!(
                    <$pod_ty>::from_primitive(<$prim_ty>::MIN).get(),
                    <$prim_ty>::MIN
                );
                assert_eq!(
                    <$pod_ty>::from_primitive(<$prim_ty>::MAX).get(),
                    <$prim_ty>::MAX
                );
            }

            #[test]
            fn layout_properties() {
                assert_eq!(align_of::<$pod_ty>(), 1);
                assert_eq!(size_of::<$pod_ty>(), $size);
                assert!(!needs_drop::<$pod_ty>());
            }

            #[test]
            fn can_be_read_from_misaligned_offset() {
                // Build a buffer with 1 leading byte so the value starts at an odd address.
                let mut buf = [0u8; 1 + $size];
                buf[1..1 + $size].copy_from_slice(&($test_value as $prim_ty).to_le_bytes());

                let ptr = unsafe { buf.as_ptr().add(1) as *const $pod_ty };
                let pod = unsafe { *ptr };
                assert_eq!(pod.get(), $test_value);
            }

            #[test]
            fn as_slice_mut_respects_little_endian_low_byte() {
                let mut pod = <$pod_ty>::from_primitive(0 as $prim_ty);
                let s = pod.as_slice_mut();
                assert_eq!(s.len(), $size);

                // Set only the least significant byte.
                s.fill(0);
                s[0] = 1;
                assert_eq!(pod.get(), 1 as $prim_ty);
            }

            #[test]
            fn const_from_primitive_compiles() {
                const _POD: $pod_ty = <$pod_ty>::from_primitive(0 as $prim_ty);
            }
        }
    };
}

pod_int_tests!(PodU32, u32, 4, 123456789u32, pod_u32);
pod_int_tests!(PodU64, u64, 8, 9876543210u64, pod_u64);
pod_int_tests!(PodI64, i64, 8, -1234567890i64, pod_i64);

// =============================================================================
// Address tests
// =============================================================================

#[test]
fn from_bytes_and_as_bytes_roundtrip() {
    let bytes = [42; 32];
    let addr = Address::new_from_array(bytes);
    assert_eq!(addr.to_bytes(), bytes);
}

#[test]
fn default_is_zero() {
    let addr = Address::default();
    assert_eq!(addr.to_bytes(), [0u8; 32]);
}

#[test]
fn equality() {
    let a = Address::new_from_array([1; 32]);
    let b = Address::new_from_array([1; 32]);
    let c = Address::new_from_array([2; 32]);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn layout_properties() {
    use core::mem::{align_of, needs_drop, size_of};
    assert_eq!(align_of::<Address>(), 1);
    assert_eq!(size_of::<Address>(), 32);
    assert!(!needs_drop::<Address>());
}

#[test]
fn const_from_bytes_compiles() {
    const _A: Address = Address::new_from_array([0u8; 32]);
}
