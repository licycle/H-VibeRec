// Audio processing utilities for local recording
// Enhanced with dual-stream processing and optimized downsampling

use anyhow::Result;
use log::debug;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::collections::VecDeque;

/// Target sample rate for final mixed audio (optimized for file size and quality)
pub const TARGET_SAMPLE_RATE: u32 = 16000;

/// Streaming processing configuration constants
pub const RESAMPLER_CHUNK_SIZE: usize = 4096; // Chunk size for StreamingResampler
pub const RESAMPLER_SINC_LEN: usize = 512; // Sinc filter length for high quality
pub const RESAMPLER_F_CUTOFF: f32 = 0.95; // Cutoff frequency for anti-aliasing
pub const RESAMPLER_OVERSAMPLING_FACTOR: usize = 256; // Oversampling factor for precision

/// Buffer management constants  
pub const BUFFER_FLUSH_THRESHOLD: usize = 320000; // ~20 seconds at 16kHz (20 * 16000)
pub const BUFFER_FLUSH_INTERVAL_MS: u64 = 10000; // Flush every 10 seconds

/// Streaming resampler for real-time processing
/// Maintains state between chunks to ensure smooth resampling
pub struct StreamingResampler {
    resampler: SincFixedIn<f32>,
    buffer: VecDeque<f32>,
    chunk_size: usize,
    from_rate: u32,
    to_rate: u32,
}

impl StreamingResampler {
    pub fn new(from_sample_rate: u32, to_sample_rate: u32, chunk_size: usize) -> Result<Self> {
        if from_sample_rate == to_sample_rate {
            // Pass-through case - create a minimal resampler (1:1 ratio)
            let params = SincInterpolationParameters {
                sinc_len: 64,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 64,
                window: WindowFunction::BlackmanHarris2,
            };
            return Ok(Self {
                resampler: SincFixedIn::<f32>::new(1.0, 2.0, params, chunk_size, 1)?,
                buffer: VecDeque::new(),
                chunk_size,
                from_rate: from_sample_rate,
                to_rate: to_sample_rate,
            });
        }

        let params = SincInterpolationParameters {
            sinc_len: RESAMPLER_SINC_LEN,
            f_cutoff: RESAMPLER_F_CUTOFF,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: RESAMPLER_OVERSAMPLING_FACTOR,
            window: WindowFunction::BlackmanHarris2,
        };

        let resampler = SincFixedIn::<f32>::new(
            to_sample_rate as f64 / from_sample_rate as f64,
            2.0,
            params,
            chunk_size,
            1,
        )?;

        Ok(Self {
            resampler,
            buffer: VecDeque::new(),
            chunk_size,
            from_rate: from_sample_rate,
            to_rate: to_sample_rate,
        })
    }

    pub fn process_chunk(&mut self, input: &[f32]) -> Result<Vec<f32>> {
        if self.from_rate == self.to_rate {
            // Pass-through
            return Ok(input.to_vec());
        }

        // Add new samples to buffer
        self.buffer.extend(input.iter());

        let mut output = Vec::new();

        // Process complete chunks
        while self.buffer.len() >= self.chunk_size {
            let chunk: Vec<f32> = self.buffer.drain(0..self.chunk_size).collect();
            let waves_in = vec![chunk];
            let waves_out = self.resampler.process(&waves_in, None)?;
            output.extend(waves_out.into_iter().next().unwrap());
        }

        Ok(output)
    }

    pub fn flush(&mut self) -> Result<Vec<f32>> {
        if self.buffer.is_empty() || self.from_rate == self.to_rate {
            let remaining = self.buffer.drain(..).collect();
            return Ok(remaining);
        }

        // Process remaining samples with padding to ensure complete processing
        let mut remaining: Vec<f32> = self.buffer.drain(..).collect();
        if !remaining.is_empty() {
            // Store original length for proper output calculation
            let original_len = remaining.len();

            // SincFixedIn requires exactly chunk_size input frames. Pad the final partial chunk
            // with silence, then trim the resampled output back to the unpadded duration.
            remaining.resize(self.chunk_size, 0.0);

            let waves_in = vec![remaining];
            let waves_out = self.resampler.process(&waves_in, None)?;
            let mut result = waves_out.into_iter().next().unwrap();

            // Remove the padding-induced samples from the end
            let expected_output_len =
                (original_len as f64 * self.to_rate as f64 / self.from_rate as f64) as usize;
            if result.len() > expected_output_len {
                result.truncate(expected_output_len);
            }

            Ok(result)
        } else {
            Ok(Vec::new())
        }
    }
}

/// Mix two audio streams with proper gain control and timing alignment
pub fn mix_streams_aligned(
    stream1: &[f32],
    stream2: &[f32],
    stream1_gain: f32,
    stream2_gain: f32,
) -> Vec<f32> {
    let max_len = stream1.len().max(stream2.len());
    let mut mixed = Vec::with_capacity(max_len);

    for i in 0..max_len {
        let sample1 = if i < stream1.len() {
            stream1[i] * stream1_gain
        } else {
            0.0
        };
        let sample2 = if i < stream2.len() {
            stream2[i] * stream2_gain
        } else {
            0.0
        };

        // Mix with soft limiting to prevent clipping
        let combined = sample1 + sample2;
        let limited = combined.clamp(-0.95, 0.95);
        mixed.push(limited);
    }

    debug!(
        "Mixed {} samples with gains ({:.2}, {:.2})",
        max_len, stream1_gain, stream2_gain
    );
    mixed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_resampler_flush_handles_partial_chunk() {
        let mut resampler =
            StreamingResampler::new(44_100, TARGET_SAMPLE_RATE, RESAMPLER_CHUNK_SIZE)
                .expect("create streaming resampler");

        let output = resampler
            .process_chunk(&vec![0.1; RESAMPLER_CHUNK_SIZE - 1280])
            .expect("buffer partial input");
        assert!(output.is_empty());

        let remaining = resampler
            .flush()
            .expect("partial chunk flush should succeed");

        assert!(!remaining.is_empty());
    }
}
