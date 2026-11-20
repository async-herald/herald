use std::fmt;

/// Note de A+ à F, mêmes paliers que `score::grade_of` du moteur `herald-engine`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Grade {
    APlus,
    A,
    AMinus,
    BPlus,
    B,
    BMinus,
    CPlus,
    C,
    CMinus,
    DPlus,
    D,
    DMinus,
    F,
}

/// Famille de couleur d'une note dans l'interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradeTone {
    Good,
    Average,
    Poor,
}

impl Grade {
    /// Convertit un score sur 100 en note.
    pub fn from_score(score: u8) -> Self {
        match score {
            97.. => Self::APlus,
            93..=96 => Self::A,
            90..=92 => Self::AMinus,
            85..=89 => Self::BPlus,
            80..=84 => Self::B,
            75..=79 => Self::BMinus,
            70..=74 => Self::CPlus,
            65..=69 => Self::C,
            60..=64 => Self::CMinus,
            54..=59 => Self::DPlus,
            47..=53 => Self::D,
            40..=46 => Self::DMinus,
            _ => Self::F,
        }
    }

    /// Score minimal pour obtenir cette note.
    pub fn min_score(self) -> u8 {
        match self {
            Self::APlus => 97,
            Self::A => 93,
            Self::AMinus => 90,
            Self::BPlus => 85,
            Self::B => 80,
            Self::BMinus => 75,
            Self::CPlus => 70,
            Self::C => 65,
            Self::CMinus => 60,
            Self::DPlus => 54,
            Self::D => 47,
            Self::DMinus => 40,
            Self::F => 0,
        }
    }

    /// `true` si cette note est meilleure que `other`.
    ///
    /// L'ordre dérivé (`Ord`) suit la déclaration, de A+ à F : ne pas l'utiliser
    /// pour comparer la qualité de deux notes.
    pub fn is_better_than(self, other: Self) -> bool {
        self.min_score() > other.min_score()
    }

    /// Note immédiatement supérieure, `None` pour A+.
    pub fn next_up(self) -> Option<Self> {
        match self {
            Self::APlus => None,
            Self::A => Some(Self::APlus),
            Self::AMinus => Some(Self::A),
            Self::BPlus => Some(Self::AMinus),
            Self::B => Some(Self::BPlus),
            Self::BMinus => Some(Self::B),
            Self::CPlus => Some(Self::BMinus),
            Self::C => Some(Self::CPlus),
            Self::CMinus => Some(Self::C),
            Self::DPlus => Some(Self::CMinus),
            Self::D => Some(Self::DPlus),
            Self::DMinus => Some(Self::D),
            Self::F => Some(Self::DMinus),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::APlus => "A+",
            Self::A => "A",
            Self::AMinus => "A-",
            Self::BPlus => "B+",
            Self::B => "B",
            Self::BMinus => "B-",
            Self::CPlus => "C+",
            Self::C => "C",
            Self::CMinus => "C-",
            Self::DPlus => "D+",
            Self::D => "D",
            Self::DMinus => "D-",
            Self::F => "F",
        }
    }

    pub fn tone(self) -> GradeTone {
        match self {
            Self::APlus | Self::A | Self::AMinus => GradeTone::Good,
            Self::BPlus | Self::B | Self::BMinus | Self::CPlus => GradeTone::Average,
            _ => GradeTone::Poor,
        }
    }
}

impl fmt::Display for Grade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_boundaries_match_herald_scale() {
        assert_eq!(Grade::from_score(100), Grade::APlus);
        assert_eq!(Grade::from_score(97), Grade::APlus);
        assert_eq!(Grade::from_score(96), Grade::A);
        assert_eq!(Grade::from_score(88), Grade::BPlus);
        assert_eq!(Grade::from_score(78), Grade::BMinus);
        assert_eq!(Grade::from_score(60), Grade::CMinus);
        assert_eq!(Grade::from_score(40), Grade::DMinus);
        assert_eq!(Grade::from_score(39), Grade::F);
        assert_eq!(Grade::from_score(0), Grade::F);
    }

    #[test]
    fn matches_engine_grades() {
        for score in 0..=100u8 {
            assert_eq!(
                Grade::from_score(score).label(),
                herald_engine::score::grade_of(u32::from(score)).label()
            );
        }
    }

    #[test]
    fn next_up_thresholds_are_consistent() {
        let mut grade = Grade::F;
        while let Some(next) = grade.next_up() {
            assert!(next.min_score() > grade.min_score());
            assert_eq!(Grade::from_score(next.min_score()), next);
            grade = next;
        }
        assert_eq!(grade, Grade::APlus);
    }

    #[test]
    fn better_grade_comparison() {
        assert!(Grade::AMinus.is_better_than(Grade::BPlus));
        assert!(!Grade::BPlus.is_better_than(Grade::AMinus));
        assert!(!Grade::B.is_better_than(Grade::B));
    }

    #[test]
    fn tones_group_grades() {
        assert_eq!(Grade::AMinus.tone(), GradeTone::Good);
        assert_eq!(Grade::BPlus.tone(), GradeTone::Average);
        assert_eq!(Grade::C.tone(), GradeTone::Poor);
    }
}
