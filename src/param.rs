use std::fmt;

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

#[derive(Debug, PartialEq, Eq, Hash)]
enum ParamValueInner {
    Int(i64),
    Dec(OrderedFloat<f64>),
    String(String),
}

impl ParamValue {
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
            ParamValueInner::String(value) => value.clone(),
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
        ParamValueInner::String(value).into()
    }
}

impl From<&'static str> for ParamValue {
    fn from(value: &'static str) -> Self {
        value.to_owned().into()
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
