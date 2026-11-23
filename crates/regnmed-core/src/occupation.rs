//! SSB's occupation catalogue (Klass 145, STYRK-08), searched so nobody
//! has to know a seven-digit a-melding code by heart. The list is a
//! suggestion source only: the a-melding still checks the code's format.

/// `code;name`, one per line. Reduced from SSB's export (docs/lonn.md).
const CATALOGUE: &str = include_str!("../../../docs/lonn/yrkeskoder-ssb-145.csv");

/// Digits match the start of a code. Text matches every word anywhere in
/// the name, with names starting on the first word first, shortest first.
pub fn search(query: &str, limit: usize) -> Vec<(&'static str, &'static str)> {
    let query = query.trim().to_uppercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let Some(first) = words.first() else {
        return Vec::new();
    };
    let entries = CATALOGUE.lines().filter_map(|line| line.split_once(';'));
    if query.bytes().all(|b| b.is_ascii_digit()) {
        return entries
            .filter(|(code, _)| code.starts_with(&query))
            .take(limit)
            .collect();
    }
    let mut hits: Vec<_> = entries
        .filter(|(_, name)| words.iter().all(|word| name.contains(word)))
        .collect();
    hits.sort_by_key(|(_, name)| (!name.starts_with(first), name.len()));
    hits.truncate(limit);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_is_complete_and_well_formed() {
        assert_eq!(CATALOGUE.lines().count(), 7130);
        assert!(CATALOGUE.lines().all(|line| {
            line.split_once(';').is_some_and(|(code, name)| {
                code.len() == 7 && code.bytes().all(|b| b.is_ascii_digit()) && !name.is_empty()
            })
        }));
    }

    #[test]
    fn a_code_or_a_title_finds_the_occupation() {
        assert_eq!(search("2512101", 5), [("2512101", "FORHANDLINGSKONSULENT")]);
        assert_eq!(search(" forhandlingskonsulent ", 1)[0].0, "2512101");
        assert_eq!(search("radiograf", 1)[0], ("3221103", "RADIOGRAF"));
    }

    #[test]
    fn a_blank_or_unknown_search_finds_nothing_and_the_limit_holds() {
        assert!(search("   ", 5).is_empty());
        assert!(search("xyzzy", 5).is_empty());
        assert_eq!(search("konsulent", 3).len(), 3);
    }
}
