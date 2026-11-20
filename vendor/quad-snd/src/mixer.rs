use crate::{AudioContext, PlaySoundParams};

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::sync::mpsc;

/// minebound patch: how muffled everything sounds, 0.0 (clear) to 1.0 (heavily low-passed), as f32 bits
static MUFFLE: AtomicU32 = AtomicU32::new(0);

/// Muffles the whole mix, like hearing it through a wall: 0.0 is clear, 1.0 is heaviest.
pub fn set_muffle(amount: f32) {
    MUFFLE.store(amount.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
}

/// minebound patch: files bigger than this (music) are decoded a little at a time while they play,
/// instead of whole into memory (a 5-minute track is ~100 MB of samples, and decoding it peaked higher)
const STREAM_BYTES: usize = 1 << 20;
/// samples per streamed chunk (about 0.1 s of stereo), and how many chunks the decoder runs ahead
const CHUNK: usize = 8192;
const AHEAD: usize = 8;

enum AudioMessage {
    AddSound(u32, Arc<[f32]>),
    /// sound id, play id, volume, and the chunks its decoder thread sends
    PlayStream(u32, u32, f32, mpsc::Receiver<Arc<[f32]>>),
    Play(u32, u32, bool, f32),
    Stop(u32),
    StopAll(u32),
    SetVolume(u32, f32),
    SetVolumeAll(u32, f32),
    Delete(u32),
}

#[derive(Debug)]
pub struct SoundState {
    sound_id: u32,
    play_id: u32,
    sample: usize,
    data: Arc<[f32]>,
    looped: bool,
    volume: f32,
    /// a streamed sound: `data` is its current chunk, the rest arrive here
    stream: Option<mpsc::Receiver<Arc<[f32]>>>,
}

impl SoundState {
    fn get_samples(&mut self, n: usize) -> &[f32] {
        // a stream waiting on its decoder has read past its chunk
        let data = &self.data[self.sample.min(self.data.len())..];

        self.sample += n;

        match data.get(..n) {
            Some(data) => data,
            None => data,
        }
    }

    fn rewind(&mut self) {
        self.sample = 0;
    }
}

pub struct Mixer {
    rx: mpsc::Receiver<AudioMessage>,
    sounds: HashMap<u32, Arc<[f32]>>,
    mixer_state: Vec<SoundState>,
    /// plays of sounds still decoding; they start when the samples arrive
    waiting: Vec<SoundState>,
    /// low-pass filter memory, left and right
    low: [f32; 2],
}

pub struct MixerBuilder {
    rx: mpsc::Receiver<AudioMessage>,
}

pub struct MixerControl {
    tx: mpsc::Sender<AudioMessage>,
    sound_id: Cell<u32>,
    play_id: Cell<u32>,
    /// the encoded bytes of sounds that stream
    streams: RefCell<HashMap<u32, Arc<[u8]>>>,
}

pub struct Playback {
    play_id: u32,
}

impl Playback {
    pub fn stop(self, ctx: &AudioContext) {
        ctx.mixer_ctrl.send(AudioMessage::Stop(self.play_id));
    }

    pub fn set_volume(&self, ctx: &AudioContext, volume: f32) {
        ctx.mixer_ctrl
            .send(AudioMessage::SetVolume(self.play_id, volume));
    }
}

impl MixerControl {
    pub fn load(&self, data: &[u8]) -> u32 {
        let sound_id = self.sound_id.get();
        self.sound_id.set(sound_id + 1);

        let rate = || audrey::Reader::new(std::io::Cursor::new(data)).map(|r| r.description().sample_rate());
        if data.len() > STREAM_BYTES && rate().ok() == Some(44100) {
            self.streams.borrow_mut().insert(sound_id, data.into());
            return sound_id;
        }

        // minebound patch: decode off the calling thread (a long Ogg froze the game for ~0.4 s), on one
        // worker that takes the sounds in turn rather than a thread per sound
        type Job = (u32, Vec<u8>, mpsc::Sender<AudioMessage>);
        static DECODER: std::sync::OnceLock<std::sync::Mutex<mpsc::Sender<Job>>> = std::sync::OnceLock::new();
        let decoder = DECODER.get_or_init(|| {
            let (jobs, rx) = mpsc::channel::<Job>();
            std::thread::spawn(move || {
                for (id, data, tx) in rx {
                    match load_samples_from_file(&data) {
                        Ok(samples) => tx.send(AudioMessage::AddSound(id, samples.into())).unwrap_or_else(|_| println!("Audio thread died")),
                        Err(()) => println!("quad-snd: could not decode sound {id}"),
                    }
                }
            });
            std::sync::Mutex::new(jobs)
        });
        let _ = decoder.lock().unwrap().send((sound_id, data.to_vec(), self.tx.clone()));

        sound_id
    }

    pub fn play(&self, sound_id: u32, params: PlaySoundParams) -> Playback {
        let play_id = self.play_id.get();

        if let Some(bytes) = self.streams.borrow().get(&sound_id).cloned() {
            let (tx, rx) = mpsc::sync_channel(AHEAD);
            let looped = params.looped;
            std::thread::spawn(move || stream(&bytes, looped, tx));
            self.send(AudioMessage::PlayStream(sound_id, play_id, params.volume, rx));
            self.play_id.set(play_id + 1);
            return Playback { play_id };
        }

        self.send(AudioMessage::Play(
            sound_id,
            play_id,
            params.looped,
            params.volume,
        ));

        self.play_id.set(play_id + 1);

        Playback { play_id }
    }

    pub fn stop(&self, play_id: u32) {
        self.send(AudioMessage::Stop(play_id));
    }

    pub fn stop_all(&self, sound_id: u32) {
        self.send(AudioMessage::StopAll(sound_id));
    }

    pub fn set_volume_all(&self, sound_id: u32, volume: f32) {
        self.send(AudioMessage::SetVolumeAll(sound_id, volume));
    }

    pub fn delete(&self, sound_id: u32) {
        self.streams.borrow_mut().remove(&sound_id);
        self.send(AudioMessage::Delete(sound_id));
    }

    fn send(&self, message: AudioMessage) {
        self.tx
            .send(message)
            .unwrap_or_else(|_| println!("Audio thread died"))
    }
}

impl MixerBuilder {
    pub fn build(self) -> Mixer {
        Mixer {
            rx: self.rx,
            sounds: HashMap::new(),
            mixer_state: vec![],
            waiting: vec![],
            low: [0.0; 2],
        }
    }
}

impl Mixer {
    pub fn new() -> (MixerBuilder, MixerControl) {
        let (tx, rx) = mpsc::channel();

        (
            MixerBuilder { rx },
            MixerControl {
                tx,
                sound_id: Cell::new(0),
                play_id: Cell::new(0),
                streams: RefCell::new(HashMap::new()),
            },
        )
    }

    pub fn fill_audio_buffer(&mut self, buffer: &mut [f32], frames: usize) {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                AudioMessage::AddSound(id, data) => {
                    for mut s in self.waiting.extract_if(.., |s| s.sound_id == id) {
                        s.data = data.clone();
                        self.mixer_state.push(s);
                    }
                    self.sounds.insert(id, data);
                }
                AudioMessage::Play(sound_id, play_id, looped, volume) => {
                    let data = self.sounds.get(&sound_id).cloned();
                    let state = SoundState {
                        sound_id,
                        play_id,
                        sample: 0,
                        data: data.clone().unwrap_or_else(|| Arc::new([])),
                        looped,
                        volume,
                        stream: None,
                    };
                    match data {
                        Some(_) => self.mixer_state.push(state),
                        None => self.waiting.push(state),
                    }
                }
                AudioMessage::PlayStream(sound_id, play_id, volume, rx) => {
                    let data = Arc::new([]);
                    let stream = Some(rx);
                    self.mixer_state.push(SoundState { sound_id, play_id, sample: 0, data, looped: false, volume, stream });
                }
                // the rest apply to playing and waiting sounds alike
                AudioMessage::Stop(play_id) => {
                    self.mixer_state.retain(|s| s.play_id != play_id);
                    self.waiting.retain(|s| s.play_id != play_id);
                }
                AudioMessage::StopAll(sound_id) => {
                    self.mixer_state.retain(|s| s.sound_id != sound_id);
                    self.waiting.retain(|s| s.sound_id != sound_id);
                }
                AudioMessage::SetVolume(play_id, volume) => {
                    for s in self.mixer_state.iter_mut().chain(&mut self.waiting) {
                        if s.play_id == play_id {
                            s.volume = volume;
                        }
                    }
                }
                AudioMessage::SetVolumeAll(sound_id, volume) => {
                    for s in self.mixer_state.iter_mut().chain(&mut self.waiting) {
                        if s.sound_id == sound_id {
                            s.volume = volume;
                        }
                    }
                }
                AudioMessage::Delete(sound_id) => {
                    self.mixer_state.retain(|s| s.sound_id != sound_id);
                    self.waiting.retain(|s| s.sound_id != sound_id);
                    self.sounds.remove(&sound_id);
                }
            }
        }

        // zeroize the buffer
        buffer.fill(0.0);

        // Note: Doing manual iteration so we can remove sounds that finished playing
        let mut i = 0;

        while let Some(sound) = self.mixer_state.get_mut(i) {
            let volume = sound.volume;
            let mut remainder = buffer.len();

            loop {
                let samples = sound.get_samples(remainder);

                for (b, s) in buffer.iter_mut().zip(samples) {
                    *b += s * volume;
                }

                remainder -= samples.len();

                if remainder > 0 {
                    match sound.stream.as_ref().map(|rx| rx.try_recv()) {
                        Some(Ok(chunk)) => {
                            sound.data = chunk;
                            sound.sample = 0;
                            continue;
                        }
                        // the decoder is behind: a moment of silence, not the end
                        Some(Err(mpsc::TryRecvError::Empty)) => remainder = 0,
                        Some(Err(mpsc::TryRecvError::Disconnected)) | None => {}
                    }
                }

                if remainder > 0 && sound.looped {
                    sound.rewind();
                    continue;
                }

                break;
            }

            if remainder > 0 {
                self.mixer_state.swap_remove(i);
            } else {
                i += 1;
            }
        }

        // one-pole low-pass on the interleaved stereo mix; at full muffle the corner is about 500 Hz
        let muffle = f32::from_bits(MUFFLE.load(Ordering::Relaxed));
        if muffle > 0.0 {
            let k = 1.0 - 0.93 * muffle;
            for frame in buffer.chunks_exact_mut(2) {
                for (s, low) in frame.iter_mut().zip(&mut self.low) {
                    *low += (*s - *low) * k;
                    *s = *low;
                }
            }
        } else {
            self.low = [0.0; 2];
        }
    }
}

