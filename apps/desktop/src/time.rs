//! Dates : libellés relatifs (« il y a 2 j »).

/// « il y a 2 j », mêmes paliers que le site.
pub fn relative_label(at: u64, now: u64) -> String {
    let minutes = now.saturating_sub(at) / 60;
    match minutes {
        0 => "à l'instant".to_owned(),
        1..=59 => format!("il y a {minutes} min"),
        60..=1_439 => format!("il y a {} h", minutes / 60),
        1_440..=10_079 => format!("il y a {} j", minutes / 1_440),
        _ => "il y a plus d'une semaine".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_labels_match_the_site() {
        let now = 1_000_000;
        assert_eq!(relative_label(now, now), "à l'instant");
        assert_eq!(relative_label(now - 5 * 60, now), "il y a 5 min");
        assert_eq!(relative_label(now - 3 * 3_600, now), "il y a 3 h");
        assert_eq!(relative_label(now - 2 * 86_400, now), "il y a 2 j");
        assert_eq!(
            relative_label(now - 9 * 86_400, now),
            "il y a plus d'une semaine"
        );
    }
}
