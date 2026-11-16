//! Fuzzy subsequence matching for pickers. Pure, so callers can rank a large
//! list on the background executor.

/// One candidate's match. Higher scores are better matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub score: i32,
    /// Byte offsets of the matched characters in `candidate`, ascending.
    pub positions: Vec<usize>,
}

const MATCH: i32 = 16;
/// The previous query character matched the previous candidate character.
const CONSECUTIVE: i32 = 12;
/// First character of the candidate, or of a path segment.
const SEGMENT_START: i32 = 12;
/// First character after a space or punctuation.
const WORD_START: i32 = 10;
/// A capital after a lowercase letter, or a digit after a letter.
const INNER_START: i32 = 7;
/// The character sits after the last path separator: the file name.
const FILE_NAME: i32 = 3;
/// A capital in the query asks for a capital; lowercase asks for nothing.
const SAME_CASE: i32 = 1;
const GAP_OPEN: i32 = 3;
const GAP_EXTEND: i32 = 1;
/// A long directory prefix should not outweigh a good file-name match.
const GAP_LIMIT: i32 = 10;
/// Query × candidate cells for the exact search; larger inputs match greedily.
const TABLE_LIMIT: usize = 64 * 1024;
const NONE: i32 = i32::MIN / 2;

struct Cell {
    offset: usize,
    original: char,
    folded: char,
    bonus: i32,
}

fn fold(ch: char) -> char {
    // One-to-one folding keeps byte offsets simple; multi-character lowercase
    // forms compare by their first character.
    ch.to_lowercase().next().unwrap_or(ch)
}

fn is_separator(ch: char) -> bool {
    ch == '/' || ch == '\\'
}

fn cells(candidate: &str) -> Vec<Cell> {
    // Both separators are one byte, so the file name starts just after.
    let name_start = candidate.rfind(is_separator).map_or(0, |offset| offset + 1);
    let mut previous: Option<char> = None;
    candidate
        .char_indices()
        .map(|(offset, ch)| {
            let start = match previous {
                None => SEGMENT_START,
                Some(before) if is_separator(before) => SEGMENT_START,
                Some(before) if !before.is_alphanumeric() && ch.is_alphanumeric() => WORD_START,
                Some(before) if before.is_lowercase() && ch.is_uppercase() => INNER_START,
                Some(before) if before.is_alphabetic() && ch.is_numeric() => INNER_START,
                Some(_) => 0,
            };
            previous = Some(ch);
            Cell {
                offset,
                original: ch,
                folded: fold(ch),
                bonus: start + if offset >= name_start { FILE_NAME } else { 0 },
            }
        })
        .collect()
}

fn gap_penalty(skipped: usize) -> i32 {
    (GAP_OPEN + (skipped as i32 - 1) * GAP_EXTEND).min(GAP_LIMIT)
}

/// Score of matching `needle` at `cell`, before what led up to it.
fn base(needle: char, cell: &Cell) -> i32 {
    let same_case = needle.is_uppercase() && needle == cell.original;
    MATCH + cell.bonus + if same_case { SAME_CASE } else { 0 }
}

/// Leftmost alignment, or `None` when `needle` is not a subsequence.
fn greedy(needle: &[char], hay: &[Cell]) -> Option<Vec<usize>> {
    let mut indices = Vec::with_capacity(needle.len());
    let mut next = 0;
    for ch in needle {
        let folded = fold(*ch);
        let found = hay[next..].iter().position(|cell| cell.folded == folded)?;
        indices.push(next + found);
        next += found + 1;
    }
    Some(indices)
}

fn score(needle: &[char], hay: &[Cell], indices: &[usize]) -> i32 {
    let mut total = 0;
    for (j, index) in indices.iter().enumerate() {
        total += base(needle[j], &hay[*index]);
        if j > 0 {
            let skipped = index - indices[j - 1] - 1;
            total += if skipped == 0 {
                CONSECUTIVE
            } else {
                -gap_penalty(skipped)
            };
        }
    }
    total
}

