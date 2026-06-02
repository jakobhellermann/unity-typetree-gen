// TODO(ai-review): review for style and correctness
//! Unity version parsing, mirroring AssetsTools.NET `UnityVersion` for the
//! parts the type-tree templates branch on (major/minor/patch).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnityVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl UnityVersion {
    /// Parse e.g. `6000.0.0` or `2022.3.0f1`. Each component contributes its
    /// leading run of digits; a missing component is 0.
    pub fn parse(version: &str) -> UnityVersion {
        let mut parts = version.split('.');
        UnityVersion {
            major: leading_number(parts.next().unwrap_or("")),
            minor: leading_number(parts.next().unwrap_or("")),
            patch: leading_number(parts.next().unwrap_or("")),
        }
    }

    /// Nesting depth at and beyond which collections are no longer serialized.
    pub fn serialization_limit(&self) -> i32 {
        if self.major > 2020
            || (self.major == 2020 && (self.minor > 1 || (self.minor == 1 && self.patch >= 4)))
            || (self.major == 2019 && self.minor == 4 && self.patch >= 9)
        {
            10
        } else {
            7
        }
    }
}

fn leading_number(component: &str) -> u32 {
    let digits: String = component
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().unwrap_or(0)
}
