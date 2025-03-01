use byteorder::ReadBytesExt;
use std::collections::BTreeMap;
use std::io::Read;
use std::str::FromStr;
use std::{fmt, io};
use crate::error::{invalid_data, Error};
use crate::Result;

#[derive(Clone, Default)]
pub struct Metadata {
    pairs: BTreeMap<String, String>,
}

impl Metadata {
    pub fn get<T: FromStr>(&self, key: &str) -> Option<T> {
        self.pairs.get(key).and_then(|value| value.parse().ok())
    }
}

impl fmt::Debug for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (key, value) in &self.pairs {
            write!(f, "{}={};", key, value)?;
        }
        Ok(())
    }
}

impl FromStr for Metadata {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        // metadata is a list of `key=value` pairs separated by semicolons
        let mut pairs = BTreeMap::new();
        for pair in s.split(';') {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next().ok_or_else(|| invalid_data("missing key"))?;
            let value = parts.next().ok_or_else(|| invalid_data("missing value"))?;
            pairs.insert(key.to_string(), value.to_string());
        }
        Ok(Self { pairs })
    }
}

pub(crate) fn read_indexed_metadata(data: &[u8]) -> Result<Vec<Metadata>> {
    let mut cursor = io::Cursor::new(data);
    let mut metadata = vec![Metadata::default()];  // metadata #0 is empty metadata
    while cursor.position() < data.len() as u64 {
        let size = cursor.read_u8()?;
        let mut buffer = vec![0; size as usize];
        cursor.read_exact(&mut buffer)?;
        let meta =
            std::str::from_utf8(&buffer).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid UTF-8"))?;
        metadata.push(meta.parse()?);
    }
    Ok(metadata)
}
