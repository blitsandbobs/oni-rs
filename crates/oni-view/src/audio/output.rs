//! The output device: its callback mixes the voices (play mode only).

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::mixer::Mixer;

/// A playing output stream (sound stops when dropped).
pub struct Output {
    _stream: cpal::Stream,
}

impl Output {
    /// Opens the default output device (32-bit float samples), or `None`.
    pub fn open(mixer: Arc<Mutex<Mixer>>) -> Option<Output> {
        let device = cpal::default_host().default_output_device()?;
        let config = device.default_output_config().ok()?;
        if config.sample_format() != cpal::SampleFormat::F32 {
            eprintln!("audio: the output device doesn't take float samples; no sound");
            return None;
        }
        let channels = config.channels() as usize;
        mixer.lock().ok()?.rate = config.sample_rate() as f32;
        let mut stereo = Vec::new();
        let stream = device
            .build_output_stream(
                config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / channels.max(1);
                    stereo.clear();
                    stereo.resize(frames * 2, 0.0);
                    if let Ok(mut m) = mixer.lock() {
                        m.mix(Some(&mut stereo), frames);
                    }
                    for (f, out) in data.chunks_mut(channels).enumerate() {
                        let (l, r) = (
                            stereo[f * 2].clamp(-1.0, 1.0),
                            stereo[f * 2 + 1].clamp(-1.0, 1.0),
                        );
                        for (c, s) in out.iter_mut().enumerate() {
                            *s = match (channels, c) {
                                (1, _) => (l + r) * 0.5,
                                (_, 0) => l,
                                (_, 1) => r,
                                _ => 0.0,
                            };
                        }
                    }
                },
                |e| eprintln!("audio: {e}"),
                None,
            )
            .ok()?;
        stream.play().ok()?;
        Some(Output { _stream: stream })
    }
}
