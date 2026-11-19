/// Runtime toggle for the candidate dropdown, backed by `Set-Suggestions` in `.shyena`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Suggestions {
    #[default]
    On,
    Off,
}

impl Suggestions {
    #[inline]
    pub fn from_config_str(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("on") {
            Some(Self::On)
        } else if value.eq_ignore_ascii_case("off") {
            Some(Self::Off)
        } else {
            None
        }
    }

    #[inline]
    pub fn enabled(self) -> bool {
        matches!(self, Self::On)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suggestions_config_strings() {
        assert_eq!(Suggestions::default(), Suggestions::On);
        assert_eq!(Suggestions::from_config_str("on"), Some(Suggestions::On));
        assert_eq!(Suggestions::from_config_str("OFF"), Some(Suggestions::Off));
        assert_eq!(Suggestions::from_config_str("enabled"), None);
    }

    #[test]
    fn test_suggestions_enabled() {
        assert!(Suggestions::On.enabled());
        assert!(!Suggestions::Off.enabled());
    }
}
