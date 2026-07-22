use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BitrateBps(pub u64);

impl BitrateBps {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
    pub fn from_kbps(value: f64) -> Result<Self, CoreError> {
        if !value.is_finite() || value < 0.0 {
            return Err(CoreError::InvalidValue("bitrate must be finite and non-negative".into()));
        }
        Ok(Self(value.mul_add(1000.0, 0.5) as u64))
    }
}

impl Display for BitrateBps {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} bps", self.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CompressionRatio(pub f64);

impl CompressionRatio {
    pub fn new(value: f64) -> Result<Self, CoreError> {
        if !value.is_finite() || value <= 0.0 {
            return Err(CoreError::InvalidValue("ratio must be finite and greater than 0".into()));
        }
        Ok(Self(value))
    }
    pub const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Seconds(pub f64);

impl Seconds {
    pub fn new(value: f64) -> Result<Self, CoreError> {
        if !value.is_finite() || value < 0.0 {
            return Err(CoreError::InvalidValue("seconds must be finite and non-negative".into()));
        }
        Ok(Self(value))
    }
    pub const fn get(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Percent(pub f64);

impl Percent {
    pub fn new(value: f64) -> Result<Self, CoreError> {
        if !value.is_finite() || !(0.0..=100.0).contains(&value) {
            return Err(CoreError::InvalidValue("percent must be between 0 and 100".into()));
        }
        Ok(Self(value))
    }
    pub const fn get(self) -> f64 {
        self.0
    }
}
