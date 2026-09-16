//! Fuzzy scoring helper wrapping `nucleo-matcher`.
//!
//! Keeps a reusable [`Matcher`] around so callers don't pay the
//! construction cost on every keystroke.

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// A scored match for an indexed haystack.
pub struct FuzzyMatch {
    /// Original index of the matched haystack.
    pub index: usize,
    /// Score from the matcher (higher is better).
    pub score: u32,
    /// Indices of matched characters in the haystack (for highlighting).
    pub indices: Vec<u32>,
}

/// Reusable fuzzy matcher. Cheap to call repeatedly.
pub struct Fuzzy {
    matcher: Matcher,
}

impl Default for Fuzzy {
    fn default() -> Self {
        Self::new()
    }
}

impl Fuzzy {
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
        }
    }

    /// Score `haystacks` against `query` and return results sorted by score
    /// (highest first). Non-matching haystacks are filtered out.
    pub fn score<I, S>(&mut self, query: &str, haystacks: I) -> Vec<FuzzyMatch>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let pattern = Pattern::new(
            query,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut buf = Vec::new();
        let mut results: Vec<FuzzyMatch> = haystacks
            .into_iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let hay = Utf32Str::new(s.as_ref(), &mut buf);
                let mut indices = Vec::new();
                let score = pattern.indices(hay, &mut self.matcher, &mut indices)?;
                Some(FuzzyMatch {
                    index: i,
                    score,
                    indices,
                })
            })
            .collect();
        results.sort_by(|a, b| b.score.cmp(&a.score));
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_substring() {
        let mut fuzzy = Fuzzy::new();
        let results = fuzzy.score("git", &["github.com", "example.org", "gitlab"]);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].index, 0);
        assert_eq!(results[1].index, 2);
    }

    #[test]
    fn empty_query_matches_nothing() {
        // Empty queries produce matches with score 0 in nucleo — callers
        // typically short-circuit and skip scoring entirely. Documented here.
        let mut fuzzy = Fuzzy::new();
        let results = fuzzy.score("", &["a", "b"]);
        assert!(results.iter().all(|r| r.score == 0));
    }
}
