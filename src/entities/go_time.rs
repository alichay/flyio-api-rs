use std::{ops::{Deref, DerefMut, Range}, fmt::{Display, Formatter}};
use serde::{Serialize, Serializer, Deserialize, Deserializer, de::Visitor};
use thiserror::Error;

/// Wrapper for [`chrono::DateTime<chrono::FixedOffset>`] that serializes and deserializes to a formatted string,
/// like Go's `time.Time` type. RFC 3339 formatted
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Copy)]
pub struct GoTime(
    chrono::DateTime<chrono::FixedOffset>
);

impl Serialize for GoTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.to_rfc3339().as_str())
    }
}

impl<'de> Deserialize<'de> for GoTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>
    {
        deserializer.deserialize_str(TimeVisitor)
    }
}

struct TimeVisitor;
impl<'de> Visitor<'de> for TimeVisitor {
    type Value = GoTime;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a RFC 3339 formatted string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error
    {
        match chrono::DateTime::parse_from_rfc3339(v) {
            Ok(dt) => Ok(GoTime(dt)),
            Err(e) => Err(E::custom(format!("invalid RFC 3339 formatted string: {}", e))),
        }
    }
}

impl Deref for GoTime {
    type Target = chrono::DateTime<chrono::FixedOffset>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl From<chrono::DateTime<chrono::FixedOffset>> for GoTime {
    fn from(dt: chrono::DateTime<chrono::FixedOffset>) -> Self {
        GoTime(dt)
    }
}
impl From<GoTime> for chrono::DateTime<chrono::FixedOffset> {
    fn from(t: GoTime) -> Self {
        t.0
    }
}
impl DerefMut for GoTime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Error)]
#[error("invalid duration format: {0}")]
pub struct InvalidDurationFormatError(String);

/// Wrapper for [`std::time::Duration`] that serializes and deserializes to a formatted string,
/// like Flyctl's Duration wrapper type.
/// 
/// Serializes as a string in the format of "72h3m0.5s"
/// * leading zeros are omitted
/// * if duration <1s, uses ns, µs, or ms.
/// * if time == 0, serializes to "0s"
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Copy)]
pub struct FlyctlDuration(
    std::time::Duration
);
struct TrimmedStr<const S: usize>(arrayvec::ArrayString::<S>, Range<usize>);
impl<const S: usize> Deref for TrimmedStr<S> {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0[self.1.clone()]
    }
}
impl<const S: usize> Display for TrimmedStr<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self)
    }
}

fn format_fixed_point(n: u128, decimal_idx: usize) -> TrimmedStr<32> {
    let mut sb = arrayvec::ArrayString::<32>::new();
    let mut n = n;
    let mut decimal_idx = decimal_idx;
    while n > 0 {
        sb.push(((n % 10) as u8 + b'0') as char);
        n /= 10;
        if decimal_idx == 1 {
            sb.push('.');
        }
        decimal_idx = decimal_idx.saturating_sub(1);
    }
    if sb.ends_with('.') {
        sb.push('0');
    }
    // No .collect() for arrayvec
    let s = {
        let mut s = arrayvec::ArrayString::new();
        for c in sb.chars().rev() {
            s.push(c);
        }
        s
    };

    let last_number = s.rfind(|c| !matches!(c, '0'|'.'));
    if let Some(last_number) = last_number {
        TrimmedStr(s, 0..last_number+1)
    } else {
        TrimmedStr(s, 0..0)
    }
}

fn parse_fixed_point(s: &str, shift_amt: usize) -> Result<u64, InvalidDurationFormatError> {


    let mut buf = 0u64;
    let mut iter = s.chars();
    for c in iter.by_ref() {
        if c == '.' {
            break;
        }
        let c = c.to_digit(10).ok_or_else(|| InvalidDurationFormatError(s.to_string()))? as u64;
        buf = buf * 10 + c;
    }
    let new_iter = iter.chain(std::iter::repeat('0')).take(shift_amt);
    for c in new_iter {
        let c = c.to_digit(10).ok_or_else(|| InvalidDurationFormatError(s.to_string()))? as u64;
        buf = buf * 10 + c;
    }
    Ok(buf)
}

