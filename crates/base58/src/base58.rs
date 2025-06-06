use core::{
    borrow::Borrow,
    cmp::{Ord, Ordering, PartialEq, PartialOrd},
    ffi::CStr,
    fmt,
    hash::{Hash, Hasher},
    ops::Deref,
    result::Result,
    str::{self, FromStr},
};

use buggy::{Bug, BugExt};

use crate::arith::{div_ww, mul_add_ww};

const ALPHABET: [u8; 58] = [
    b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'A', b'B', b'C', b'D', b'E', b'F', b'G',
    b'H', b'J', b'K', b'L', b'M', b'N', b'P', b'Q', b'R', b'S', b'T', b'U', b'V', b'W', b'X', b'Y',
    b'Z', b'a', b'b', b'c', b'd', b'e', b'f', b'g', b'h', b'i', b'j', b'k', b'm', b'n', b'o', b'p',
    b'q', b'r', b's', b't', b'u', b'v', b'w', b'x', b'y', b'z',
];

/// 58^i
const RADII: [u64; 11] = [
    0,
    58,
    58 * 58,
    58 * 58 * 58,
    58 * 58 * 58 * 58,
    58 * 58 * 58 * 58 * 58,
    58 * 58 * 58 * 58 * 58 * 58,
    58 * 58 * 58 * 58 * 58 * 58 * 58,
    58 * 58 * 58 * 58 * 58 * 58 * 58 * 58,
    58 * 58 * 58 * 58 * 58 * 58 * 58 * 58 * 58,
    58 * 58 * 58 * 58 * 58 * 58 * 58 * 58 * 58 * 58,
];

const B58: [u8; 256] = [
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 1, 2, 3, 4, 5, 6, 7, 8, 255, 255,
    255, 255, 255, 255, 255, 9, 10, 11, 12, 13, 14, 15, 16, 255, 17, 18, 19, 20, 21, 255, 22, 23,
    24, 25, 26, 27, 28, 29, 30, 31, 32, 255, 255, 255, 255, 255, 255, 33, 34, 35, 36, 37, 38, 39,
    40, 41, 42, 43, 255, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
];

/// Implemented by types that can encode themselves as Base58.
pub trait ToBase58 {
    /// A Base58 string.
    type Output: Borrow<str>;

    /// Encodes itself as a Base58 string.
    fn to_base58(&self) -> Self::Output;
}

/// The Base58 could not be decoded.
#[derive(Clone, Debug)]
pub enum DecodeError {
    /// The input is not valid base58.
    BadInput,
    /// An internal bug occurred.
    Bug(Bug),
}

impl From<Bug> for DecodeError {
    fn from(bug: Bug) -> Self {
        DecodeError::Bug(bug)
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadInput => write!(f, "bad input"),
            Self::Bug(bug) => write!(f, "{bug}"),
        }
    }
}

impl core::error::Error for DecodeError {}

// Generate PartialEq for $lhs and $rhs.
macro_rules! impl_eq {
    ($lhs:ty, $rhs:ty) => {
        #[allow(unused_lifetimes)]
        impl<'a, 'b> PartialEq<$rhs> for $lhs {
            #[inline]
            fn eq(&self, other: &$rhs) -> bool {
                PartialEq::eq(&self[..], &other[..])
            }
        }

        #[allow(unused_lifetimes)]
        impl<'a, 'b> PartialEq<$lhs> for $rhs {
            #[inline]
            fn eq(&self, other: &$lhs) -> bool {
                PartialEq::eq(&self[..], &other[..])
            }
        }
    };
}

