//! UE3 configuration files: `BasedOn=` chains and `+`/`-`/`.`/`!` line prefixes (plan/12-tasks.md T-032b).
//! Checked against the Willow*.ini files the original wrote on first launch (plan/03-evidence.md E-057).

use std::io::{Error, ErrorKind, Result};
use std::path::Path;

/// Sections and their key/value lines in first-seen order; a key may repeat (UE3 multimap).
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Ini {
    pub sections: Vec<(String, Vec<(String, String)>)>,
}

impl Ini {
    pub fn section(&self, name: &str) -> Option<&Vec<(String, String)>> {
        self.sections.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v)
    }

    fn section_mut(&mut self, name: &str) -> &mut Vec<(String, String)> {
        let i = match self.sections.iter().position(|(n, _)| n.eq_ignore_ascii_case(name)) {
            Some(i) => i,
            None => {
                self.sections.push((name.to_string(), Vec::new()));
                self.sections.len() - 1
            }
        };
        &mut self.sections[i].1
    }

    /// Values of `key` in `section`, in order.
    pub fn values(&self, section: &str, key: &str) -> Vec<&str> {
        self.section(section).map_or_else(Vec::new, |s| {
            s.iter().filter(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str()).collect()
        })
    }

    /// Applies one file's text on top of `self` with UE3 semantics (rules verified in E-057):
    /// a plain key replaces all its values unless that exact pair already exists, `+` adds if absent,
    /// `-` removes the pair, `.` always adds,
    /// `!` clears the key; lines without `=` and keys starting with `;` are comments; one pair of
    /// surrounding quotes is removed.
    pub fn apply(&mut self, text: &str) {
        self.read(text, false);
    }

    /// Reads a file line by line keeping every value: the game's own output files and the root
    /// (`Base*.ini`) of a BasedOn chain, where repeated plain keys are all kept (E-057).
    pub fn parse_generated(text: &str) -> Ini {
        let mut ini = Ini::default();
        ini.read(text, true);
        ini
    }

    fn read(&mut self, text: &str, keep_all: bool) {
        let mut current: Option<String> = None;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                current = Some(line[1..line.len() - 1].to_string());
                continue;
            }
            let Some(sec) = current.as_deref() else { continue };
            let (op, rest) = match line.as_bytes()[0] {
                b'+' | b'-' | b'.' | b'!' => (line.as_bytes()[0], &line[1..]),
                _ => (b'=', line),
            };
            let (key, value) = match rest.split_once('=') {
                Some((k, v)) => (k.trim(), v.trim()),
                None if op == b'!' => (rest.trim(), ""),
                None => continue,
            };
            if key.starts_with(';') {
                continue;
            }
            let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).unwrap_or(value);
            let s = self.section_mut(sec);
            let same_key = |k: &String| k.eq_ignore_ascii_case(key);
            let entry = (key.to_string(), value.to_string());
            match op {
                _ if keep_all => s.push(entry),
                b'+' => {
                    if !s.iter().any(|(k, v)| same_key(k) && v == value) {
                        s.push(entry);
                    }
                }
                b'-' => s.retain(|(k, v)| !(same_key(k) && v == value)),
                b'.' => s.push(entry),
                b'!' => s.retain(|(k, _)| !same_key(k)),
                // Plain key: no-op if this exact pair exists, otherwise replaces the key's values.
                _ if s.iter().any(|(k, v)| same_key(k) && v == value) => {}
                _ => match s.iter().position(|(k, _)| same_key(k)) {
                    Some(i) => {
                        s.retain(|(k, _)| !same_key(k));
                        s.insert(i.min(s.len()), entry);
                    }
                    None => s.push(entry),
                },
            }
        }
    }

    /// Replaces `%GAME%` the way the shipped game does (generated files say `WillowGame`).
    pub fn expand_game(&mut self) {
        for (_, lines) in &mut self.sections {
            for (_, v) in lines.iter_mut() {
                if v.contains("%GAME%") {
                    *v = v.replace("%GAME%", "Willow");
                }
            }
        }
    }

    /// Loads `path` with its `[Configuration] BasedOn=` chain (relative to the file's directory's parent,
    /// as in the shipped configs: `..\Engine\Config\BaseEngine.ini` from `WillowGame\Config`).
    pub fn load(path: &Path) -> Result<Ini> {
        let mut ini = Self::load_chain(path)?;
        ini.sections.retain(|(n, _)| !n.eq_ignore_ascii_case("Configuration"));
        ini.expand_game();
        Ok(ini)
    }

    fn load_chain(path: &Path) -> Result<Ini> {
        let text = std::fs::read_to_string(path)?;
        let mut base = Ini::default();
        base.apply(&text);
        let based_on = base.values("Configuration", "BasedOn").first().map(|s| s.replace('\\', "/"));
        // The root of a BasedOn chain is read as a plain multimap; derived files apply with prefix semantics.
        let ini = match based_on {
            Some(rel) => {
                let dir = path.parent().and_then(Path::parent).ok_or_else(|| Error::new(ErrorKind::InvalidInput, "config path has no parent"))?;
                let mut ini = Ini::load_chain(&dir.join(rel))?;
                ini.apply(&text);
                ini
            }
            None => Ini::parse_generated(&text),
        };
        Ok(ini)
    }
}

#[cfg(test)]
mod tests {
    use super::Ini;

    #[test]
    fn prefixes_follow_ue3_rules() {
        let mut ini = Ini::default();
        ini.apply("[S]\nA=1\nA=1b\n+L=x\n+L=y\nP=keep\n# note\n+;C=1\nQ=\"quoted\"\n");
        assert_eq!(ini.values("S", "A"), ["1b"]);
        ini.apply("[S]\nA=2\n+L=x\n-L=y\n.L=x\n!P\n");
        assert_eq!(ini.values("S", "A"), ["2"]);
        assert_eq!(Ini::parse_generated("[S]\nA=1\nA=2\n").values("S", "A"), ["1", "2"]);
        let mut keep = Ini::parse_generated("[S]\nK=a\nK=b\n");
        keep.apply("[S]\nK=a\nK=b\n");
        assert_eq!(keep.values("S", "K"), ["a", "b"]);
        assert_eq!(ini.values("S", "L"), ["x", "x"]);
        assert!(ini.values("S", "P").is_empty());
        assert!(ini.values("S", "# note").is_empty() && ini.values("S", ";C").is_empty());
        assert_eq!(ini.values("S", "Q"), ["quoted"]);
    }
}
