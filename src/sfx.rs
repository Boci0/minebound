//! Recorded sound effects (Kenney, CC0), built into the exe.
//! Most sounds have several takes, picked at random, so repeats do not grate.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sfx {
    Flip,
    Claim,
    Mine,
    Strike,
    Skill,
    Miss,
    Hurt,
    Kill,
    Click,
    Coins,
    Win,
    Lose,
}

pub const ALL: [Sfx; 12] = [
    Sfx::Flip,
    Sfx::Claim,
    Sfx::Mine,
    Sfx::Strike,
    Sfx::Skill,
    Sfx::Miss,
    Sfx::Hurt,
    Sfx::Kill,
    Sfx::Click,
    Sfx::Coins,
    Sfx::Win,
    Sfx::Lose,
];

macro_rules! takes {
    ($($file:literal),+) => {
        &[$(include_bytes!(concat!("../assets/sfx/", $file, ".ogg")) as &[u8]),+]
    };
}

/// The recordings for a sound, as Ogg files in memory.
pub fn takes(s: Sfx) -> &'static [&'static [u8]] {
    match s {
        Sfx::Flip => takes!("impactMining_000", "impactMining_001", "impactMining_002", "impactMining_003"),
        Sfx::Claim => takes!("impactGlass_light_000", "impactGlass_light_001", "impactGlass_light_002"),
        Sfx::Mine => takes!("impactPlate_heavy_000", "impactPlate_heavy_001"),
        Sfx::Strike => takes!("knifeSlice", "knifeSlice2"),
        Sfx::Skill => takes!("impactGlass_heavy_000", "impactGlass_heavy_001"),
        Sfx::Miss => takes!("cloth1"),
        Sfx::Hurt => takes!("impactPunch_medium_000", "impactPunch_medium_001", "impactPunch_medium_002"),
        Sfx::Kill => takes!("impactPunch_heavy_000", "impactPunch_heavy_001"),
        Sfx::Click => takes!("click_001", "click_002"),
        Sfx::Coins => takes!("handleCoins"),
        Sfx::Win => takes!("confirmation_001"),
        Sfx::Lose => takes!("error_004"),
    }
}

/// How loud each sound plays: the ones you hear constantly sit further back.
pub fn volume(s: Sfx) -> f32 {
    match s {
        Sfx::Flip => 0.35,
        Sfx::Click => 0.3,
        Sfx::Claim | Sfx::Miss | Sfx::Coins => 0.5,
        _ => 0.7,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_has_at_least_one_ogg_take() {
        for s in ALL {
            assert!(!takes(s).is_empty());
            for take in takes(s) {
                assert_eq!(&take[..4], b"OggS", "{s:?}");
            }
            assert!(volume(s) > 0.0 && volume(s) <= 1.0);
        }
    }
}
