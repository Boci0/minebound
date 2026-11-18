//! Recorded sound effects (Kenney, and horror hits by psychhead_; all CC0), built into the exe.
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
    /// Spark skills
    Zap,
    /// Gale skills
    Whoosh,
    /// an upgraded skill charging up
    Charge,
    LevelUp,
    /// the blast under a mine going off
    Boom,
}

macro_rules! takes {
    ($volume:literal, $($file:literal),+) => {
        ($volume, &[$(include_bytes!(concat!("../assets/sfx/", $file, ".ogg")) as &[u8]),+])
    };
}

/// For each `Sfx`, in order: how loud it plays (the ones you hear constantly sit further back)
/// and its recordings, as Ogg files in memory.
pub static SOUNDS: [(f32, &[&[u8]]); 17] = [
    takes!(0.35, "impactMining_000", "impactMining_001", "impactMining_002", "impactMining_003"),
    takes!(0.5, "impactGlass_light_000", "impactGlass_light_001", "impactGlass_light_002"),
    takes!(0.55, "horror-mine-1", "horror-mine-2"),
    // made with ffmpeg: a deep pop, a tone falling fast from ~500 Hz to ~70 Hz, three pitches; quieter,
    // since the Strike plays on every bolt and its tone cuts through the mix
    takes!(0.35, "strike-pop-1", "strike-pop-2", "strike-pop-3"),
    takes!(0.7, "impactGlass_heavy_000", "impactGlass_heavy_001"),
    takes!(0.5, "cloth1"),
    takes!(0.7, "impactPunch_medium_000", "impactPunch_medium_001", "impactPunch_medium_002"),
    takes!(0.6, "horror-kill-1", "horror-kill-2"),
    takes!(0.3, "click_001", "click_002"),
    takes!(0.5, "handleCoins"),
    // horror hits from "Horror Hit Soundpack 1", cut short, faded and levelled
    takes!(0.6, "horror-win"),
    takes!(0.6, "horror-lose"),
    // Kenney's Digital SFX set, cut short and converted to 44.1 kHz Ogg
    takes!(0.3, "digital-spark-zap"),
    takes!(0.3, "digital-gale-whoosh", "digital-gale-whoosh-2"),
    takes!(0.3, "digital-charge"),
    takes!(0.5, "horror-rite"),
    // made with ffmpeg: a brown-noise rumble over a low sine thump, both dying away fast
    takes!(0.8, "boom-1", "boom-2"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_has_at_least_one_ogg_take() {
        assert_eq!(SOUNDS.len(), Sfx::Boom as usize + 1);
        for (volume, takes) in SOUNDS {
            assert!(!takes.is_empty() && takes.iter().all(|t| &t[..4] == b"OggS"));
            assert!(volume > 0.0 && volume <= 1.0);
        }
    }
}
