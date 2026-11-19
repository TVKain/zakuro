//! the versions a game shows on its title screen, "Ver. 1.2", against the
//! version its update's TMD holds, v2080. a TMD only has the second, and how
//! a game's updates number it is up to the game, so there is no working one
//! out from the other: these are the games whose updates are known.

/// for each game, by the low half of its title id, which a game, its update
/// and its DLC share: the version the game shows and the TMD's, an update's.
const KNOWN: &[(u32, &[(&str, u16)])] = &[
    // Pokémon X and Y
    (0x0005_5D00, X_Y),
    (0x0005_5E00, X_Y),
    // Pokémon Omega Ruby and Alpha Sapphire
    (0x0011_C400, OMEGA_RUBY_ALPHA_SAPPHIRE),
    (0x0011_C500, OMEGA_RUBY_ALPHA_SAPPHIRE),
    // Pokémon Sun and Moon
    (0x0016_4800, SUN_MOON),
    (0x0017_5E00, SUN_MOON),
    // Pokémon Ultra Sun and Ultra Moon
    (0x001B_5000, SUN_MOON),
    (0x001B_5100, SUN_MOON),
];

const X_Y: &[(&str, u16)] = &[("1.5", 5216)];
const OMEGA_RUBY_ALPHA_SAPPHIRE: &[(&str, u16)] = &[("1.4", 7232)];
const SUN_MOON: &[(&str, u16)] = &[("1.1", 1040), ("1.2", 2080)];

/// the game's known versions, what it shows and its update's TMD version,
/// none for a game not known.
pub fn known(title_id: u64) -> &'static [(&'static str, u16)] {
    KNOWN.iter().find(|&&(game, _)| game == title_id as u32).map_or(&[], |&(_, versions)| versions)
}

/// the version a game's update shows, "1.2", if it is known.
pub fn shown(title_id: u64, version: u16) -> Option<&'static str> {
    known(title_id).iter().find(|&&(_, tmd)| tmd == version).map(|&(shown, _)| shown)
}

/// the TMD version of a game's update that shows as shown, "1.2".
pub fn tmd(title_id: u64, shown: &str) -> Option<u16> {
    known(title_id).iter().find(|&&(name, _)| name == shown).map(|&(_, tmd)| tmd)
}

/// an update's version for a person: what the game calls it with the TMD's
/// number, "1.2 (v2080)", or the number alone for a game not known.
pub fn label(title_id: u64, version: u16) -> String {
    match shown(title_id, version) {
        Some(shown) => format!("{shown} (v{version})"),
        None => format!("v{version}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_update_shows_the_version_its_game_calls_it() {
        // Ultra Sun's update, its own title id rather than the game's
        assert_eq!(label(0x0004_000E_001B_5000, 2080), "1.2 (v2080)");
        assert_eq!(label(0x0004_0000_0005_5D00, 5216), "1.5 (v5216)");
        assert_eq!(tmd(0x0004_0000_0011_C500, "1.4"), Some(7232));
    }

    #[test]
    fn a_version_not_known_shows_its_number() {
        assert_eq!(label(0x0004_000E_001B_5000, 3120), "v3120");
        assert_eq!(label(0x0004_000E_0000_1234, 1040), "v1040");
        assert_eq!(tmd(0x0004_0000_0000_1234, "1.1"), None);
    }
}
