use std::ffi::c_void;

#[derive(Copy, Clone)]
pub struct Value(u64);

impl Value {
    // ------------------------------------------------------------------------
    // Constants & Masks
    // ------------------------------------------------------------------------
    
    // We use the negative Quiet NaN space for our tagged values.
    // Sign bit: 1, Exponent: all 1s, QNaN bit: 1.
    // This provides a base of 0xFFF8_0000_0000_0000 for our tags.
    const TAGGED_BASE: u64 = 0xFFF0000000000000;
    const TAG_SHIFT: u64 = 48;
    const PAYLOAD_MASK: u64 = 0x0000FFFFFFFFFFFF;
    
    // Type Tags (Bits 48-51)
    // To ensure they fall within the QNaN space, tags start at 0x8 (setting bit 51 to 1).
    const TAG_INT: u64  = 0x8;
    const TAG_BOOL: u64 = 0x9;
    const TAG_NULL: u64 = 0xA;
    const TAG_PTR: u64  = 0xB;

    // ------------------------------------------------------------------------
    // Internal Helper
    // ------------------------------------------------------------------------
    
    #[inline(always)]
    fn tag(tag: u64, payload: u64) -> u64 {
        Self::TAGGED_BASE | (tag << Self::TAG_SHIFT) | payload
    }

    // ------------------------------------------------------------------------
    // Constructors
    // ------------------------------------------------------------------------

    #[inline]
    pub fn new_float(f: f64) -> Self {
        let bits = f.to_bits();
        // Edge case: if a hardware operation genuinely produces a negative NaN
        // that collides with our tag space, normalize it to a positive QNaN.
        if bits >= 0xFFF8000000000000 {
            Self(0x7FF8000000000000)
        } else {
            Self(bits)
        }
    }

    #[inline]
    pub fn new_int(i: i32) -> Self {
        Self(Self::tag(Self::TAG_INT, (i as u32) as u64))
    }

    #[inline]
    pub fn new_bool(b: bool) -> Self {
        Self(Self::tag(Self::TAG_BOOL, b as u64))
    }

    #[inline]
    pub fn new_null() -> Self {
        Self(Self::tag(Self::TAG_NULL, 0))
    }

    #[inline]
    pub fn new_ptr(p: *mut c_void) -> Self {
        // Bridge through usize to ensure we can cast between pointer and u64 
        // across both 32-bit (wasm32) and 64-bit (x86_64/aarch64) architectures.
        let ptr_val = p as usize as u64;
        
        // Critical: On AArch64, the top 16 bits may contain Pointer Authentication Codes (PAC).
        // By masking them off, we strip the hardware signature. We add a debug_assert
        // to catch environments where pointers exceed the 48-bit address space.
        debug_assert!(
            ptr_val <= Self::PAYLOAD_MASK, 
            "Pointer exceeds 48-bit payload space (Possible PAC signature detected)"
        );
        
        Self(Self::tag(Self::TAG_PTR, ptr_val & Self::PAYLOAD_MASK))
    }

    // ------------------------------------------------------------------------
    // Type Checks
    // ------------------------------------------------------------------------

    #[inline]
    pub fn is_float(&self) -> bool {
        // Any value less than our tagged space base is a valid float.
        // This naturally includes positive NaNs, infinities, and all numbers.
        self.0 < 0xFFF8000000000000
    }

    #[inline]
    pub fn is_int(&self) -> bool {
        (self.0 & !Self::PAYLOAD_MASK) == Self::tag(Self::TAG_INT, 0)
    }

    #[inline]
    pub fn is_bool(&self) -> bool {
        (self.0 & !Self::PAYLOAD_MASK) == Self::tag(Self::TAG_BOOL, 0)
    }

    #[inline]
    pub fn is_null(&self) -> bool {
        (self.0 & !Self::PAYLOAD_MASK) == Self::tag(Self::TAG_NULL, 0)
    }

    #[inline]
    pub fn is_ptr(&self) -> bool {
        (self.0 & !Self::PAYLOAD_MASK) == Self::tag(Self::TAG_PTR, 0)
    }

    // ------------------------------------------------------------------------
    // Extractions
    // ------------------------------------------------------------------------

    #[inline]
    pub fn as_float(&self) -> Option<f64> {
        if self.is_float() {
            Some(f64::from_bits(self.0))
        } else {
            None
        }
    }

