use gif_core::fit::{Adjust, BEST_LEVEL, EncodeSettings, Quality};

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityChoice {
    pub level: Option<usize>,
    pub max_bytes: Option<usize>,
    pub dither: f32,
}

pub const NOTHING_TO_ADJUST: &str =
    "크기와 화질 단계를 둘 다 정하면 용량 목표에 맞춰 조정할 것이 없습니다. 둘 중 하나를 자동으로 두세요.";

impl QualityChoice {
    pub fn settings(self, size_is_fixed: bool) -> Result<EncodeSettings, String> {
        let quality = match (self.max_bytes, self.level, size_is_fixed) {
            (None, level, _) => Quality::Level(level.unwrap_or(BEST_LEVEL)),
            (Some(max_bytes), None, true) => Quality::WithinBytes { max_bytes, adjust: Adjust::Level },
            (Some(max_bytes), Some(level), false) => Quality::WithinBytes { max_bytes, adjust: Adjust::Size { level } },
            (Some(max_bytes), None, false) => Quality::WithinBytes { max_bytes, adjust: Adjust::LevelThenSize },
            (Some(_), Some(_), true) => return Err(NOTHING_TO_ADJUST.to_string()),
        };
        Ok(EncodeSettings { quality, dither: self.dither })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXED_SIZE: bool = true;
    const FREE_SIZE: bool = false;

    fn quality(level: Option<usize>, max_bytes: Option<usize>, size_is_fixed: bool) -> Result<Quality, String> {
        let choice = QualityChoice { level, max_bytes, dither: 0.5 };
        choice.settings(size_is_fixed).map(|settings| settings.quality)
    }

    #[test]
    fn without_a_limit_the_chosen_level_is_used_and_automatic_means_the_best() {
        assert_eq!(quality(Some(3), None, FIXED_SIZE), Ok(Quality::Level(3)));
        assert_eq!(quality(Some(3), None, FREE_SIZE), Ok(Quality::Level(3)));
        assert_eq!(quality(None, None, FIXED_SIZE), Ok(Quality::Level(BEST_LEVEL)));
    }

    #[test]
    fn a_limit_with_the_size_fixed_adjusts_the_level() {
        let expected = Quality::WithinBytes { max_bytes: 5000, adjust: Adjust::Level };
        assert_eq!(quality(None, Some(5000), FIXED_SIZE), Ok(expected));
    }

    #[test]
    fn a_limit_with_the_level_fixed_adjusts_the_size() {
        let expected = Quality::WithinBytes { max_bytes: 5000, adjust: Adjust::Size { level: 2 } };
        assert_eq!(quality(Some(2), Some(5000), FREE_SIZE), Ok(expected));
    }

    #[test]
    fn a_limit_with_nothing_fixed_adjusts_the_level_and_then_the_size() {
        let expected = Quality::WithinBytes { max_bytes: 5000, adjust: Adjust::LevelThenSize };
        assert_eq!(quality(None, Some(5000), FREE_SIZE), Ok(expected));
    }

    #[test]
    fn a_limit_with_both_fixed_is_refused_with_the_reason() {
        assert_eq!(quality(Some(2), Some(5000), FIXED_SIZE), Err(NOTHING_TO_ADJUST.to_string()));
    }

    #[test]
    fn the_dithering_strength_is_passed_through() {
        let choice = QualityChoice { level: None, max_bytes: None, dither: 0.25 };
        assert_eq!(choice.settings(FREE_SIZE).unwrap().dither, 0.25);
    }
}
