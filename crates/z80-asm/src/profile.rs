//! Dialect profiles.

/// One Z80 assembler dialect, as a particular version of its reference tool
/// accepts it.
///
/// A profile names the exact tool and version it describes, because dialects
/// change between versions and the recorded verdicts come from one version.
///
/// ```
/// use lang198x_z80_asm::Profile;
///
/// let profile = Profile::PASMONEXT_0_1_3;
/// assert_eq!(profile.tool(), "PasmoNext");
/// assert_eq!(profile.version(), "0.1.3");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Profile {
    id: &'static str,
    tool: &'static str,
    version: &'static str,
}

impl Profile {
    /// PasmoNext 0.1.3, C Kirby's fork of Julian Albo's Pasmo with the ZX
    /// Spectrum Next's extra instructions. The 198x curriculum builds with it,
    /// and Asm198x's verdicts for its `pasmo` and `pasmonext` dialects were
    /// recorded with it.
    pub const PASMONEXT_0_1_3: Self = Self {
        id: "pasmonext-0.1.3",
        tool: "PasmoNext",
        version: "0.1.3",
    };

    /// Returns the profile's identifier, such as `pasmonext-0.1.3`.
    #[must_use]
    pub const fn id(self) -> &'static str {
        self.id
    }

    /// Returns the name of the reference tool, such as `PasmoNext`.
    #[must_use]
    pub const fn tool(self) -> &'static str {
        self.tool
    }

    /// Returns the version of the reference tool this profile describes.
    #[must_use]
    pub const fn version(self) -> &'static str {
        self.version
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasmonext_names_its_tool_and_version() {
        let profile = Profile::PASMONEXT_0_1_3;
        assert_eq!(profile.id(), "pasmonext-0.1.3");
        assert_eq!(profile.tool(), "PasmoNext");
        assert_eq!(profile.version(), "0.1.3");
        assert_eq!(
            format!("{profile:?}"),
            "Profile { id: \"pasmonext-0.1.3\", tool: \"PasmoNext\", version: \"0.1.3\" }"
        );
    }
}