// Generate fixed-size encodings.
macro_rules! encode_x {
    ($($name:ident => $size:expr),+ $(,)?) => {
        $(
            #[doc = concat!("A Base58-encoded ", stringify!($size), "-byte value.")]
            #[derive(Copy, Clone)]
            pub struct $name {
                data: [u8; Self::BUFFER_SIZE],
            }

            impl $name {
                /// The size in bytes of the encoded value.
                pub const B58_SIZE: usize = ($size * 1375) / 1000;

                /// The size in bytes of the encoded value, with a null terminator.
                pub const BUFFER_SIZE: usize = Self::B58_SIZE + 1;
            }

            impl fmt::Display for $name {
                #[inline]
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    fmt::Display::fmt(self.as_str(), f)
                }
            }

            impl fmt::Debug for $name {
                #[inline]
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    fmt::Debug::fmt(self.as_str(), f)
                }
            }

            impl AsRef<[u8]> for $name {
                #[inline]
                fn as_ref(&self) -> &[u8] {
                    self.as_bytes()
                }
            }

            impl AsRef<str> for $name {
                #[inline]
                fn as_ref(&self) -> &str {
                    self.as_str()
                }
            }

            impl AsRef<CStr> for $name {
                #[inline]
                fn as_ref(&self) -> &CStr {
                    self.as_cstr()
                }
            }

            impl Borrow<str> for $name {
                #[inline]
                fn borrow(&self) -> &str {
                    self.as_str()
                }
            }

            impl Deref for $name {
                type Target = str;

                #[inline]
                fn deref(&self) -> &str {
                    self.as_str()
                }
            }

            impl TryFrom<&str> for $name {
                type Error = DecodeError;

                #[inline]
                fn try_from(s: &str) -> Result<Self, DecodeError> {
                    Self::from_str(s)
                }
            }

            impl TryFrom<$name> for [u8; $size] {
                type Error = DecodeError;

                #[inline]
                fn try_from(s: $name) -> Result<Self, DecodeError> {
                    $name::decode(&s)
                }
            }

            impl FromStr for $name {
                type Err = DecodeError;

                fn from_str(s: &str) -> Result<Self, DecodeError> {
                    let b = s.as_bytes();
                    Self::decode(b)?;
                    // It's valid Base58, so just copy it over.
                    let mut v = Self::default();
                    let start = Self::B58_SIZE.checked_sub(b.len()).assume("decode ensures it will fit")?;
                    v.data[start..Self::B58_SIZE].copy_from_slice(b);
                    Ok(v)
                }
            }

            impl Default for $name {
                #[inline]
                fn default() -> Self  {
                    let mut this = Self {
                        data: [b'1'; Self::BUFFER_SIZE],
                    };
                    this.data[Self::BUFFER_SIZE - 1] = 0;
                    this
                }
            }

            impl Eq for $name {}
            impl PartialEq for $name {
                #[inline]
                fn eq(&self, other: &Self) -> bool {
                    PartialEq::eq(&self[..], &other[..])
                }
            }
            impl_eq!($name, str);
            impl_eq!($name, &'a str);
            #[cfg(feature = "alloc")]
            #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
            impl_eq!(alloc::borrow::Cow<'a, str>, $name);
            #[cfg(feature = "alloc")]
            #[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
            impl_eq!($name, alloc::string::String);

            impl Ord for $name {
                #[inline]
                fn cmp(&self, other: &Self) -> Ordering {
                    Ord::cmp(&self[..], &other[..])
                }
            }

            impl PartialOrd for $name {
                #[inline]
                fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                    Some(self.cmp(other))
                }
            }

            impl Hash for $name {
                #[inline]
                fn hash<H: Hasher>(&self, hasher: &mut H) {
                    (**self).hash(hasher)
                }
            }

            impl ToBase58 for [u8; $size] {
                type Output = $name;

                fn to_base58(&self) -> Self::Output {
                    $name::encode(self)
                }
            }

            impl $name {
                /// Returns a string slice of the string's contents.
                #[inline]
                pub fn as_str(&self) -> &str {
                    str::from_utf8(self.as_bytes())
                        .expect("should be valid UTF-8")
                }

                /// Returns a byte slice of the string's contents.
                #[inline]
                pub fn as_bytes(&self) -> &[u8; Self::B58_SIZE] {
                    self.data[..Self::B58_SIZE].try_into().expect("array size matches slice")
                }

                /// Returns a null terminated cstr of the string's contents.
                #[inline]
                pub fn as_cstr(&self) -> &CStr {
                    CStr::from_bytes_with_nul(&self.data).expect("should be valid C string")
                }

                /// Decodes `s` as bytes.
                pub fn decode<T: AsRef<[u8]>>(s: T) -> Result<[u8; $size], DecodeError> {
                    let mut x = Uint::<{ $size/8 }, $size>::new();
                    // Work 10 bytes at a time (see `RADIX`, etc).
                    for chunk in s.as_ref().chunks(10) {
                        let total = chunk
                            .iter()
                            .map(|c| B58[*c as usize])
                            .try_fold(0, |acc: u64, v| {
                                if v == 255 {
                                    Err(DecodeError::BadInput)
                                } else {
                                    Ok(acc.checked_mul(58).assume("doesn't wrap")?.checked_add(u64::from(v)).assume("doesn't wrap")?)
                                }
                            })?;
                        if !x.fma(RADII[chunk.len()], total) {
                            return Err(DecodeError::BadInput);
                        }
                    }
                    Ok(x.to_be_bytes())
                }

                /// Encodes `b` as a Base58 string.
                pub fn encode(b: &[u8; $size]) -> Self {
                    let mut dst = Self::default();

                    let mut x = Uint::<{ $size/8 }, $size>::from_be_bytes(b);

                    let mut i = Self::B58_SIZE;
                    while !x.is_zero() {
                        let mut r = x.quo_radix();
                        if x.is_zero() {
                            while r > 0 {
                                i = i.checked_sub(1).expect("i must be non-zero");
                                dst.data[i] = ALPHABET[(r % 58) as usize];
                                r /= 58;
                            }
                        } else {
                            for _ in 0..10 {
                                i = i.checked_sub(1).expect("i must be non-zero");
                                dst.data[i] = ALPHABET[(r % 58) as usize];
                                r /= 58;
                            }
                        }
                    }

                    dst
                }
            }
        )+
    };
}
encode_x! {
    String16 => 16,
    String32 => 32,
    String64 => 64,
}

