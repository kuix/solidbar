//! Matching what someone types in the project/task picker against option labels.

/// Scores `text` against `query`; `None` when it doesn't match. Every word of the query must
/// occur in the text, either as a substring (ranked higher, highest at a word start) or as
/// a subsequence of characters (`dsgn` matches `Design`).
pub fn score(query: &str, text: &str) -> Option<u32> {
    let text = text.to_lowercase();
    query
        .split_whitespace()
        .map(|word| word_score(&word.to_lowercase(), &text))
        .try_fold(0, |total, score| Some(total + score?))
}

fn word_score(word: &str, text: &str) -> Option<u32> {
    if let Some(position) = text.find(word) {
        let at_word_start = position == 0
            || text[..position]
                .chars()
                .next_back()
                .is_some_and(|before| !before.is_alphanumeric());
        return Some(if at_word_start { 100 } else { 60 });
    }
    let mut characters = text.chars();
    word.chars()
        .all(|wanted| characters.any(|candidate| candidate == wanted))
        .then_some(10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_must_match() {
        assert!(score("design head", "Design system › Header").is_some());
        assert!(score("design footer", "Design system › Header").is_none());
    }

    #[test]
    fn empty_queries_match_everything() {
        assert_eq!(score("  ", "Anything"), Some(0));
    }

    #[test]
    fn word_starts_beat_substrings_beat_subsequences() {
        assert_eq!(score("sys", "Design system"), Some(100));
        assert_eq!(score("sys", "Admin › Easysync"), Some(60));
        assert_eq!(score("dsgn", "Design system"), Some(10));
        assert_eq!(score("sys", "Kuix › Dsgn review"), None);
    }

    #[test]
    fn matching_ignores_case_and_handles_accents() {
        assert!(score("ÁRAJ", "Árajánlat › Kuix").is_some());
    }
}
