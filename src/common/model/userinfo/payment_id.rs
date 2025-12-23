#[derive(
    Clone, Copy, PartialEq, Eq, Hash, ::rkyv::Archive, ::rkyv::Deserialize, ::rkyv::Serialize,
)]
#[repr(transparent)]
pub struct PaymentId([u8; 14]);

impl PaymentId {
    pub fn new(id: &str) -> Option<Self> {
        let suffix = id.strip_prefix("cus_").unwrap_or(id);
        let bytes = suffix.as_bytes();

        match bytes.try_into() {
            Ok(array) => {
                if bytes.iter().all(|&c| is_alphanumeric(c)) {
                    Some(Self(array))
                } else {
                    crate::debug!("{suffix:?} 包含非字母数字字符");
                    None
                }
            }
            Err(_) => {
                crate::debug!("{suffix:?} length is {} but expected 14", suffix.len());
                None
            }
        }
    }

    #[inline(always)]
    pub const fn as_str(&self) -> &str { unsafe { std::str::from_utf8_unchecked(&self.0) } }
}

/// 验证字符是否为大写字母、小写字母或数字
///
/// # 参数
/// * `c` - 要验证的 u8 字符
///
/// # 返回值
/// * `bool` - 如果字符是 A-Z, a-z 或 0-9 之间的字符则返回 true，否则返回 false
#[inline]
fn is_alphanumeric(c: u8) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_lowercase() || c.is_ascii_digit()
}

impl core::fmt::Debug for PaymentId {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result { self.as_str().fmt(f) }
}

impl core::fmt::Display for PaymentId {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "cus_{}", self.as_str())
    }
}

impl serde::Serialize for PaymentId {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: ::serde::Serializer {
        serializer.collect_str(self)
    }
}

impl<'de> ::serde::Deserialize<'de> for PaymentId {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: ::serde::Deserializer<'de> {
        let s = <String as ::serde::Deserialize>::deserialize(deserializer)?;
        Self::new(&s)
            .ok_or_else(|| ::serde::de::Error::custom(format_args!("unknown payment id: {s}")))
    }
}
