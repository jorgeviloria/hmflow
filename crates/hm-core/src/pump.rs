//! Tipos de bomba y su vocabulario común.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Tipo de bomba del proyecto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpType {
    EndSuction,
    Inline,
    RingSection,
    SplitCase,
    Barrel,
}

impl PumpType {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "end_suction" | "end-suction" | "oh1" | "oh2" => Some(Self::EndSuction),
            "inline" | "oh3" | "oh4" => Some(Self::Inline),
            "ring_section" | "ring-section" | "bb4" => Some(Self::RingSection),
            "split_case" | "split-case" | "bb1" | "bb2" | "bb3" => Some(Self::SplitCase),
            "barrel" | "bb5" => Some(Self::Barrel),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::EndSuction => "end_suction",
            Self::Inline => "inline",
            Self::RingSection => "ring_section",
            Self::SplitCase => "split_case",
            Self::Barrel => "barrel",
        }
    }

    /// Norma dimensional por defecto. Solo end-suction tiene una asignada;
    /// el resto la declara el usuario.
    pub fn default_standard(self) -> &'static str {
        match self {
            Self::EndSuction => "ISO 2858",
            Self::Inline | Self::RingSection | Self::SplitCase | Self::Barrel => "",
        }
    }
}

impl fmt::Display for PumpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::PumpType;

    const ALL: [PumpType; 5] = [
        PumpType::EndSuction,
        PumpType::Inline,
        PumpType::RingSection,
        PumpType::SplitCase,
        PumpType::Barrel,
    ];

    #[test]
    fn every_type_parses_from_its_own_name() {
        for pump in ALL {
            assert_eq!(PumpType::parse(pump.as_str()), Some(pump));
        }
    }

    #[test]
    fn api_610_designations_and_dashes_are_aliases() {
        for (text, expected) in [
            ("oh1", PumpType::EndSuction),
            ("oh2", PumpType::EndSuction),
            ("end-suction", PumpType::EndSuction),
            ("oh3", PumpType::Inline),
            ("oh4", PumpType::Inline),
            ("bb4", PumpType::RingSection),
            ("ring-section", PumpType::RingSection),
            ("bb1", PumpType::SplitCase),
            ("bb2", PumpType::SplitCase),
            ("bb3", PumpType::SplitCase),
            ("split-case", PumpType::SplitCase),
            ("bb5", PumpType::Barrel),
        ] {
            assert_eq!(PumpType::parse(text), Some(expected), "{text}");
        }
    }

    #[test]
    fn unknown_names_and_other_cases_are_rejected() {
        assert_eq!(PumpType::parse("inventada"), None);
        assert_eq!(PumpType::parse(""), None);
        assert_eq!(PumpType::parse("END_SUCTION"), None);
    }

    #[test]
    fn display_is_the_snake_case_name() {
        for pump in ALL {
            assert_eq!(pump.to_string(), pump.as_str());
        }
        assert_eq!(PumpType::RingSection.to_string(), "ring_section");
    }

    #[test]
    fn only_end_suction_has_a_default_standard() {
        assert_eq!(PumpType::EndSuction.default_standard(), "ISO 2858");
        for pump in ALL.into_iter().filter(|pump| *pump != PumpType::EndSuction) {
            assert_eq!(pump.default_standard(), "", "{pump}");
        }
    }
}
