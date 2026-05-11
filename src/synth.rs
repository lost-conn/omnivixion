//! Audio synthesis backend.
//!
//! Owns the cpal output stream. Receives commands from the main thread over a
//! lock-free-ish mpsc channel; the audio callback drains the channel each
//! buffer and then renders the next chunk of samples from its 4-voice state.
//!
//! Current coverage:
//! - 4 polyphonic voices
//! - 8 base waveforms (SPEC §9.2)
//! - SFX step engine with per-SFX speed + loop metadata (SPEC §9.4)
//! - sfx/audio_master command surface
//!
//! Not yet implemented (later phases):
//! - Effects (slide, vibrato, drop, fades, arpeggios — SPEC §9.3)
//! - Custom instruments (waveform values 8..15 — SPEC §9.2 last paragraph)
//! - Music sequencer / pattern chaining (SPEC §9.5)

use std::sync::mpsc;

use anyhow::{Context, Result, anyhow};
use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub const SFX_COUNT: usize = 64;
pub const MUSIC_COUNT: usize = 128;
pub const SFX_STEPS: usize = 32;
pub const VOICES: usize = 4;

/// Engine logic rate. SPEC §9.4 defines step duration in terms of cart hz;
/// the engine loop is hardcoded to 60 Hz in `main.rs`, so the audio side uses
/// 60 too. If the engine ever honors cart-declared hz, this becomes a runtime
/// command.
const ENGINE_HZ: f32 = 60.0;

#[derive(Clone, Copy, Default, Debug)]
pub struct Sfx {
    pub steps: [u16; SFX_STEPS],
    pub speed: u8,
    pub loop_start: u8,
    pub loop_end: u8,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Pattern {
    pub flags: u8,
    pub channels: [u8; 4],
}

#[derive(Debug)]
pub enum AudioCmd {
    LoadSfxBank(Box<[Sfx; SFX_COUNT]>),
    LoadMusicBank(Box<[Pattern; MUSIC_COUNT]>),
    PlaySfx { n: u8, ch: i8, offset: u8 },
    StopChannel { ch: u8 },
    StopAll,
    SetMaster(f32),
}

pub type AudioCmdTx = mpsc::Sender<AudioCmd>;
type AudioCmdRx = mpsc::Receiver<AudioCmd>;

pub struct Synth {
    pub tx: AudioCmdTx,
    _stream: Stream,
}

impl Synth {
    pub fn try_open() -> Result<Self> {
        let host = cpal::default_host();
        let device = pick_output_device(&host)
            .ok_or_else(|| anyhow!("no audio output device"))?;

        let device_name = device.name().unwrap_or_else(|_| "<unnamed>".into());

        let config = device
            .default_output_config()
            .context("querying default output config")?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let sample_format = config.sample_format();
        println!(
            "[audio] device={:?} host={:?} {} Hz, {} ch, {:?}",
            device_name,
            host.id(),
            sample_rate,
            channels,
            sample_format
        );
        let stream_config: cpal::StreamConfig = config.into();

        let (tx, rx) = mpsc::channel();
        let mut state = State::new(sample_rate, channels, rx);

        let err_fn = |err| eprintln!("[audio] stream error: {err}");

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], _| state.render_f32(data),
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                &stream_config,
                move |data: &mut [i16], _| {
                    let mut buf = vec![0.0_f32; data.len()];
                    state.render_f32(&mut buf);
                    for (out, &v) in data.iter_mut().zip(buf.iter()) {
                        *out = (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    }
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                &stream_config,
                move |data: &mut [u16], _| {
                    let mut buf = vec![0.0_f32; data.len()];
                    state.render_f32(&mut buf);
                    for (out, &v) in data.iter_mut().zip(buf.iter()) {
                        let n = (v.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32;
                        *out = n as u16;
                    }
                },
                err_fn,
                None,
            ),
            fmt => return Err(anyhow!("unsupported sample format: {fmt:?}")),
        }
        .context("building output stream")?;

        stream.play().context("starting audio stream")?;

        Ok(Self {
            tx,
            _stream: stream,
        })
    }
}

/// Pick an output device, preferring the higher-level routing layer over raw
/// ALSA "default" on Linux. On many PipeWire setups (no asound.conf), the
/// ALSA "default" PCM points at the first hardware card — which may be
/// suspended while the user's actual sink is a different device. The "pulse"
/// (or "pipewire") ALSA device is provided by pipewire-pulse / pipewire-alsa
/// and routes correctly to the active sink, so we prefer it when present.
fn pick_output_device(host: &cpal::Host) -> Option<cpal::Device> {
    // Lazy iteration with early-out: prefer "pipewire" / "pulse" devices
    // because on PipeWire/Pulse systems the ALSA "default" PCM can route to
    // a suspended hardware sink. We can't enumerate the full list eagerly
    // because cpal's Linux backend probes OSS / JACK during enumeration and
    // those probes are slow.
    if let Ok(devs) = host.output_devices() {
        for d in devs {
            if let Ok(name) = d.name() {
                if name == "pipewire" || name == "pulse" {
                    return Some(d);
                }
            }
        }
    }
    host.default_output_device()
}

// ---------------------------------------------------------------------------
//  Internal mixer state — lives in the audio callback thread.
// ---------------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
struct Voice {
    active: bool,
    sfx_id: usize,
    step_idx: u8,
    samples_in_step: u32,
    samples_per_step: u32,
    phase: f32,
    pitch_hz: f32,
    waveform: u8,
    volume: f32,
    effect: u8,
}

struct State {
    sample_rate: f32,
    output_channels: usize,
    voices: [Voice; VOICES],
    sfx_bank: Box<[Sfx; SFX_COUNT]>,
    music_bank: Box<[Pattern; MUSIC_COUNT]>,
    master: f32,
    rx: AudioCmdRx,
    noise_state: u32,
}

impl State {
    fn new(sample_rate: u32, channels: u16, rx: AudioCmdRx) -> Self {
        Self {
            sample_rate: sample_rate as f32,
            output_channels: channels as usize,
            voices: [Voice::default(); VOICES],
            sfx_bank: Box::new([Sfx::default(); SFX_COUNT]),
            music_bank: Box::new([Pattern::default(); MUSIC_COUNT]),
            master: 1.0,
            rx,
            noise_state: 0xDEAD_BEEF,
        }
    }

    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.rx.try_recv() {
            self.apply(cmd);
        }
    }

