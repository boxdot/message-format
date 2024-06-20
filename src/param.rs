use std::{borrow::Cow, fmt, hash};

use icu::locid::Locale;
use icu_decimal::FixedDecimalFormatter;
use ordered_float::OrderedFloat;

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ParamValue {
    inner: ParamValueInner,
}

impl From<ParamValueInner> for ParamValue {
    fn from(inner: ParamValueInner) -> Self {
        Self { inner }
    }
}

#[derive(Debug, Eq)]
enum ParamValueInner {
    Int(i64),
    Dec(OrderedFloat<f64>),
    String(Cow<'static, str>),
}

impl PartialEq for ParamValueInner {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Dec(a), Self::Dec(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Int(a), Self::Dec(b)) => Some(*a) == as_integer(b.into_inner()),
            (Self::Dec(a), Self::Int(b)) => as_integer(a.into_inner()) == Some(*b),
            _ => false,
        }
    }
}

fn as_integer(x: f64) -> Option<i64> {
    (x.is_finite() && x.fract() == 0.0).then_some(x as i64)
}

impl hash::Hash for ParamValueInner {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        match self {
            ParamValueInner::Int(a) => a.hash(state),
            ParamValueInner::Dec(a) => {
                if let Some(a_int) = as_integer(a.into_inner()) {
                    a_int.hash(state);
                } else {
                    a.hash(state);
                }
            }
            ParamValueInner::String(a) => a.hash(state),
        }
    }
}

pub(crate) const OTHER: ParamValue = ParamValue::from_static_str("other");
pub(crate) const ARGUMENT_NAME: ParamValue = ParamValue::from_static_str("argumentName");
pub(crate) const ARGUMENT_OFFSET: ParamValue = ParamValue::from_static_str("argumentOffset");

impl ParamValue {
    pub(crate) const fn from_static_str(s: &'static str) -> Self {
        ParamValue {
            inner: ParamValueInner::String(Cow::Borrowed(s)),
        }
    }

    pub(crate) fn parse_number(s: &str) -> Option<Self> {
        if let Ok(num) = s.parse::<i64>() {
            Some(ParamValueInner::Int(num).into())
        } else if let Ok(num) = s.parse() {
            Some(ParamValueInner::Dec(OrderedFloat(num)).into())
        } else {
            None
        }
    }

    pub(crate) fn format_with_locale(&self, locale: &Locale) -> String {
        match &self.inner {
            ParamValueInner::Int(value) => {
                let fdf = FixedDecimalFormatter::try_new(&locale.into(), Default::default())
                    .expect("missing locale");
                fdf.format_to_string(&(*value).into())
            }
            ParamValueInner::Dec(value) => {
                let value_str = value.to_string();
                if let Ok(fixed_dec) = value.to_string().parse() {
                    let fdf = FixedDecimalFormatter::try_new(&locale.into(), Default::default())
                        .expect("missing locale");
                    fdf.format_to_string(&fixed_dec)
                } else {
                    value_str
                }
            }
            ParamValueInner::String(value) => value.clone().into_owned(),
        }
    }

    pub(crate) fn as_decimal(&self) -> Option<f64> {
        match &self.inner {
            ParamValueInner::Int(n) => Some(*n as f64),
            ParamValueInner::Dec(x) => Some(x.0),
            ParamValueInner::String(s) => s.parse().ok(),
        }
    }
}

impl From<f64> for ParamValue {
    fn from(value: f64) -> Self {
        ParamValueInner::Dec(OrderedFloat(value)).into()
    }
}

macro_rules! impl_from_integer_type {
    ($itype:ident) => {
        impl From<$itype> for ParamValue {
            fn from(value: $itype) -> Self {
                ParamValueInner::Int(value.into()).into()
            }
        }
    };
}

impl_from_integer_type!(i64);
impl_from_integer_type!(i32);
impl_from_integer_type!(i16);
impl_from_integer_type!(i8);
impl_from_integer_type!(u32);
impl_from_integer_type!(u16);
impl_from_integer_type!(u8);

impl From<String> for ParamValue {
    fn from(value: String) -> Self {
        ParamValueInner::String(Cow::Owned(value)).into()
    }
}

impl From<&'static str> for ParamValue {
    fn from(value: &'static str) -> Self {
        ParamValueInner::String(Cow::Borrowed(value)).into()
    }
}

impl fmt::Display for ParamValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.inner {
            ParamValueInner::Int(value) => write!(f, "{}", value),
            ParamValueInner::Dec(value) => write!(f, "{}", value),
            ParamValueInner::String(value) => f.write_str(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use hash::{DefaultHasher, Hasher};
    use quickcheck_macros::quickcheck;

    use super::*;

    #[test]
    fn test_as_integer() {
        assert_eq!(as_integer(0.0), Some(0));
        assert_eq!(as_integer(1.0), Some(1));
        assert_eq!(as_integer(f64::MAX.trunc()), Some(i64::MAX));
        assert_eq!(as_integer(f64::MIN.trunc()), Some(i64::MIN));
        assert_eq!(as_integer(0.1), None);
        assert_eq!(as_integer(f64::NAN), None);
        assert_eq!(as_integer(f64::MIN_POSITIVE), None);
        assert_eq!(as_integer(1.0 + f64::MIN_POSITIVE), Some(1));
        assert_eq!(as_integer(42.0), Some(42));
    }

    #[test]
    fn test_int_float_eq() {
        assert_eq!(ParamValue::from(1.0), ParamValue::from(1));
        assert_eq!(ParamValue::from(1), ParamValue::from(1.0));
        assert_ne!(ParamValue::from(1.1), ParamValue::from(1));
        assert_ne!(ParamValue::from(1), ParamValue::from(1.1));
    }

    fn hash<T: hash::Hash>(value: T) -> u64 {
        let mut state = DefaultHasher::new();
        value.hash(&mut state);
        state.finish()
    }

    // a == b => hash(a) == hash(b) for a, b in {int, float}
    #[quickcheck]
    fn prop_reverse_reverse(x: i64, y: f64) -> bool {
        let a = ParamValue::from(x);
        let b = ParamValue::from(y);
        let c = ParamValue::from(y);
        let d = ParamValue::from(x);
        (a != b || hash(a) == hash(b)) && (c != d || hash(c) == hash(d))
    }
}
