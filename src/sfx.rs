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

macro_rules! takes {
    ($volume:literal, $($file:literal),+) => {
        ($volume, &[$(include_bytes!(concat!("../assets/sfx/", $file, ".ogg")) as &[u8]),+])
    };
}

/// For each `Sfx`, in order: how loud it plays (the ones you hear constantly sit further back)
/// and its recordings, as Ogg files in memory.
pub static SOUNDS: [(f32, &[&[u8]]); 12] = [
    takes!(0.35, "impactMining_000", "impactMining_001", "impactMining_002", "impactMining_003"),
    takes!(0.5, "impactGlass_light_000", "impactGlass_light_001", "impactGlass_light_002"),
    takes!(0.7, "impactPlate_heavy_000", "impactPlate_heavy_001"),
    takes!(0.7, "knifeSlice", "knifeSlice2"),
    takes!(0.7, "impactGlass_heavy_000", "impactGlass_heavy_001"),
    takes!(0.5, "cloth1"),
    takes!(0.7, "impactPunch_medium_000", "impactPunch_medium_001", "impactPunch_medium_002"),
    takes!(0.7, "impactPunch_heavy_000", "impactPunch_heavy_001"),
    takes!(0.3, "click_001", "click_002"),
    takes!(0.5, "handleCoins"),
    takes!(0.7, "confirmation_001"),
    takes!(0.7, "error_004"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_has_at_least_one_ogg_take() {
        assert_eq!(SOUNDS.len(), Sfx::Lose as usize + 1);
        for (volume, takes) in SOUNDS {
            assert!(!takes.is_empty() && takes.iter().all(|t| &t[..4] == b"OggS"));
            assert!(volume > 0.0 && volume <= 1.0);
        }
    }
}