    fn apply(&mut self, cmd: AudioCmd) {
        match cmd {
            AudioCmd::LoadSfxBank(bank) => self.sfx_bank = bank,
            AudioCmd::LoadMusicBank(bank) => self.music_bank = bank,
            AudioCmd::SetMaster(v) => self.master = v.clamp(0.0, 1.0),
            AudioCmd::StopAll => {
                for v in self.voices.iter_mut() {
                    v.active = false;
                }
            }
            AudioCmd::StopChannel { ch } => {
                if let Some(v) = self.voices.get_mut(ch as usize) {
                    v.active = false;
                }
            }
            AudioCmd::PlaySfx { n, ch, offset } => self.start_sfx(n, ch, offset),
        }
    }

    fn start_sfx(&mut self, n: u8, ch: i8, offset: u8) {
        let n = n as usize;
        if n >= SFX_COUNT {
            return;
        }
        let sfx = self.sfx_bank[n];
        if sfx.speed == 0 {
            return;
        }
        let voice_idx = if ch < 0 {
            // First free voice; if all busy, drop the request.
            match self.voices.iter().position(|v| !v.active) {
                Some(i) => i,
                None => return,
            }
        } else if (ch as usize) < VOICES {
            ch as usize
        } else {
            return;
        };
        let v = &mut self.voices[voice_idx];
        v.active = true;
        v.sfx_id = n;
        v.step_idx = offset.min((SFX_STEPS - 1) as u8);
        v.samples_in_step = 0;
        v.phase = 0.0;
        v.samples_per_step = step_samples(sfx.speed, self.sample_rate);
        load_step(&sfx, v.step_idx, v);
    }

    fn render_f32(&mut self, output: &mut [f32]) {
        self.drain_commands();
        let n_chans = self.output_channels.max(1);
        let frames = output.len() / n_chans;
        let mut i = 0;
        for _ in 0..frames {
            let s = self.mix_one_sample();
            for _ in 0..n_chans {
                output[i] = s;
                i += 1;
            }
        }
        while i < output.len() {
            output[i] = 0.0;
            i += 1;
        }
    }

    fn mix_one_sample(&mut self) -> f32 {
        // Sum each active voice at its own volume, then soft-clip. Vintage
        // chiptune behavior: a single voice at vol=7 plays at full amplitude;
        // multiple voices stack but never blow out the DAC.
        let mut sum = 0.0_f32;
        for vi in 0..VOICES {
            if !self.voices[vi].active {
                continue;
            }
            sum += self.synth_voice(vi);
            self.advance_voice(vi);
        }
        (sum * self.master).tanh()
    }

