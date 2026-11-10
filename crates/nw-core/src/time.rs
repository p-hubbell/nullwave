use serde::{Deserialize, Serialize};

/// Pulses (ticks) per quarter note.
pub const PPQ: u32 = 960;
pub type Tick = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self { numerator: 4, denominator: 4 }
    }
}

impl TimeSignature {
    pub fn ticks_per_bar(&self) -> Tick {
        PPQ * 4 * self.numerator as u32 / self.denominator as u32
    }
}