/// Best alignment by dynamic programming: `table[j][i]` is the best score of
/// `needle[..=j]` with its last character matched at `hay[i]`.
fn best(needle: &[char], hay: &[Cell]) -> Vec<usize> {
    let (rows, columns) = (needle.len(), hay.len());
    let mut table = vec![NONE; rows * columns];
    for (j, ch) in needle.iter().enumerate() {
        let folded = fold(*ch);
        // Best earlier-row score reachable across a gap that ends before column `i`.
        let mut affine = NONE;
        let mut peak = NONE;
        for i in j..columns {
            if j > 0 && i >= 2 {
                let earlier = table[(j - 1) * columns + i - 2];
                affine = (affine - GAP_EXTEND).max(earlier - GAP_OPEN);
                peak = peak.max(earlier);
            }
            if hay[i].folded != folded {
                continue;
            }
            let lead = if j == 0 {
                0
            } else {
                let adjacent = table[(j - 1) * columns + i - 1] + CONSECUTIVE;
                adjacent.max(affine).max(peak - GAP_LIMIT)
            };
            if lead > NONE / 2 {
                table[j * columns + i] = base(*ch, &hay[i]) + lead;
            }
        }
    }
    trace(needle, hay, &table)
}

/// Walk the table back from its best final cell to recover the alignment.
fn trace(needle: &[char], hay: &[Cell], table: &[i32]) -> Vec<usize> {
    let (rows, columns) = (needle.len(), hay.len());
    let last = &table[(rows - 1) * columns..];
    let mut index = (0..columns).max_by_key(|i| (last[*i], *i)).unwrap_or(0);
    let mut indices = vec![index; rows];
    for j in (1..rows).rev() {
        let lead = table[j * columns + index] - base(needle[j], &hay[index]);
        let row = &table[(j - 1) * columns..j * columns];
        index = if row[index - 1] + CONSECUTIVE == lead {
            index - 1
        } else {
            (0..index - 1)
                .rev()
                .find(|k| row[*k] > NONE / 2 && row[*k] - gap_penalty(index - 1 - k) == lead)
                .unwrap_or(index - 1)
        };
        indices[j - 1] = index;
    }
    indices
}

