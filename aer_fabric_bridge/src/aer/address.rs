use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct SynapseId(pub u64);

impl SynapseId {
    pub const SYNAPSE_ENTITY_CLASS: u8 = 0x5;

    pub fn entity_class(self) -> u8 {
        ((self.0 >> 60) & 0x0f) as u8
    }
}

impl Display for SynapseId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:016x}", self.0)
    }
}

impl FromStr for SynapseId {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("synapse id is empty".to_string());
        }
        let value = if let Some(hex) = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
        {
            u64::from_str_radix(hex, 16).map_err(|e| format!("invalid hex synapse id: {e}"))?
        } else {
            trimmed
                .parse::<u64>()
                .map_err(|e| format!("invalid synapse id: {e}"))?
        };
        Ok(SynapseId(value))
    }
}

impl Serialize for SynapseId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SynapseId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum SynapseIdInput {
            Str(String),
            Num(u64),
        }

        let input = SynapseIdInput::deserialize(deserializer)?;
        match input {
            SynapseIdInput::Str(s) => SynapseId::from_str(&s).map_err(serde::de::Error::custom),
            SynapseIdInput::Num(v) => Ok(SynapseId(v)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SynapseId;
    use std::str::FromStr;

    #[test]
    fn synapse_hex_parses() {
        let parsed = SynapseId::from_str("0x5001000200001234").expect("hex parses");
        assert_eq!(parsed.0, 0x5001_0002_0000_1234);
        assert_eq!(parsed.entity_class(), SynapseId::SYNAPSE_ENTITY_CLASS);
    }

    #[test]
    fn synapse_display_round_trip() {
        let id = SynapseId(0x5001_0002_0000_1234);
        let rendered = id.to_string();
        let reparsed = SynapseId::from_str(&rendered).expect("round trip");
        assert_eq!(reparsed, id);
    }
}
