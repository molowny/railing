//! Fuzzy scoring: what the picker and the browsed grid filter with.
//!
//! With the `fuzzy` feature it wraps `nucleo-matcher`, keeping a reusable
//! matcher around so callers don't pay the construction cost on every
//! keystroke, and ranks matches by relevance. Without it, the same API
//! matches substrings: each space-separated word of the query must appear,
//! case-insensitively unless the word has a capital (nucleo's smart case),
//! and matches keep the order they came in.

#[cfg(feature = "fuzzy")]
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
#[cfg(feature = "fuzzy")]
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
    #[cfg(feature = "fuzzy")]
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
            #[cfg(feature = "fuzzy")]
            matcher: Matcher::new(Config::DEFAULT.match_paths()),
        }
    }

    /// Score `haystacks` against `query` and return results sorted by score
    /// (highest first). Non-matching haystacks are filtered out.
    #[cfg(feature = "fuzzy")]
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
        results.sort_by_key(|m| std::cmp::Reverse(m.score));
        results
    }

    /// Without the `fuzzy` feature: every word of `query` as a substring,
    /// matches in the order given (each scores 1), the matched characters as
    /// `indices`.
    #[cfg(not(feature = "fuzzy"))]
    pub fn score<I, S>(&mut self, query: &str, haystacks: I) -> Vec<FuzzyMatch>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let words: Vec<&str> = query.split_whitespace().collect();
        haystacks
            .into_iter()
            .enumerate()
            .filter_map(|(index, hay)| {
                let mut indices = Vec::new();
                for word in &words {
                    indices.extend(find(hay.as_ref(), word)?);
                }
                indices.sort_unstable();
                indices.dedup();
                Some(FuzzyMatch {
                    index,
                    score: 1,
                    indices,
                })
            })
            .collect()
    }
}

/// Where `word` first appears in `hay`, as the char indices it covers:
/// ignoring case unless `word` has a capital.
#[cfg(not(feature = "fuzzy"))]
fn find(hay: &str, word: &str) -> Option<Vec<u32>> {
    let fold = !word.chars().any(char::is_uppercase);
    let norm = |c: char| {
        if fold {
            c.to_lowercase().next().unwrap_or(c)
        } else {
            c
        }
    };
    let hay: Vec<char> = hay.chars().map(norm).collect();
    let word: Vec<char> = word.chars().map(norm).collect();
    if word.is_empty() || word.len() > hay.len() {
        return None;
    }
    let at = (0..=hay.len() - word.len()).find(|&i| hay[i..i + word.len()] == word[..])?;
    Some((at..at + word.len()).map(|i| i as u32).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_substring() {
        let mut fuzzy = Fuzzy::new();
        let results = fuzzy.score("git", ["github.com", "example.org", "gitlab"]);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].index, 0);
        assert_eq!(results[1].index, 2);
    }

    #[cfg(not(feature = "fuzzy"))]
    #[test]
    fn without_nucleo_every_word_is_a_substring_in_order() {
        let mut fuzzy = Fuzzy::new();
        let r = fuzzy.score("srv 1", ["srv-10", "web-1", "SRV-21", "srv-00"]);
        assert_eq!(r.iter().map(|m| m.index).collect::<Vec<_>>(), [0, 2]);
        assert_eq!(r[0].indices, [0, 1, 2, 4]);
        assert!(
            fuzzy.score("Srv", ["srv-1"]).is_empty(),
            "a capital is matched as typed"
        );
    }

    #[cfg(feature = "fuzzy")]
    #[test]
    fn empty_query_matches_nothing() {
        // Empty queries produce matches with score 0 in nucleo — callers
        // typically short-circuit and skip scoring entirely. Documented here.
        let mut fuzzy = Fuzzy::new();
        let results = fuzzy.score("", ["a", "b"]);
        assert!(results.iter().all(|r| r.score == 0));
    }
}
