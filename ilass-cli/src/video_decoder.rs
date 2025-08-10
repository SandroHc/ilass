#![allow(non_local_definitions)]

use failure::{Backtrace, Context, Fail, ResultExt};
use std::fmt;
use std::path::{Path, PathBuf};
use ffmpeg_next::{codec, media, format, channel_layout, frame, decoder};
use ffmpeg_next::software::resampling;
use crate::define_error;

pub trait AudioReceiver {
    type Output;
    type Error: failure::Fail;

    /// Samples are in 8000kHz mono/single-channel format.
    fn push_samples(&mut self, samples: &[i16]) -> Result<(), Self::Error>;

    fn finish(self) -> Result<Self::Output, Self::Error>;
}

pub struct ChunkedAudioReceiver<R: AudioReceiver> {
    buffer: Vec<i16>,
    filled: usize,
    next: R,
}

impl<R: AudioReceiver> ChunkedAudioReceiver<R> {
    pub fn new(size: usize, next: R) -> ChunkedAudioReceiver<R> {
        ChunkedAudioReceiver {
            buffer: std::vec::from_elem(0, size),
            filled: 0,
            next,
        }
    }
}

impl<R: AudioReceiver> AudioReceiver for ChunkedAudioReceiver<R> {
    type Output = R::Output;
    type Error = R::Error;

    fn push_samples(&mut self, mut samples: &[i16]) -> Result<(), R::Error> {
        assert!(self.buffer.len() > self.filled);

        loop {
            if samples.is_empty() {
                break;
            }

            let sample_count = std::cmp::min(self.buffer.len() - self.filled, samples.len());
            self.buffer[self.filled..self.filled + sample_count].clone_from_slice(&samples[..sample_count]);

            samples = &samples[sample_count..];

            self.filled += sample_count;

            if self.filled == self.buffer.len() {
                self.next.push_samples(self.buffer.as_slice())?;
                self.filled = 0;
            }
        }

        Ok(())
    }

    fn finish(self) -> Result<R::Output, R::Error> {
        self.next.finish()
    }
}

/// Use this trait if you want more detailed information about the progress of operations.
pub trait ProgressHandler {
    /// Will be called one time before `inc()` is called. `steps` is the
    /// number of times `inc()` will be called.
    ///
    /// The number of steps is around the number of lines in the "incorrect" subtitle.
    /// Be aware that this number can be zero!
    #[allow(unused_variables)]
    fn init(&mut self, steps: i64) {}

    /// We made (small) progress!
    fn inc(&mut self) {}

    /// Will be called after the last `inc()`, when `inc()` was called `steps` times.
    fn finish(&mut self) {}
}

define_error!(DecoderError, DecoderErrorKind);

#[derive(Debug, Fail)]
pub enum DecoderErrorKind {
    Decode,
    Init,
    NoAudioStream,
    OpenFile(PathBuf),
    Receiver,
    ResamplerInit,
}

impl fmt::Display for DecoderErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecoderErrorKind::Decode => write!(f, "Decoding audio stream failed"),
            DecoderErrorKind::Init => write!(f, "Initialization of FFmpeg failed"),
            DecoderErrorKind::NoAudioStream => write!(f, "No audio stream found"),
            DecoderErrorKind::OpenFile(path) => write!(f, "Failed to open media file: {}", path.display()),
            DecoderErrorKind::Receiver => write!(f, "Processing of audio samples failed"),
            DecoderErrorKind::ResamplerInit => write!(f, "Resampler initialization failed"),
        }
    }
}

pub(crate) struct VideoDecoder {}

impl VideoDecoder {
    /// Samples are pushed in 8 kHz mono/single-channel format.
    pub(crate) fn decode<T>(
        file_path: impl AsRef<Path>,
        audio_index: Option<usize>,
        mut receiver: impl AudioReceiver<Output = T>,
        mut progress_handler: impl ProgressHandler,
    ) -> Result<T, DecoderError> {
        ffmpeg_next::init().with_context(|_| DecoderErrorKind::Init)?;

        let mut format_context = format::input(&file_path)
            .with_context(|_| DecoderErrorKind::OpenFile(file_path.as_ref().to_path_buf()))?;

        // Find the best audio stream or use the specified index
        let audio_stream = if let Some(index) = audio_index {
            format_context.streams()
                .nth(index)
                .filter(|stream| stream.parameters().medium() == media::Type::Audio)
                .ok_or(DecoderErrorKind::NoAudioStream)?
        } else {
            // Find audio stream with the least channels (can be resampled faster)
            format_context.streams()
                .filter(|stream| stream.parameters().medium() == media::Type::Audio)
                .min_by_key(|stream| {
                    let Ok(codec_context) = codec::context::Context::from_parameters(stream.parameters()) else {
                        return u16::MAX
                    };
                    let Ok(decoder) = codec_context.decoder().audio() else {
                        return u16::MAX
                    };
                    decoder.channels()
                })
                .ok_or(DecoderErrorKind::NoAudioStream)?
        };
        let audio_stream_idx = audio_stream.index();

        let codec_context = codec::context::Context::from_parameters(audio_stream.parameters())
            .with_context(|_| DecoderErrorKind::Decode)?;
        let mut decoder = codec_context.decoder().audio()
            .with_context(|_| DecoderErrorKind::Decode)?;

        let mut resampler = {
            let out_format = format::Sample::I16(format::sample::Type::Planar);
            let out_channel_layout = channel_layout::ChannelLayout::MONO;
            let out_rate = 8000;

            resampling::context::Context::get(
                decoder.format(),
                decoder.channel_layout(),
                decoder.rate(),
                out_format,
                out_channel_layout,
                out_rate,
            )
        }.with_context(|_| DecoderErrorKind::ResamplerInit)?;

        progress_handler.init(audio_stream.frames());

        let mut process_frames = |decoder: &mut decoder::Audio| -> Result<(), DecoderError> {
            let mut decoded = frame::audio::Audio::empty();
            while decoder.receive_frame(&mut decoded).is_ok() {
                let mut resampled_frame = frame::audio::Audio::empty();
                resampler.run(&decoded, &mut resampled_frame).with_context(|_| DecoderErrorKind::Decode)?;

                // Extract all samples from channel 0 (mono)
                let sample_count = resampled_frame.samples();
                let samples = unsafe {
                    std::slice::from_raw_parts(
                        resampled_frame.data(0).as_ptr() as *const i16,
                        sample_count,
                    )
                };
                receiver.push_samples(samples)
                    .with_context(|_| DecoderErrorKind::Receiver)?;
            }
            Ok(())
        };

        for (stream, packet) in format_context.packets() {
            if stream.index() == audio_stream_idx {
                decoder.send_packet(&packet).with_context(|_| DecoderErrorKind::Decode)?;
                process_frames(&mut decoder)?;
                progress_handler.inc();
            }
        }

        decoder.send_eof().with_context(|_| DecoderErrorKind::Decode)?;
        process_frames(&mut decoder)?;
        progress_handler.finish();

        let result = receiver.finish().with_context(|_| DecoderErrorKind::Receiver)?;
        Ok(result)
    }
}
