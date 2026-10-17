//! Minimal, case-insensitive path patterns for cleaning rules and
//! exclusions.
//!
//! - `*` matches any run of characters within one folder or file name,
//! - `?` matches one character,
//! - `**` as a whole segment matches any number of folders (including none),
//! - `/` and `\` both separate segments.
//!
//! There are no character classes or braces, so a pattern means exactly
//! what it looks like.

/// A compiled pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glob {
    source: String,
    segments: Vec<Segment>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    AnyDepth,
    Name(Vec<Token>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Star,
    One,
    Char(char),
}

impl Glob {
    /// Parses a pattern. Rejects empty patterns, `.`/`..` segments, and
    /// `**` mixed with other characters.
    pub fn new(pattern: &str) -> Result<Glob, String> {
        if pattern.trim().is_empty() {
            return Err("pattern is empty".into());
        }
        if pattern.len() > 260 {
            return Err("pattern is too long".into());
        }
        let mut segments = Vec::new();
        for seg in pattern.split(['/', '\\']) {
            if seg.is_empty() {
                continue;
            }
            if seg == "." || seg == ".." {
                return Err(format!("`{pattern}`: `.` and `..` are not allowed"));
            }
            if seg == "**" {
                if segments.last() != Some(&Segment::AnyDepth) {
                    segments.push(Segment::AnyDepth);
                }
                continue;
            }
            if seg.contains("**") {
                return Err(format!("`{pattern}`: `**` must be a whole segment"));
            }
            let tokens = seg
                .chars()
                .map(|c| match c {
                    '*' => Token::Star,
                    '?' => Token::One,
                    c => Token::Char(fold(c)),
                })
                .collect();
            segments.push(Segment::Name(tokens));
        }
        if segments.is_empty() {
            return Err(format!("`{pattern}` matches nothing"));
        }
        Ok(Glob {
            source: pattern.to_string(),
            segments,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Matches path components (already split). Comparison ignores case.
    pub fn matches<S: AsRef<str>>(&self, comps: &[S]) -> bool {
        let folded: Vec<Vec<char>> = comps
            .iter()
            .map(|c| c.as_ref().chars().map(fold).collect())
            .collect();
        match_segments(&self.segments, &folded)
    }

    /// Matches a path string, split on `/` and `\`.
    pub fn matches_path(&self, path: &str) -> bool {
        let comps: Vec<&str> = path.split(['/', '\\']).filter(|c| !c.is_empty()).collect();
        self.matches(&comps)
    }

    /// Whether this is a single literal name (no wildcards).
    pub fn is_literal_name(&self) -> bool {
        matches!(self.segments.as_slice(), [Segment::Name(t)] if t.iter().all(|t| matches!(t, Token::Char(_))))
    }
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn match_segments(pat: &[Segment], comps: &[Vec<char>]) -> bool {
    match pat.split_first() {
        None => comps.is_empty(),
        Some((Segment::AnyDepth, rest)) => {
            (0..=comps.len()).any(|i| match_segments(rest, &comps[i..]))
        }
        Some((Segment::Name(tokens), rest)) => match comps.split_first() {
            Some((first, others)) => match_name(tokens, first) && match_segments(rest, others),
            None => false,
        },
    }
}

/// Matches one name against `*`/`?`/literal tokens (iterative, linear-ish).
pub(crate) fn match_name_str(pattern: &str, name: &str) -> bool {
    let tokens: Vec<Token> = pattern
        .chars()
        .map(|c| match c {
            '*' => Token::Star,
            '?' => Token::One,
            c => Token::Char(fold(c)),
        })
        .collect();
    let chars: Vec<char> = name.chars().map(fold).collect();
    match_name(&tokens, &chars)
}

fn match_name(tokens: &[Token], name: &[char]) -> bool {
    let (mut t, mut n) = (0usize, 0usize);
    let (mut star_t, mut star_n) = (usize::MAX, 0usize);
    while n < name.len() {
        match tokens.get(t) {
            Some(Token::Star) => {
                star_t = t;
                star_n = n;
                t += 1;
            }
            Some(Token::One) => {
                t += 1;
                n += 1;
            }
            Some(Token::Char(c)) if *c == name[n] => {
                t += 1;
                n += 1;
            }
            _ if star_t != usize::MAX => {
                t = star_t + 1;
                star_n += 1;
                n = star_n;
            }
            _ => return false,
        }
    }
    tokens[t..].iter().all(|t| *t == Token::Star)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, path: &str) -> bool {
        Glob::new(p).unwrap().matches_path(path)
    }

    #[test]
    fn wildcards() {
        assert!(m("**", "a/b/c.txt"));
        assert!(m("*.log", "setup.LOG"));
        assert!(!m("*.log", "sub/setup.log"));
        assert!(m("**/*.log", "setup.log"));
        assert!(m("**/*.log", "a/b/setup.log"));
        assert!(m("thumbcache_*.db", "thumbcache_1024.db"));
        assert!(!m("thumbcache_*.db", "iconcache_1024.db"));
        assert!(m("a/**/z", "a/z"));
        assert!(m("a/**/z", "a/b/c/z"));
        assert!(!m("a/**/z", "b/z"));
        assert!(m("f?o", "FOO"));
        assert!(!m("f?o", "fo"));
        assert!(m(r"Cache\**", "cache/x/y"));
        assert!(m("*a*b*", "xxaYYbzz"));
        assert!(!m("*a*b*", "xxbYYazz"));
    }

    #[test]
    fn rejects_bad_patterns() {
        for p in ["", "  ", "../x", "a/./b", "a**", "**b/c"] {
            assert!(Glob::new(p).is_err(), "{p}");
        }
    }

    #[test]
    fn literal_names() {
        assert!(Glob::new("MEMORY.DMP").unwrap().is_literal_name());
        assert!(!Glob::new("*.dmp").unwrap().is_literal_name());
        assert!(!Glob::new("a/b").unwrap().is_literal_name());
    }

    #[test]
    fn name_matching_is_not_exponential() {
        let name = "a".repeat(200);
        assert!(!match_name_str("*a*a*a*a*a*a*a*a*b", &name));
    }
}