/// Decodes `bytes` (44.1 kHz) into stereo chunks for one play of a streamed sound, from the start
/// again if `looped`. Ends when the sound ends or the mixer drops the play.
fn stream(bytes: &[u8], looped: bool, tx: mpsc::SyncSender<Arc<[f32]>>) {
    loop {
        let Ok(mut reader) = audrey::Reader::new(std::io::Cursor::new(bytes)) else { return };
        let mono = reader.description().channel_count() == 1;
        let mut chunk = Vec::with_capacity(CHUNK + 1);
        for sample in reader.samples::<f32>() {
            let Ok(sample) = sample else { break };
            chunk.push(sample);
            if mono {
                chunk.push(sample);
            }
            if chunk.len() >= CHUNK && tx.send(std::mem::replace(&mut chunk, Vec::with_capacity(CHUNK + 1)).into()).is_err() {
                return;
            }
        }
        if !chunk.is_empty() && tx.send(chunk.into()).is_err() || !looped {
            return;
        }
    }
}

/// Parse ogg/wav/etc and get  resampled to 44100, 2 channel data
pub fn load_samples_from_file(bytes: &[u8]) -> Result<Vec<f32>, ()> {
    let mut audio_stream = {
        let file = std::io::Cursor::new(bytes);
        audrey::Reader::new(file).unwrap()
    };

    let description = audio_stream.description();
    let channels_count = description.channel_count();
    assert!(channels_count == 1 || channels_count == 2);

    let mut frames: Vec<f32> = Vec::with_capacity(4096);
    let mut samples_iterator = audio_stream
        .samples::<f32>()
        .map(std::result::Result::unwrap);

    // audrey's frame docs: "TODO: Should consider changing this behaviour to check the audio file's actual number of channels and automatically convert to F's number of channels while reading".
    // lets fix this TODO here
    if channels_count == 1 {
        frames.extend(samples_iterator.flat_map(|sample| [sample, sample]));
    } else if channels_count == 2 {
        frames.extend(samples_iterator);
    }

    let sample_rate = description.sample_rate();

    // stupid nearest-neighbor resampler
    if sample_rate != 44100 {
        let mut new_length = ((44100 as f32 / sample_rate as f32) * frames.len() as f32) as usize;

        // `new_length` must be an even number
        new_length -= new_length % 2;

        let mut resampled = vec![0.0; new_length];

        for (n, sample) in resampled.chunks_exact_mut(2).enumerate() {
            let ix = 2 * ((n as f32 / new_length as f32) * frames.len() as f32) as usize;
            sample[0] = frames[ix];
            sample[1] = frames[ix + 1];
        }
        return Ok(resampled);
    }

    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16-bit stereo 44.1 kHz WAV, every sample at half scale
    fn wav(frames: u32) -> Vec<u8> {
        let len = frames * 4;
        let mut b = Vec::new();
        for part in [&b"RIFF"[..], &(36 + len).to_le_bytes(), b"WAVEfmt ", &16u32.to_le_bytes(), &1u16.to_le_bytes(),
            &2u16.to_le_bytes(), &44100u32.to_le_bytes(), &(44100u32 * 4).to_le_bytes(), &4u16.to_le_bytes(),
            &16u16.to_le_bytes(), b"data", &len.to_le_bytes()] {
            b.extend_from_slice(part);
        }
        for _ in 0..frames * 2 {
            b.extend_from_slice(&16384i16.to_le_bytes());
        }
        b
    }

    #[test]
    fn play_sent_before_decode_finishes_still_plays() {
        let (builder, ctl) = Mixer::new();
        let mut mixer = builder.build();
        // play is queued before the decode even starts
        ctl.play(ctl.sound_id.get(), PlaySoundParams { looped: false, volume: 1.0 });
        ctl.load(&wav(64));
        let mut buf = [0.0f32; 4];
        let start = std::time::Instant::now();
        while buf[0] == 0.0 && start.elapsed().as_secs() < 5 {
            mixer.fill_audio_buffer(&mut buf, 2);
        }
        assert_eq!(buf, [0.5; 4]);
        assert!(mixer.waiting.is_empty());
    }

    #[test]
    fn a_big_sound_streams_through_to_its_end() {
        let (builder, ctl) = Mixer::new();
        let mut mixer = builder.build();
        let frames = STREAM_BYTES / 4 + 1000;
        let id = ctl.load(&wav(frames as u32));
        assert!(ctl.streams.borrow().contains_key(&id), "a file this big streams");
        ctl.play(id, PlaySoundParams { looped: false, volume: 1.0 });
        let (mut heard, mut buf) = (0, [0.0f32; 1024]);
        let start = std::time::Instant::now();
        while (heard == 0 || !mixer.mixer_state.is_empty()) && start.elapsed().as_secs() < 10 {
            mixer.fill_audio_buffer(&mut buf, 512);
            assert!(buf.iter().all(|&s| s == 0.5 || s == 0.0), "only the sound, or a gap while it decodes");
            heard += buf.iter().filter(|&&s| s == 0.5).count();
        }
        assert_eq!(heard, frames * 2, "every sample played once");
        assert!(mixer.mixer_state.is_empty(), "and then it ended");
    }

    #[test]
    fn muffle_smooths_a_jump_and_off_passes_it_through() {
        let (builder, _ctl) = Mixer::new();
        let mut mixer = builder.build();
        mixer.mixer_state.push(SoundState { sound_id: 0, play_id: 0, sample: 0, data: Arc::new([1.0; 8]), looped: false, volume: 1.0, stream: None });
        set_muffle(1.0);
        let mut buf = [0.0f32; 4];
        mixer.fill_audio_buffer(&mut buf, 2);
        assert!(buf[0] < 0.1 && buf[2] > buf[0], "a step rises slowly when muffled: {buf:?}");
        set_muffle(0.0);
        mixer.fill_audio_buffer(&mut buf, 2);
        assert_eq!(buf, [1.0; 4]);
    }
}
