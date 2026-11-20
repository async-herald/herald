/// Consommation du quota d'analyses du jour, telle que fournie par le compte.
/// La limite est fixée par le serveur (offre de base : 100 analyses par jour).
#[derive(Clone, Copy, Debug)]
pub struct Usage {
    pub used: u32,
    /// Limite du jour ; `None` si illimité.
    pub limit: Option<u32>,
}

impl Usage {
    /// Analyses restantes aujourd'hui ; `None` si illimité.
    pub fn remaining(&self) -> Option<u32> {
        self.limit.map(|limit| limit.saturating_sub(self.used))
    }

    /// Part du quota consommée, entre 0 et 1 (0 si illimité).
    pub fn ratio(&self) -> f32 {
        match self.limit {
            Some(limit) if limit > 0 => (self.used as f32 / limit as f32).min(1.),
            _ => 0.,
        }
    }

    pub fn is_exhausted(&self) -> bool {
        self.remaining() == Some(0)
    }

    /// Il ne reste presque plus d'analyses aujourd'hui (un dixième du quota au plus).
    pub fn is_low(&self) -> bool {
        match (self.remaining(), self.limit) {
            (Some(remaining), Some(limit)) => remaining <= (limit / 10).max(1),
            _ => false,
        }
    }

    /// « 12 / 100 analyses aujourd'hui » ou « Analyses illimitées ».
    pub fn label(&self) -> String {
        match self.limit {
            Some(limit) => format!("{} / {limit} analyses aujourd'hui", self.used),
            None => "Analyses illimitées".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_quota() {
        let usage = Usage {
            used: 12,
            limit: Some(100),
        };
        assert_eq!(usage.remaining(), Some(88));
        assert!(!usage.is_low());
        assert_eq!(usage.label(), "12 / 100 analyses aujourd'hui");

        let usage = Usage {
            used: 95,
            limit: Some(100),
        };
        assert!(usage.is_low());

        let usage = Usage {
            used: 105,
            limit: Some(100),
        };
        assert_eq!(usage.remaining(), Some(0));
        assert!(usage.is_exhausted());
        assert_eq!(usage.ratio(), 1.);
    }

    #[test]
    fn unlimited_usage_has_no_remaining_count() {
        let usage = Usage {
            used: 400,
            limit: None,
        };
        assert_eq!(usage.remaining(), None);
        assert!(!usage.is_low());
        assert_eq!(usage.ratio(), 0.);
    }
}
