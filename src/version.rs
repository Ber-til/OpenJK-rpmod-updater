//! OpenJK-rpmod's version tags: "v0.7.3" for a release, "v0.7.4-dev-g5cc1ce8" for a build made
//! between releases, named after the release it leads to.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub nums: [u32; 3],
    /// a build between releases: older than the release it names
    pub dev: bool,
    pub text: String,
}

impl Version {
    pub fn parse(tag: &str) -> Option<Version> {
        let text = tag.trim().to_string();
        let mut rest = text.strip_prefix(['v', 'V']).unwrap_or(&text);
        let mut nums = [0u32; 3];

        for (i, num) in nums.iter_mut().enumerate() {
            let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
            if end == 0 {
                return None;
            }
            *num = rest[..end].parse().ok()?;
            rest = &rest[end..];
            if i < 2 {
                rest = rest.strip_prefix('.')?;
            }
        }
        let dev = !rest.is_empty();
        Some(Version { nums, dev, text })
    }

    /// whether `other`, a release, came out after this
    pub fn is_older_than(&self, other: &Version) -> bool {
        if self.nums != other.nums {
            return self.nums < other.nums;
        }
        self.dev && !other.dev
    }

    /// whether this is a build towards a release not out yet
    pub fn is_newer_than(&self, other: &Version) -> bool {
        self.nums > other.nums
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn parses() {
        assert_eq!(v("v0.7.3").nums, [0, 7, 3]);
        assert!(!v("v0.7.3").dev);
        assert!(v("v0.7.4-dev-g5cc1ce8").dev);
        assert!(Version::parse("vUNKNOWN").is_none());
        assert!(Version::parse("v0.7").is_none());
    }

    #[test]
    fn orders() {
        assert!(v("v0.7.2").is_older_than(&v("v0.7.3")));
        assert!(!v("v0.7.3").is_older_than(&v("v0.7.3")));
        assert!(v("v0.7.3-dev-gabc").is_older_than(&v("v0.7.3")));
        assert!(!v("v0.7.4-dev-gabc").is_older_than(&v("v0.7.3")));
        assert!(v("v0.7.4-dev-gabc").is_newer_than(&v("v0.7.3")));
        assert!(v("v0.10.0").is_newer_than(&v("v0.9.9")));
    }
}