    #[inline]
    pub fn as_int(&self) -> Option<i32> {
        if self.is_int() {
            // Casting directly truncates correctly and preserves 2's complement
            Some(self.0 as i32)
        } else {
            None
        }
    }

    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        if self.is_bool() {
            Some((self.0 & Self::PAYLOAD_MASK) != 0)
        } else {
            None
        }
    }

    #[inline]
    pub fn as_ptr(&self) -> Option<*mut c_void> {
        if self.is_ptr() {
            // Bridge through usize for safe cross-platform pointer reconstruction.
            // On AArch64, if we were stripped of PAC bits, this returns a "raw" 
            // address which may fail dereference if the OS enforces PAC.
            Some((self.0 & Self::PAYLOAD_MASK) as usize as *mut c_void)
        } else {
            None
        }
    }
}

// ------------------------------------------------------------------------
// Standard Traits
// ------------------------------------------------------------------------

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        if self.is_float() && other.is_float() {
            // Respect IEEE 754 semantics: NaN != NaN, 0.0 == -0.0
            f64::from_bits(self.0) == f64::from_bits(other.0)
        } else {
            self.0 == other.0
        }
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_float() {
            write!(f, "{:?}", f64::from_bits(self.0))
        } else if self.is_int() {
            write!(f, "Int({:?})", self.as_int().unwrap())
        } else if self.is_bool() {
            write!(f, "Bool({:?})", self.as_bool().unwrap())
        } else if self.is_null() {
            write!(f, "Null")
        } else if self.is_ptr() {
            write!(f, "Pointer({:p})", self.as_ptr().unwrap())
        } else {
            write!(f, "Unknown(0x{:016X})", self.0)
        }
    }
}

// ------------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_float() {
        let v = Value::new_float(42.5);
        assert!(v.is_float());
        assert!(!v.is_int());
        assert_eq!(v.as_float(), Some(42.5));
    }

    #[test]
    fn test_int() {
        let v = Value::new_int(-42);
        assert!(v.is_int());
        assert!(!v.is_float());
        assert_eq!(v.as_int(), Some(-42));

        let v2 = Value::new_int(i32::MAX);
        assert_eq!(v2.as_int(), Some(i32::MAX));
        
        let v3 = Value::new_int(i32::MIN);
        assert_eq!(v3.as_int(), Some(i32::MIN));
    }

    #[test]
    fn test_bool() {
        let t = Value::new_bool(true);
        let f = Value::new_bool(false);
        assert!(t.is_bool());
        assert!(f.is_bool());
        assert_eq!(t.as_bool(), Some(true));
        assert_eq!(f.as_bool(), Some(false));
    }

    #[test]
    fn test_null() {
        let n = Value::new_null();
        assert!(n.is_null());
        assert!(!n.is_float());
    }

    #[test]
    fn test_ptr() {
        let mut data = 42u64;
        let ptr = &mut data as *mut _ as *mut c_void;
        let v = Value::new_ptr(ptr);
        assert!(v.is_ptr());
        assert_eq!(v.as_ptr(), Some(ptr));
    }

    #[test]
    fn test_nan_normalization() {
        // Construct a negative NaN that deliberately collides with our tag space
        let negative_nan_bits: u64 = 0xFFF8000000000001;
        let nan_float = f64::from_bits(negative_nan_bits);
        let v = Value::new_float(nan_float);
        
        // It should still be a float (but cleanly normalized)
        assert!(v.is_float());
        assert!(v.as_float().unwrap().is_nan());
        assert_eq!(v.0, 0x7FF8000000000000);
    }
    
    #[test]
    fn test_infinity() {
        let pos_inf = Value::new_float(f64::INFINITY);
        assert!(pos_inf.is_float());
        assert_eq!(pos_inf.as_float(), Some(f64::INFINITY));

        let neg_inf = Value::new_float(f64::NEG_INFINITY);
        assert!(neg_inf.is_float());
        assert_eq!(neg_inf.as_float(), Some(f64::NEG_INFINITY));
    }
    
    #[test]
    fn test_partial_eq() {
        assert_eq!(Value::new_int(42), Value::new_int(42));
        assert_ne!(Value::new_int(42), Value::new_int(43));
        assert_eq!(Value::new_float(0.0), Value::new_float(-0.0));
        assert_ne!(Value::new_float(f64::NAN), Value::new_float(f64::NAN)); // NaN != NaN
    }
}
