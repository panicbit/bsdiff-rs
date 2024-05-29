use std::fmt;

use md5::{Digest, Md5};
use serde::de::Error;
use serde::Deserialize;

#[derive(Clone)]
pub struct Checksum([u8; 16]);

impl Checksum {
    pub fn validate_bytes(&self, bytes: impl AsRef<[u8]>) -> bool {
        let checksum = Md5::digest(bytes);

        checksum.as_slice() == self.0
    }
}

impl<'de> Deserialize<'de> for Checksum {
    fn deserialize<D>(de: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let checksum = String::deserialize(de)?;
        let checksum = hex::decode(checksum).map_err(D::Error::custom)?;
        let checksum = <[u8; 16]>::try_from(checksum)
            .map_err(|_| D::Error::custom("invalid checksum size"))?;

        Ok(Checksum(checksum))
    }
}

impl fmt::Debug for Checksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Checksum")
            .field(&hex::encode(self.0))
            .finish()
    }
}
