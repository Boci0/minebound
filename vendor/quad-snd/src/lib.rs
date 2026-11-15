//! Loading and playing sounds.
//! Minebound's copy keeps only the Windows (WASAPI) backend; upstream has the others.

#![allow(warnings)]

#[cfg(not(windows))]
compile_error!("this copy of quad-snd only has the Windows audio backend; see vendor/quad-snd/README.md");

mod error;

pub use error::Error;

#[path = "wasapi_snd.rs"]
mod snd;

mod mixer;
pub use mixer::set_muffle;

pub use snd::{AudioContext, Playback, Sound};

pub struct PlaySoundParams {
    pub looped: bool,
    pub volume: f32,
}

impl Default for PlaySoundParams {
    fn default() -> PlaySoundParams {
        PlaySoundParams {
            looped: false,
            volume: 1.,
        }
    }
}