/// Case-insensitive subsequence match of `query` in `candidate`.
///
/// An empty query matches everything with score 0. Consecutive runs, word
/// starts, path-segment starts, and matches in the file name score higher.
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<Match> {
    if query.is_empty() {
        return Some(Match {
            score: 0,
            positions: Vec::new(),
        });
    }
    let needle = query.chars().collect::<Vec<_>>();
    let hay = cells(candidate);
    // Rejects most candidates before the table is allocated.
    let leftmost = greedy(&needle, &hay)?;
    let indices = if needle.len() * hay.len() > TABLE_LIMIT {
        leftmost
    } else {
        best(&needle, &hay)
    };
    Some(Match {
        score: score(&needle, &hay, &indices),
        positions: indices.iter().map(|index| hay[*index].offset).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched(query: &str, candidate: &str) -> Match {
        fuzzy_match(query, candidate).unwrap_or_else(|| panic!("{query:?} in {candidate:?}"))
    }

    fn text(candidate: &str, found: &Match) -> String {
        found
            .positions
            .iter()
            .map(|offset| candidate[*offset..].chars().next().unwrap())
            .collect()
    }

    #[test]
    fn empty_query_matches_everything_with_score_zero() {
        for candidate in ["", "anything", "src/main.rs"] {
            assert_eq!(
                fuzzy_match("", candidate),
                Some(Match {
                    score: 0,
                    positions: vec![]
                })
            );
        }
    }

    #[test]
    fn matching_is_a_case_insensitive_subsequence() {
        assert!(fuzzy_match("gpt", "GPT-5.6 Luna").is_some());
        assert!(fuzzy_match("GL", "gpt-5.6 luna").is_some());
        assert!(fuzzy_match("lg", "gpt-5.6 luna").is_none());
        assert!(fuzzy_match("gptx", "gpt-5.6 luna").is_none());
        assert!(fuzzy_match("a", "").is_none());
        // A query longer than the candidate cannot match.
        assert!(fuzzy_match("abcd", "abc").is_none());
    }

    #[test]
    fn positions_are_ascending_byte_offsets_of_the_matched_characters() {
        let candidate = "héllo wörld";
        let found = matched("hw", candidate);
        assert_eq!(found.positions, vec![0, 7]);
        let found = matched("öd", candidate);
        assert_eq!(text(candidate, &found), "öd");
        assert!(
            found
                .positions
                .iter()
                .all(|offset| candidate.is_char_boundary(*offset))
        );
        let found = matched("ÉL", candidate);
        assert_eq!(text(candidate, &found), "él");
        let found = matched("lol", "hello world");
        assert!(found.positions.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn consecutive_runs_beat_scattered_matches() {
        let run = matched("set", "settings");
        let scattered = matched("set", "save default");
        assert!(run.score > scattered.score);
        // The alignment prefers the run even when an earlier scattered one exists.
        let found = matched("thread", "the red thread");
        assert_eq!(found.positions, vec![8, 9, 10, 11, 12, 13]);
    }

    #[test]
    fn word_starts_beat_matches_inside_words() {
        let starts = matched("nt", "New Thread");
        let inside = matched("nt", "Environment");
        assert!(starts.score > inside.score);
        assert_eq!(starts.positions, vec![0, 4]);
        let camel = matched("tf", "TextField");
        let flat = matched("tf", "textfield");
        assert!(camel.score > flat.score);
        assert_eq!(matched("g5", "gpt5").positions, vec![0, 3]);
    }

    #[test]
    fn path_segment_starts_and_file_names_score_higher() {
        let segment = matched("pm", "crates/plait/main.rs");
        let inside = matched("pm", "crates/uplift/gamma.rs");
        assert!(segment.score > inside.score);
        // The same letters in the file name outrank the directory.
        let name = matched("mod", "ui/mod.rs");
        let directory = matched("mod", "mod/ui.rs");
        assert!(name.score > directory.score);
        // When both are possible, the alignment lands in the file name.
        let found = matched("main", "main/src/main.rs");
        assert_eq!(found.positions, vec![9, 10, 11, 12]);
        // Windows separators count as well.
        assert_eq!(matched("m", "src\\main.rs").positions, vec![4]);
    }

    #[test]
    fn exact_case_breaks_ties() {
        assert!(matched("Plait", "Plait").score > matched("plait", "Plait").score);
    }

    #[test]
    fn reported_score_is_the_score_of_the_reported_alignment() {
        for (query, candidate) in [
            ("op", "openrouter/anthropic/claude-opus"),
            ("claude", "openrouter/anthropic/claude-opus"),
            ("aco", "openrouter/anthropic/claude-opus"),
            ("ss", "mississippi"),
        ] {
            let found = matched(query, candidate);
            let needle = query.chars().collect::<Vec<_>>();
            let hay = cells(candidate);
            let indices = found
                .positions
                .iter()
                .map(|offset| hay.iter().position(|cell| cell.offset == *offset).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(found.score, score(&needle, &hay, &indices));
            // No other alignment of the leftmost kind scores higher.
            let leftmost = greedy(&needle, &hay).unwrap();
            assert!(found.score >= score(&needle, &hay, &leftmost));
        }
    }

    #[test]
    fn oversized_input_still_matches() {
        let candidate = format!("{}needle", "x/".repeat(40_000));
        let found = matched("needle", &candidate);
        assert_eq!(text(&candidate, &found), "needle");
        assert!(fuzzy_match("needles", &candidate).is_none());
    }
}