// Test parse_fixed_point
#[test]
fn test_parse_fixed_point() {
    assert_eq!(parse_fixed_point("0.5", 0).unwrap(), 0);
    assert_eq!(parse_fixed_point("0.5", 1).unwrap(), 5);
    assert_eq!(parse_fixed_point("0.5", 2).unwrap(), 50);
    assert_eq!(parse_fixed_point("100", 0).unwrap(), 100);
    assert_eq!(parse_fixed_point("100", 3).unwrap(), 100000);
    assert_eq!(parse_fixed_point("100.5", 0).unwrap(), 100);
    assert_eq!(parse_fixed_point("100.5", 1).unwrap(), 1005);
    assert_eq!(parse_fixed_point("10.512", 2).unwrap(), 1051);
}


#[cfg(test)]
#[derive(Debug, Deserialize)]
struct TestCase {
    #[serde(rename = "Str")]
    s: String,
    #[serde(rename = "Hrs")]
    hrs: u64,
    #[serde(rename = "SecPart")]
    sec_part: u64,
    #[serde(rename = "NsPart")]
    ns_part: u32,
}

#[test]
fn test_parse_go_durations() {

    let test_cases: Vec<TestCase> = serde_json::from_str(include_str!("time_test_data.json")).unwrap();

    for t in test_cases {
        let d = t.s.parse::<FlyctlDuration>().unwrap();
        
        let d_hrs = d.0.as_secs() / 3600;
        assert_eq!(d_hrs, t.hrs);

        let d_sec_part = d.0.as_secs() % 3600;
        assert_eq!(d_sec_part, t.sec_part);

        assert_eq!(d.0.subsec_nanos(), t.ns_part);
    }
}

#[test]
fn test_write_go_durations() {

    let test_cases: Vec<TestCase> = serde_json::from_str(include_str!("time_test_data.json")).unwrap();

    for t in test_cases {
        let d = FlyctlDuration(std::time::Duration::new(t.hrs * 3600 + t.sec_part, t.ns_part));
        let s = d.to_string();
        assert_eq!(s, t.s);
    }
}

// Test that duration can rount-trip
#[test]
fn test_duration_round_trip() {

    let d = FlyctlDuration(std::time::Duration::from_secs(123456789));
    let s = d.to_string();
    println!("s: {s}");
    let d2 = s.parse::<FlyctlDuration>().unwrap();
    assert_eq!(d.0, d2.0);


    let d = FlyctlDuration(std::time::Duration::from_secs_f64(0.0354));
    let s = d.to_string();
    println!("s: {s}");
    let d2 = s.parse::<FlyctlDuration>().unwrap();
    assert_eq!(d.0, d2.0);
}

impl ToString for FlyctlDuration {
    fn to_string(&self) -> String {
        let ns = self.0.as_nanos();
        if ns == 0 {
            return "0s".to_string();
        }

        let hrs = self.0.as_secs() / (60 * 60);
        let mins = (self.0.as_secs() % (60 * 60)) / 60;
        let mut sb = String::with_capacity(16);
        if hrs > 0 {
            sb.push_str(&format!("{}h", hrs));
        }
        if mins > 0 || hrs > 0 {
            sb.push_str(&format!("{}m", mins));
        }
        
        // Use fixed-point u128 to avoid floating point errors
        // Keep decimal precision, but remove trailing zeros and decimal point
        if ns < 1_000 {
            sb.push_str(&format!("{}ns", ns));
        } else if self.as_micros() < 1_000 {
            sb.push_str(&format!("{}µs", format_fixed_point(ns, 3)));
        } else if self.as_millis() < 1_000 {
            sb.push_str(&format!("{}ms", format_fixed_point(ns, 6)));
        } else {
            let mins_as_secs = ((self.0.as_secs() / 60) * 60) as u128;
            let ns = self.as_nanos() - mins_as_secs * 1_000_000_000;
            sb.push_str(&format!("{}s", format_fixed_point(ns, 9)));
        }
        sb
    }
}
impl std::str::FromStr for FlyctlDuration {
    type Err = InvalidDurationFormatError;