/// An unsigned N-bit integer.
///
/// - `W` is the number of words
/// - `B` is the number of bytes (`B = W*8`)
///
/// This should just be `N`, but `const_generic_exprs` is still
/// +nightly.
#[derive(Debug, Eq, PartialEq)]
struct Uint<const W: usize, const B: usize> {
    words: [u64; W],
}

impl<const W: usize, const B: usize> Uint<W, B> {
    fn new() -> Self {
        Self { words: [0u64; W] }
    }

    /// Decodes an integer from big-endian format.
    fn from_be_bytes(b: &[u8; B]) -> Self {
        let mut z = [0u64; W];
        for (bytes, word) in b.chunks_exact(8).rev().zip(&mut z) {
            *word = u64::from_be_bytes(bytes.try_into().expect("len == 8"));
        }
        Self { words: z }
    }

    /// Encodes the integer in big-endian format.
    fn to_be_bytes(&self) -> [u8; B] {
        let mut b = [0u8; B];
        for (bytes, word) in b.chunks_exact_mut(8).rev().zip(self.words) {
            bytes.copy_from_slice(&word.to_be_bytes());
        }
        b
    }

    /// Reports whether `self == 0.`
    fn is_zero(&self) -> bool {
        for x in self.words {
            if x != 0 {
                return false;
            }
        }
        true
    }

    /// Sets `*self = self*y + r` and reports whether the result
    /// overflowed.
    fn fma(&mut self, y: u64, r: u64) -> bool {
        let mut c = r;
        for x in &mut self.words {
            (c, *x) = mul_add_ww(*x, y, c);
        }
        c == 0
    }

    /// Computes `(q, r) = self/(58^10)` such that
    ///
    /// ```text
    /// q = x/y
    /// r = x - y*q
    /// ```
    ///
    /// and sets `*self = q`.
    fn quo_radix(&mut self) -> u64 {
        /// 58^10
        const RADIX: u64 = 58 * 58 * 58 * 58 * 58 * 58 * 58 * 58 * 58 * 58;
        // Reciprocal of `RADIX`.
        const REC: u64 = 0x568df8b76cbf212c;

        let mut r = 0;
        for x in self.words.iter_mut().rev() {
            (*x, r) = div_ww(r, *x, RADIX, REC);
        }
        r
    }
}

impl<const W: usize, const B: usize> Ord for Uint<W, B> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare words high to low.
        self.words.iter().rev().cmp(other.words.iter().rev())
    }
}

impl<const W: usize, const B: usize> PartialOrd for Uint<W, B> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(Ord::cmp(self, other))
    }
}

#[cfg(test)]
mod test {
    use std::io::Read;

    use flate2::bufread::GzDecoder;

    use super::*;

    #[derive(Debug, serde_derive::Deserialize)]
    struct TestCase {
        input: String,
        output: String,
    }
    macro_rules! impl_test {
        ($name:ident, $type:ident) => {
            #[test]
            fn $name() {
                const TEST_CASES: &[u8] =
                    include_bytes!(concat!("../testdata/", stringify!($name), ".json.gz"));
                let tests: Vec<TestCase> = serde_json::from_slice(
                    &GzDecoder::new(TEST_CASES)
                        .bytes()
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap(),
                )
                .unwrap();
                for (i, tc) in tests.iter().enumerate() {
                    let test = format!("test case {i}");
                    let input = hex::decode(&tc.input)
                        .expect(&test)
                        .try_into()
                        .expect(&test);

                    let got = $type::encode(&input);
                    // TODO(jdygert): Update test data?
                    let padded = format!("{:1>width$}", tc.output, width = $type::B58_SIZE);
                    assert_eq!(got.as_str(), padded.as_str(), "{test}");

                    let got = $type::decode(&got).expect(&test);
                    assert_eq!(got.as_ref(), input, "{test}");
                }
            }
        };
    }
    impl_test!(test_string16, String16);
    impl_test!(test_string64, String64);

    #[test]
    fn test_from_str() {
        let str = String16::from_str("abcd").unwrap();
        assert_eq!(str.len(), 22);
        assert_eq!(str, "111111111111111111abcd");

        assert_eq!(str, String16::from_str(str.as_str()).unwrap());
    }
}
