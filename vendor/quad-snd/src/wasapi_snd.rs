// https://github.com/floooh/sokol/blob/master/sokol_audio.h
// https://github.com/norse-rs/audir/blob/master/audir/src/wasapi/mod.rs

use crate::PlaySoundParams;

pub use crate::mixer::Playback;

use winapi::shared::guiddef::{CLSID, IID};
use winapi::shared::ksmedia;
use winapi::shared::minwindef::*;
use winapi::shared::mmreg::*;
use winapi::um::audioclient::*;
use winapi::um::audiosessiontypes::*;
use winapi::um::combaseapi::*;
use winapi::um::mmdeviceapi::*;
use winapi::um::objbase::*;
use winapi::um::synchapi::*;
use winapi::um::winbase::*;

use std::sync::mpsc;

// thanks sokol_audio!
// https://github.com/floooh/sokol/blob/master/sokol_audio.h#L559
static IID_IAudioClient: IID = IID {
    Data1: 0x1cb9ad4c,
    Data2: 0xdbfa,
    Data3: 0x4c32,
    Data4: [0xb1, 0x78, 0xc2, 0xf5, 0x68, 0xa7, 0x03, 0xb2],
};
static IID_IMMDeviceEnumerator: IID = IID {
    Data1: 0xa95664d2,
    Data2: 0x9614,
    Data3: 0x4f35,
    Data4: [0xa7, 0x46, 0xde, 0x8d, 0xb6, 0x36, 0x17, 0xe6],
};
static CLSID_IMMDeviceEnumerator: CLSID = CLSID {
    Data1: 0xbcde0395,
    Data2: 0xe52f,
    Data3: 0x467c,
    Data4: [0x8e, 0x3d, 0xc4, 0x57, 0x92, 0x91, 0x69, 0x2e],
};
static IID_IAudioRenderClient: IID = IID {
    Data1: 0xf294acfc,
    Data2: 0x3146,
    Data3: 0x4483,
    Data4: [0xa7, 0xbf, 0xad, 0xdc, 0xa7, 0xc2, 0x60, 0xe2],
};
static IID_Devinterface_Audio_Render: IID = IID {
    Data1: 0xe6327cad,
    Data2: 0xdcec,
    Data3: 0x4949,
    Data4: [0xae, 0x8a, 0x99, 0x1e, 0x97, 0x6a, 0x79, 0xd2],
};
static IID_IActivateAudioInterface_Completion_Handler: IID = IID {
    Data1: 0x94ea2b94,
    Data2: 0xe9cc,
    Data3: 0x49e0,
    Data4: [0xc0, 0xff, 0xee, 0x64, 0xca, 0x8f, 0x5b, 0x90],
};

mod consts {
    pub const CHANNELS: u32 = 2;
    pub const SAMPLE_RATE: u32 = 44100;
    pub const BUFFER_FRAMES: u32 = 4096;
}

// minebound patch: the output follows the Windows default device. The original opened the default once at
// startup, so earphones connected later stayed silent, and a removed device stalled the audio thread forever.

/// An open WASAPI stream on one device; released on drop.
struct Output {
    id: Vec<u16>,
    client: *mut IAudioClient,
    render: *mut IAudioRenderClient,
    frames: u32,
}

impl Drop for Output {
    fn drop(&mut self) {
        unsafe {
            (*self.client).Stop();
            if !self.render.is_null() {
                (*self.render).Release();
            }
            (*self.client).Release();
        }
    }
}

unsafe fn device_id(device: *mut IMMDevice) -> Option<Vec<u16>> {
    let mut p: *mut u16 = std::ptr::null_mut();
    if (*device).GetId(&mut p) < 0 || p.is_null() {
        return None;
    }
    let len = (0..).take_while(|&i| *p.add(i) != 0).count();
    let id = std::slice::from_raw_parts(p, len).to_vec();
    CoTaskMemFree(p as _);
    Some(id)
}

unsafe fn default_device(enumerator: *mut IMMDeviceEnumerator) -> Option<*mut IMMDevice> {
    let mut device: *mut IMMDevice = std::ptr::null_mut();
    ((*enumerator).GetDefaultAudioEndpoint(eRender, eConsole, &mut device) >= 0).then_some(device)
}

unsafe fn default_id(enumerator: *mut IMMDeviceEnumerator) -> Option<Vec<u16>> {
    let device = default_device(enumerator)?;
    let id = device_id(device);
    (*device).Release();
    id
}