    fn synth_voice(&mut self, vi: usize) -> f32 {
        let (phase, waveform, volume) = {
            let v = &self.voices[vi];
            (v.phase, v.waveform, v.volume)
        };
        let wave = match waveform.min(7) {
            0 => triangle(phase),
            1 => tilted_saw(phase),
            2 => saw(phase),
            3 => square(phase),
            4 => pulse(phase, 0.25),
            5 => organ(phase),
            6 => self.noise(),
            7 => phaser(phase),
            _ => 0.0,
        };
        wave * volume
    }

    fn noise(&mut self) -> f32 {
        let mut x = self.noise_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.noise_state = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    fn advance_voice(&mut self, vi: usize) {
        let (sfx_id, sample_rate) = {
            let v = &mut self.voices[vi];
            let phase_inc = v.pitch_hz / self.sample_rate;
            v.phase = (v.phase + phase_inc).fract();
            v.samples_in_step += 1;
            (v.sfx_id, self.sample_rate)
        };
        let need_step_boundary = {
            let v = &self.voices[vi];
            v.samples_in_step >= v.samples_per_step
        };
        if !need_step_boundary {
            return;
        }
        let sfx = self.sfx_bank[sfx_id];
        let v = &mut self.voices[vi];
        v.samples_in_step = 0;
        let next = v.step_idx as usize + 1;
        let has_loop = sfx.loop_end > sfx.loop_start;
        if has_loop && next > sfx.loop_end as usize {
            v.step_idx = sfx.loop_start;
        } else if next >= SFX_STEPS {
            v.active = false;
            return;
        } else {
            v.step_idx = next as u8;
        }
        load_step(&sfx, v.step_idx, v);
        v.samples_per_step = step_samples(sfx.speed, sample_rate);
    }
}

// ---------------------------------------------------------------------------
//  Step / pitch decoding.
// ---------------------------------------------------------------------------

fn step_samples(speed: u8, sample_rate: f32) -> u32 {
    let secs = speed.max(1) as f32 / ENGINE_HZ;
    (secs * sample_rate).max(1.0) as u32
}

/// SPEC §9.4: pitch n → MIDI note (n + 36). A4 = MIDI 69 = 440 Hz.
fn pitch_to_hz(pitch: u8) -> f32 {
    let midi = (pitch as i32) + 36;
    440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0)
}

fn load_step(sfx: &Sfx, step_idx: u8, v: &mut Voice) {
    let raw = sfx.steps[step_idx as usize];
    let pitch = ((raw >> 10) & 0x3F) as u8;
    let waveform = ((raw >> 6) & 0x0F) as u8;
    let volume = ((raw >> 3) & 0x07) as u8;
    let effect = (raw & 0x07) as u8;
    v.pitch_hz = pitch_to_hz(pitch);
    v.waveform = waveform;
    v.volume = volume as f32 / 7.0;
    v.effect = effect;
}

// ---------------------------------------------------------------------------
//  Waveform generators. Phase ∈ [0, 1). All outputs in [-1, 1].
// ---------------------------------------------------------------------------

fn triangle(p: f32) -> f32 {
    if p < 0.5 {
        4.0 * p - 1.0
    } else {
        3.0 - 4.0 * p
    }
}

fn tilted_saw(p: f32) -> f32 {
    // Asymmetric rise/fall — rises slowly, snaps back. PICO-8-ish.
    const KNEE: f32 = 0.875;
    if p < KNEE {
        (p / KNEE) * 2.0 - 1.0
    } else {
        1.0 - ((p - KNEE) / (1.0 - KNEE)) * 2.0
    }
}

fn saw(p: f32) -> f32 {
    2.0 * p - 1.0
}

fn square(p: f32) -> f32 {
    if p < 0.5 { 1.0 } else { -1.0 }
}

fn pulse(p: f32, duty: f32) -> f32 {
    if p < duty { 1.0 } else { -1.0 }
}

fn organ(p: f32) -> f32 {
    // Stacked triangles at 1× and 2× — additive flavor.
    0.5 * (triangle(p) + triangle((p * 2.0).fract()))
}

fn phaser(p: f32) -> f32 {
    // Two saws beating at slightly detuned rates.
    (saw(p) + saw((p * 1.07).fract())) * 0.5
}