    fn from_str(s: &str) -> Result<Self, InvalidDurationFormatError> {
        let mut num_str = arrayvec::ArrayString::<16>::new();
        let mut chars = s.chars().peekable();
        let mut duration = std::time::Duration::default();
        while let Some(c) = chars.next() {
            let c: char = c;
            if matches!(c, '0'..='9'|'.') {
                // println!("current: {num_str}\npushing: {c}");
                num_str.push(c);
                continue
            }
            if num_str.is_empty() {
                return Err(InvalidDurationFormatError(s.to_string()));
            }
            let peek = chars.peek();
            match (c, peek) {
                ('m', Some('s')) => {
                    _ = chars.next();
                    duration += std::time::Duration::from_nanos(parse_fixed_point(&num_str, 6)?);
                },
                ('µ', Some('s')) | ('u', Some('s')) => {
                    _ = chars.next();
                    duration += std::time::Duration::from_nanos(parse_fixed_point(&num_str, 3)?);
                },
                ('n', Some('s')) => {
                    _ = chars.next();
                    duration += std::time::Duration::from_nanos(parse_fixed_point(&num_str, 0)?);
                },
                ('s', _) => {
                    duration += std::time::Duration::from_nanos(parse_fixed_point(&num_str, 9)?);
                }
                ('m', _) => {
                    let mins = u64::from_str(&num_str).map_err(|_| InvalidDurationFormatError(s.to_string()))?;
                    duration += std::time::Duration::from_secs(mins * 60);
                }
                ('h', _) => {
                    let hrs = u64::from_str(&num_str).map_err(|_| InvalidDurationFormatError(s.to_string()))?;
                    duration += std::time::Duration::from_secs(hrs * 60 * 60);
                }
                _ => return Err(InvalidDurationFormatError(s.to_string())),
            }
            num_str.clear();
        }

        Ok(FlyctlDuration(duration))
    }
}
impl From<std::time::Duration> for FlyctlDuration {
    fn from(d: std::time::Duration) -> Self {
        FlyctlDuration(d)
    }
}
impl From<FlyctlDuration> for std::time::Duration {
    fn from(d: FlyctlDuration) -> Self {
        d.0
    }
}
impl Deref for FlyctlDuration {
    type Target = std::time::Duration;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for FlyctlDuration {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Serialize for FlyctlDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let time_string = self.to_string();
        serializer.serialize_str(&time_string)
    }
}

impl<'de> Deserialize<'de> for FlyctlDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>
    {
        deserializer.deserialize_str(FlyctlDurationVisitor)
    }
}

struct FlyctlDurationVisitor;
impl<'de> Visitor<'de> for FlyctlDurationVisitor {
    type Value = FlyctlDuration;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a duration string, in the format or 37h12m0.5s")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error
    {
        match v.parse() {
            Ok(d) => Ok(d),
            Err(e) => Err(E::custom(format!("invalid duration string: {}", e))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Copy)]
/// Wrapper for [`std::time::Duration`] that serializes and deserializes like Go's `time.Duration`.
pub struct GoDuration(pub std::time::Duration);

impl From<std::time::Duration> for GoDuration {
    fn from(d: std::time::Duration) -> Self {
        GoDuration(d)
    }
}
impl From<GoDuration> for std::time::Duration {
    fn from(d: GoDuration) -> Self {
        d.0
    }
}
impl Deref for GoDuration {
    type Target = std::time::Duration;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for GoDuration {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Serialize for GoDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i64(self.as_nanos().min(0).max(i64::MAX as u128) as i64)
    }
}

impl<'de> Deserialize<'de> for GoDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>
    {
        deserializer.deserialize_i64(GoDurationVisitor)
    }
}

struct GoDurationVisitor;
impl<'de> Visitor<'de> for GoDurationVisitor {
    type Value = GoDuration;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a duration represented as an i64 of nanoseconds")
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error, {
        Ok(GoDuration(std::time::Duration::from_nanos(v as u64)))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error, {
        Ok(GoDuration(std::time::Duration::from_nanos(v)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Copy)]
/// Wrapper for [`std::time::Duration`] that serializes and deserializes like Go's `time.Duration`.
pub struct UnixTime(pub std::time::SystemTime);

impl From<std::time::SystemTime> for UnixTime {
    fn from(d: std::time::SystemTime) -> Self {
        UnixTime(d)
    }
}
impl From<UnixTime> for std::time::SystemTime {
    fn from(d: UnixTime) -> Self {
        d.0
    }
}
impl Deref for UnixTime {
    type Target = std::time::SystemTime;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for UnixTime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Serialize for UnixTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_i64(self.0.duration_since(std::time::UNIX_EPOCH).map_err(serde::ser::Error::custom)?.as_secs() as i64)
    }
}

impl<'de> Deserialize<'de> for UnixTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>
    {
        deserializer.deserialize_i64(UnixTimeVisitor)
    }
}

struct UnixTimeVisitor;
impl<'de> Visitor<'de> for UnixTimeVisitor {
    type Value = UnixTime;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a duration represented as an i64 of nanoseconds")
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error, {
        Ok(UnixTime(std::time::UNIX_EPOCH + std::time::Duration::from_secs(v as u64)))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error, {
        Ok(UnixTime(std::time::UNIX_EPOCH + std::time::Duration::from_secs(v)))
    }
}