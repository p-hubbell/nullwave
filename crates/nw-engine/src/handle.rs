//! Owns the cpal output stream, the lock-free queues and the basedrop collector.

use std::time::{Duration, Instant};

use basedrop::{Collector, Handle, Owned};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Stream, StreamConfig};

use crate::core::{Command, EngineCore, Feedback, MAX_BLOCK_FRAMES, run_block};
use crate::snapshot::Snapshot;

const COMMAND_CAPACITY: usize = 256;
const FEEDBACK_CAPACITY: usize = 1024;
const REQUESTED_BUFFER_FRAMES: u32 = 256;
const LOAD_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("no default audio output device")]
    NoDevice,
    #[error("audio configuration error: {0}")]
    Config(String),
    #[error("audio stream error: {0}")]
    Stream(String),
}

pub struct EngineHandle {
    // Field order matters: fields drop in declaration order, so `stream` (whose
    // callback owns the `EngineCore` and its `Owned` snapshot) must come before
    // `collector`. Dropping an `Owned` after its collector is gone would leak or
    // misbehave, and the callback must be stopped before the collector is touched.
    _stream: Stream,
    commands: rtrb::Producer<Command>,
    feedback: rtrb::Consumer<Feedback>,
    collector: Collector,
    pub sample_rate: u32,
    pub buffer_size: Option<u32>,
}

impl EngineHandle {
    pub fn start() -> Result<EngineHandle, EngineError> {
        let device = cpal::default_host().default_output_device().ok_or(EngineError::NoDevice)?;
        let supported =
            device.default_output_config().map_err(|e| EngineError::Config(e.to_string()))?;
        let sample_rate = supported.sample_rate();
        let channels = usize::from(supported.channels());
        if channels == 0 {
            return Err(EngineError::Config("device reports zero channels".into()));
        }

        let collector = Collector::new();
        let attempt = |buffer_size: BufferSize| {
            let config =
                StreamConfig { channels: supported.channels(), sample_rate, buffer_size };
            build_stream(&device, config, channels, &collector.handle())
        };
        let (stream, commands, feedback, buffer_size) =
            match attempt(BufferSize::Fixed(REQUESTED_BUFFER_FRAMES)) {
                Ok((s, c, f)) => (s, c, f, Some(REQUESTED_BUFFER_FRAMES)),
                // Device rejected the fixed size: fall back to its default.
                Err(_) => {
                    let (s, c, f) = attempt(BufferSize::Default)
                        .map_err(|e| EngineError::Stream(e.to_string()))?;
                    (s, c, f, None)
                }
            };
        stream.play().map_err(|e| EngineError::Stream(e.to_string()))?;

        Ok(EngineHandle { _stream: stream, commands, feedback, collector, sample_rate, buffer_size })
    }

    /// Queue a command for the audio thread; hands it back if the queue is full.
    pub fn send(&mut self, command: Command) -> Result<(), Command> {
        self.commands.push(command).map_err(|rtrb::PushError::Full(c)| c)
    }

    /// Drain all feedback produced since the last call.
    pub fn poll(&mut self) -> Vec<Feedback> {
        let mut out = Vec::with_capacity(self.feedback.slots());
        while let Ok(f) = self.feedback.pop() {
            out.push(f);
        }
        out
    }

    /// Free everything the audio thread has released. Call every UI frame.
    pub fn collect_garbage(&mut self) {
        self.collector.collect();
    }

    pub fn collector_handle(&self) -> Handle {
        self.collector.handle()
    }
}

type Built = (Stream, rtrb::Producer<Command>, rtrb::Consumer<Feedback>);

/// Build one output stream with fresh queues and engine core. All state the
/// callback needs is allocated here, on the calling thread.
fn build_stream(
    device: &cpal::Device,
    config: StreamConfig,
    channels: usize,
    collector: &Handle,
) -> Result<Built, cpal::Error> {
    let sample_rate = config.sample_rate;
    let initial = Owned::new(
        collector,
        Snapshot { tempo: 140.0, loop_length: 3840, instruments: Vec::new(), events: Vec::new() },
    );
    let mut core = EngineCore::new(sample_rate, initial);
    let (command_tx, mut command_rx) = rtrb::RingBuffer::new(COMMAND_CAPACITY);
    let (mut feedback_tx, feedback_rx) = rtrb::RingBuffer::new(FEEDBACK_CAPACITY);
    let mut scratch = vec![0.0f32; 2 * MAX_BLOCK_FRAMES];
    let mut load = 0.0f32;
    let mut last_load_sent = Instant::now();

    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _| {
            let mut body = || {
                let started = Instant::now();
                let frames = data.len() / channels;
                if channels == 2 {
                    run_block(&mut core, &mut command_rx, &mut feedback_tx, data);
                } else {
                    // Render stereo into scratch (chunked), then spread to the device layout.
                    let mut done = 0;
                    while done < frames {
                        let n = (frames - done).min(MAX_BLOCK_FRAMES);
                        run_block(&mut core, &mut command_rx, &mut feedback_tx, &mut scratch[..n * 2]);
                        for i in 0..n {
                            let frame = &mut data[(done + i) * channels..(done + i + 1) * channels];
                            frame.fill(0.0);
                            frame[0] = scratch[i * 2];
                            if channels > 1 {
                                frame[1] = scratch[i * 2 + 1];
                            }
                        }
                        done += n;
                    }
                    data[frames * channels..].fill(0.0);
                }
                let elapsed = started.elapsed().as_secs_f32();
                let budget = frames as f32 / sample_rate as f32;
                if budget > 0.0 {
                    load = 0.9 * load + 0.1 * (elapsed / budget);
                }
                if last_load_sent.elapsed() >= LOAD_INTERVAL {
                    last_load_sent = Instant::now();
                    // A full queue just drops this report.
                    let _ = feedback_tx.push(Feedback::Load(load));
                }
            };
            #[cfg(debug_assertions)]
            assert_no_alloc::assert_no_alloc(&mut body);
            #[cfg(not(debug_assertions))]
            body();
        },
        // No logging or I/O on the audio thread; stream errors are ignored for now.
        |_err| {},
        None,
    )?;
    Ok((stream, command_tx, feedback_rx))
}