/// Opens a stream on the current default device; None if there is none or it refuses.
unsafe fn open(enumerator: *mut IMMDeviceEnumerator, event: winapi::um::winnt::HANDLE) -> Option<Output> {
    let device = default_device(enumerator)?;
    let id = device_id(device);
    let mut client: *mut IAudioClient = std::ptr::null_mut();
    let hr = match id {
        Some(_) => (*device).Activate(&IID_IAudioClient, CLSCTX_ALL, std::ptr::null_mut(), &mut client as *mut _ as _),
        None => -1,
    };
    (*device).Release();
    if hr < 0 {
        return None;
    }
    let mut out = Output { id: id?, client, render: std::ptr::null_mut(), frames: 0 };

    let format = WAVEFORMATEX {
        nChannels: consts::CHANNELS as _,
        nSamplesPerSec: consts::SAMPLE_RATE as _,
        wFormatTag: WAVE_FORMAT_EXTENSIBLE,
        wBitsPerSample: 32,
        nBlockAlign: consts::CHANNELS as u16 * 4,
        nAvgBytesPerSec: consts::CHANNELS as u32 * consts::SAMPLE_RATE as u32 * 4,
        cbSize: (std::mem::size_of::<WAVEFORMATEXTENSIBLE>() - std::mem::size_of::<WAVEFORMATEX>())
            as _,
    };

    const FRONT_LEFT: u32 = 0b0001;
    const FRONT_RIGHT: u32 = 0b0010;

    let format_extensible = WAVEFORMATEXTENSIBLE {
        Format: format,
        Samples: 4 * 8,
        dwChannelMask: FRONT_LEFT | FRONT_RIGHT,
        SubFormat: ksmedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
    };

    // https://docs.microsoft.com/en-us/windows/win32/coreaudio/audclnt-streamflags-xxx-constants
    const AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM: u32 = 0x80000000;
    const AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY: u32 = 0x08000000;

    let dur = consts::BUFFER_FRAMES as f64 / (consts::SAMPLE_RATE as f64 * 1.0 / 10000000.0);
    let hr = (*client).Initialize(
        AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_EVENTCALLBACK
            | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
            | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
        dur as _,
        0,
        &format_extensible as *const _ as _,
        std::ptr::null(),
    );
    if hr < 0 {
        println!("quad-snd: audio_client.Initialize failed: 0x{:x}", hr as u32);
        return None;
    }
    if (*client).GetBufferSize(&mut out.frames) < 0
        || (*client).GetService(&IID_IAudioRenderClient, &mut out.render as *mut _ as _) < 0
        || (*client).SetEventHandle(event) < 0
        || (*client).Start() < 0
    {
        return None;
    }
    Some(out)
}

/// Mixes one buffer's worth into the device. Err means the device is gone or broken.
unsafe fn fill(out: &Output, mixer: &mut crate::mixer::Mixer) -> Result<(), ()> {
    let mut padding = 0;
    if (*out.client).GetCurrentPadding(&mut padding) < 0 {
        return Err(());
    }
    let num_frames = out.frames - padding;
    if num_frames == 0 {
        return Ok(());
    }
    let mut wasapi_buffer: *mut u8 = std::ptr::null_mut();
    if (*out.render).GetBuffer(num_frames, &mut wasapi_buffer) < 0 || wasapi_buffer.is_null() {
        return Err(());
    }
    let buffer = std::slice::from_raw_parts_mut(
        wasapi_buffer as *mut f32,
        num_frames as usize * consts::CHANNELS as usize,
    );
    mixer.fill_audio_buffer(buffer, num_frames as _);
    (*out.render).ReleaseBuffer(num_frames, 0);
    Ok(())
}

unsafe fn audio_thread(mut mixer: crate::mixer::Mixer) {
    CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED);

    let buffer_end_event = CreateEventA(std::ptr::null_mut(), FALSE, FALSE, std::ptr::null());
    assert!(buffer_end_event.is_null() == false);

    let mut device_enumerator: *mut IMMDeviceEnumerator = std::ptr::null_mut();
    let hr = CoCreateInstance(
        &CLSID_IMMDeviceEnumerator,
        std::ptr::null_mut(),
        CLSCTX_ALL,
        &IID_IMMDeviceEnumerator,
        &mut device_enumerator as *mut _ as _,
    );
    assert!(hr >= 0, "CoCreatInstance failed");

    let mut out = open(device_enumerator, buffer_end_event);
    let mut checked = std::time::Instant::now();
    loop {
        // time out so a lost or switched device is noticed even when no buffer event comes
        WaitForSingleObject(buffer_end_event, 100);

        if checked.elapsed().as_secs_f32() >= 1.0 {
            checked = std::time::Instant::now();
            if out.as_ref().map(|o| &o.id) != default_id(device_enumerator).as_ref() {
                out = None; // release the old stream before opening the new one
                out = open(device_enumerator, buffer_end_event);
            }
        }
        if let Some(o) = &out {
            if fill(o, &mut mixer).is_err() {
                out = None; // reopened at the next check
            }
        }
    }
}

pub struct AudioContext {
    pub(crate) mixer_ctrl: crate::mixer::MixerControl,
}

impl AudioContext {
    pub fn new() -> AudioContext {
        use crate::mixer::Mixer;

        let (mixer_builder, mixer_ctrl) = Mixer::new();
        std::thread::spawn(move || unsafe {
            audio_thread(mixer_builder.build());
        });

        AudioContext { mixer_ctrl }
    }
}

pub struct Sound {
    sound_id: u32,
}

impl Sound {
    pub fn load(ctx: &AudioContext, data: &[u8]) -> Sound {
        let sound_id = ctx.mixer_ctrl.load(data);

        Sound { sound_id }
    }

    pub fn play(&self, ctx: &AudioContext, params: PlaySoundParams) -> Playback {
        ctx.mixer_ctrl.play(self.sound_id, params)
    }

    pub fn stop(&self, ctx: &AudioContext) {
        ctx.mixer_ctrl.stop_all(self.sound_id);
    }

    pub fn set_volume(&self, ctx: &AudioContext, volume: f32) {
        ctx.mixer_ctrl.set_volume_all(self.sound_id, volume);
    }

    pub fn delete(&self, ctx: &AudioContext) {
        ctx.mixer_ctrl.delete(self.sound_id);
    }
}
