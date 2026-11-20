#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GifFps {
    Fps10,
    Fps15,
    Fps30,
}

impl GifFps {
    pub const ALL: [GifFps; 3] = [GifFps::Fps10, GifFps::Fps15, GifFps::Fps30];

    pub fn per_second(self) -> u32 {
        match self {
            GifFps::Fps10 => 10,
            GifFps::Fps15 => 15,
            GifFps::Fps30 => 30,
        }
    }
}

impl TryFrom<u32> for GifFps {
    type Error = UnsupportedFps;

    fn try_from(per_second: u32) -> Result<Self, Self::Error> {
        GifFps::ALL
            .into_iter()
            .find(|fps| fps.per_second() == per_second)
            .ok_or(UnsupportedFps(per_second))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedFps(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_offer_10_15_and_30_fps() {
        let offered: Vec<u32> = GifFps::ALL.iter().map(|fps| fps.per_second()).collect();
        assert_eq!(offered, [10, 15, 30]);
    }

    #[test]
    fn a_saved_number_turns_back_into_the_same_fps() {
        for fps in GifFps::ALL {
            assert_eq!(GifFps::try_from(fps.per_second()), Ok(fps));
        }
    }

    #[test]
    fn a_number_outside_the_offered_set_is_rejected() {
        assert_eq!(GifFps::try_from(60), Err(UnsupportedFps(60)));
    }
}
