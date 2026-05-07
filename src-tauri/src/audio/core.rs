use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::StreamError;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::{fmt, thread};
use tokio::sync::{broadcast, oneshot};

#[derive(Clone, Eq, PartialEq, Hash, Serialize, Debug, Deserialize)]
pub enum DeviceType {
    Input,
    Output,
}

#[derive(Clone, Eq, PartialEq, Hash, Serialize, Debug)]
pub struct AudioDevice {
    pub name: String,
    pub device_type: DeviceType,
}

impl AudioDevice {
    pub fn new(name: String, device_type: DeviceType) -> Self {
        AudioDevice { name, device_type }
    }
}

impl fmt::Display for AudioDevice {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} ({})",
            self.name,
            match self.device_type {
                DeviceType::Input => "input",
                DeviceType::Output => "output",
            }
        )
    }
}

// Platform-specific audio device configurations
#[cfg(target_os = "windows")]
fn configure_windows_audio(host: &cpal::Host) -> Result<Vec<AudioDevice>> {
    let mut devices = Vec::new();

    // Get WASAPI devices
    if let Ok(wasapi_host) = cpal::host_from_id(cpal::HostId::Wasapi) {
        info!("Using WASAPI host for Windows audio device enumeration");

        // Add output devices (including loopback)
        if let Ok(output_devices) = wasapi_host.output_devices() {
            for device in output_devices {
                if let Ok(name) = device.name() {
                    // For Windows, we need to mark output devices specifically for loopback
                    info!("Found Windows output device: {}", name);
                    devices.push(AudioDevice::new(name.clone(), DeviceType::Output));
                }
            }
        } else {
            warn!("Failed to enumerate WASAPI output devices");
        }

        // Add input devices from WASAPI
        if let Ok(input_devices) = wasapi_host.input_devices() {
            for device in input_devices {
                if let Ok(name) = device.name() {
                    info!("Found Windows input device: {}", name);
                    devices.push(AudioDevice::new(name.clone(), DeviceType::Input));
                }
            }
        } else {
            warn!("Failed to enumerate WASAPI input devices");
        }
    } else {
        warn!("Failed to create WASAPI host, falling back to default host");
    }

    // If WASAPI failed or returned no devices, try default host as fallback
    if devices.is_empty() {
        debug!(
            "WASAPI device enumeration failed or returned no devices, falling back to default host"
        );
        // Add regular input devices
        if let Ok(input_devices) = host.input_devices() {
            for device in input_devices {
                if let Ok(name) = device.name() {
                    info!("Found fallback input device: {}", name);
                    devices.push(AudioDevice::new(name.clone(), DeviceType::Input));
                }
            }
        } else {
            warn!("Failed to enumerate input devices from default host");
        }

        // Add output devices
        if let Ok(output_devices) = host.output_devices() {
            for device in output_devices {
                if let Ok(name) = device.name() {
                    info!("Found fallback output device: {}", name);
                    devices.push(AudioDevice::new(name.clone(), DeviceType::Output));
                }
            }
        } else {
            warn!("Failed to enumerate output devices from default host");
        }
    }

    // If we still have no devices, add default devices
    if devices.is_empty() {
        warn!("No audio devices found, adding default devices only");

        // Try to add default input device
        if let Some(device) = host.default_input_device() {
            if let Ok(name) = device.name() {
                info!("Adding default input device: {}", name);
                devices.push(AudioDevice::new(name, DeviceType::Input));
            }
        }

        // Try to add default output device
        if let Some(device) = host.default_output_device() {
            if let Ok(name) = device.name() {
                info!("Adding default output device: {}", name);
                devices.push(AudioDevice::new(name, DeviceType::Output));
            }
        }
    }

    info!("Found {} Windows audio devices", devices.len());
    Ok(devices)
}

pub async fn list_audio_devices() -> Result<Vec<AudioDevice>> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    // Platform-specific device enumeration
    #[cfg(target_os = "windows")]
    {
        devices = configure_windows_audio(&host)?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Add input devices
        if let Ok(input_devices) = host.input_devices() {
            for device in input_devices {
                if let Ok(name) = device.name() {
                    devices.push(AudioDevice::new(name, DeviceType::Input));
                }
            }
        }

        // Add output devices for system audio capture
        if let Ok(output_devices) = host.output_devices() {
            for device in output_devices {
                if let Ok(name) = device.name() {
                    devices.push(AudioDevice::new(name, DeviceType::Output));
                }
            }
        }
    }

    Ok(devices)
}

// Helper function to check if device supports system audio capture
pub fn supports_system_audio(device: &AudioDevice) -> bool {
    match device.device_type {
        DeviceType::Output => {
            #[cfg(target_os = "macos")]
            {
                // On macOS, ScreenCaptureKit devices can capture system audio
                true
            }
            #[cfg(target_os = "windows")]
            {
                // On Windows, WASAPI loopback can capture system audio
                true
            }
            #[cfg(target_os = "linux")]
            {
                // On Linux, PulseAudio monitor sources can capture system audio
                device.name.contains("monitor")
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
            {
                false
            }
        }
        DeviceType::Input => false,
    }
}

pub fn default_input_device() -> Result<AudioDevice> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("No default input device found"))?;
    Ok(AudioDevice::new(device.name()?, DeviceType::Input))
}

pub fn default_output_device() -> Result<AudioDevice> {
    #[cfg(target_os = "macos")]
    {
        // On macOS, try ScreenCaptureKit for system audio capture
        if let Ok(host) = cpal::host_from_id(cpal::HostId::ScreenCaptureKit) {
            if let Some(device) = host.default_input_device() {
                if let Ok(name) = device.name() {
                    return Ok(AudioDevice::new(name, DeviceType::Output));
                }
            }
        }
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("No default output device found"))?;
        return Ok(AudioDevice::new(device.name()?, DeviceType::Output));
    }

    #[cfg(target_os = "windows")]
    {
        // Try WASAPI host first for Windows
        if let Ok(wasapi_host) = cpal::host_from_id(cpal::HostId::Wasapi) {
            if let Some(device) = wasapi_host.default_output_device() {
                if let Ok(name) = device.name() {
                    return Ok(AudioDevice::new(name, DeviceType::Output));
                }
            }
        }
        // Fallback to default host if WASAPI fails
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("No default output device found"))?;
        return Ok(AudioDevice::new(device.name()?, DeviceType::Output));
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("No default output device found"))?;
        return Ok(AudioDevice::new(device.name()?, DeviceType::Output));
    }
}

pub fn trigger_audio_permission() -> Result<()> {
    info!("Triggering audio permission request...");

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("No default input device found"))?;

    let config = match device.default_input_config() {
        Ok(config) => config,
        Err(e) => {
            warn!(
                "Failed to get default input config: {}, trying supported configs",
                e
            );
            let mut supported_configs = device.supported_input_configs()?;
            supported_configs
                .next()
                .ok_or_else(|| anyhow!("No supported input configurations found"))?
                .with_max_sample_rate()
        }
    };

    info!("Using audio config for permission: {:?}", config);

    // Build and start an input stream to trigger the permission request
    let stream = device.build_input_stream(
        &config.into(),
        |data: &[f32], _: &cpal::InputCallbackInfo| {
            if !data.is_empty() {
                let _sample = data[0]; // Read one sample to ensure mic access
            }
        },
        |err| {
            error!("Error in audio permission stream: {}", err);
            if err.to_string().to_lowercase().contains("permission") {
                warn!("Audio permission denied or required");
            }
        },
        None,
    )?;

    stream.play()?;
    info!("Audio permission stream started");

    std::thread::sleep(std::time::Duration::from_millis(1000));

    drop(stream);
    info!("Audio permission request completed");

    Ok(())
}

pub async fn trigger_system_audio_permission() -> Result<()> {
    info!("Triggering system audio permission check...");

    let device = default_output_device()?;
    if !supports_system_audio(&device) {
        return Err(anyhow!(
            "System audio capture is not supported by device: {}",
            device.name
        ));
    }

    let (_device, config) = get_device_and_config(&device).await?;
    info!("System audio capture config available: {:?}", config);

    Ok(())
}

#[derive(Clone)]
pub struct AudioStream {
    pub device_config: cpal::SupportedStreamConfig,
    transmitter: Arc<tokio::sync::broadcast::Sender<Vec<f32>>>,
    stream_control: mpsc::Sender<StreamControl>,
    stream_thread: Option<Arc<tokio::sync::Mutex<Option<thread::JoinHandle<()>>>>>,
}

enum StreamControl {
    Stop(oneshot::Sender<()>),
}

fn report_audio_stream_ready(
    ready_tx: &mut Option<oneshot::Sender<std::result::Result<(), String>>>,
    result: std::result::Result<(), String>,
) {
    if let Some(sender) = ready_tx.take() {
        let _ = sender.send(result);
    }
}

fn publish_audio_chunk(sender: &broadcast::Sender<Vec<f32>>, samples: Vec<f32>) -> bool {
    if sender.receiver_count() == 0 {
        debug!("Dropping audio chunk because no receiver is subscribed yet");
        return true;
    }

    if let Err(error) = sender.send(samples) {
        debug!("Dropping audio chunk after receivers closed: {}", error);
    }
    true
}

impl AudioStream {
    pub async fn from_device(
        device: Arc<AudioDevice>,
        is_running: Arc<AtomicBool>,
    ) -> Result<Self> {
        info!(
            "Initializing audio stream for device: {}",
            device.to_string()
        );
        let (tx, _) = broadcast::channel::<Vec<f32>>(1000);
        let tx_clone = tx.clone();

        // Get device and config
        let (cpal_audio_device, config) = get_device_and_config(&device).await?;

        let channels = config.channels();
        info!(
            "Audio config - Sample rate: {}, Channels: {}, Format: {:?}",
            config.sample_rate().0,
            channels,
            config.sample_format()
        );

        let is_running_weak = Arc::downgrade(&is_running);
        let device_clone = device.clone();
        let config_clone = config.clone();
        let (stream_control_tx, stream_control_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = oneshot::channel::<std::result::Result<(), String>>();

        let stream_thread = Arc::new(tokio::sync::Mutex::new(Some(thread::spawn(move || {
            let mut ready_tx = Some(ready_tx);
            let device = device_clone;
            let device_name = device.to_string();
            let config = config_clone;
            info!("Starting audio stream thread for device: {}", device_name);

            let error_callback = move |err: StreamError| {
                error!("Audio stream error: {}", err);
                if let Some(arc) = is_running_weak.upgrade() {
                    arc.store(false, Ordering::Relaxed);
                }
            };

            let stream_result = match config.sample_format() {
                cpal::SampleFormat::F32 => cpal_audio_device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &_| {
                        let mono = audio_to_mono(data, channels);
                        debug!("Received audio chunk: {} samples", mono.len());
                        publish_audio_chunk(&tx, mono);
                    },
                    error_callback.clone(),
                    None,
                ),
                cpal::SampleFormat::I16 => cpal_audio_device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &_| {
                        let f32_data: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                        let mono = audio_to_mono(&f32_data, channels);
                        debug!("Received audio chunk: {} samples", mono.len());
                        publish_audio_chunk(&tx, mono);
                    },
                    error_callback.clone(),
                    None,
                ),
                _ => {
                    let message = format!("Unsupported sample format: {}", config.sample_format());
                    error!("{message}");
                    report_audio_stream_ready(&mut ready_tx, Err(message));
                    return;
                }
            };
            let stream = match stream_result {
                Ok(stream) => stream,
                Err(error) => {
                    let message = format!("Failed to build input stream: {error}");
                    error!("{message}");
                    report_audio_stream_ready(&mut ready_tx, Err(message));
                    return;
                }
            };

            if let Err(error) = stream.play() {
                let message = format!(
                    "Failed to play stream for {}: {}",
                    device.to_string(),
                    error
                );
                error!("{message}");
                report_audio_stream_ready(&mut ready_tx, Err(message));
                return;
            }
            info!(
                "Audio stream started successfully for device: {}",
                device_name
            );
            report_audio_stream_ready(&mut ready_tx, Ok(()));

            if let Ok(StreamControl::Stop(response)) = stream_control_rx.recv() {
                info!("Stopping audio stream...");
                if let Err(e) = stream.pause() {
                    error!("Failed to pause stream: {}", e);
                }
                drop(stream);
                response.send(()).ok();
                info!("Audio stream stopped and cleaned up");
            }
        }))));

        match ready_rx.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(anyhow!(error)),
            Err(_) => {
                return Err(anyhow!(
                    "Audio stream worker exited before startup completed"
                ))
            }
        }

        Ok(AudioStream {
            device_config: config,
            transmitter: Arc::new(tx_clone),
            stream_control: stream_control_tx,
            stream_thread: Some(stream_thread),
        })
    }

    pub async fn subscribe(&self) -> broadcast::Receiver<Vec<f32>> {
        self.transmitter.subscribe()
    }

    pub async fn stop(&self) -> Result<()> {
        let (tx, _rx) = oneshot::channel();
        self.stream_control.send(StreamControl::Stop(tx))?;

        if let Some(thread_mutex) = &self.stream_thread {
            let thread_mutex = thread_mutex.clone();
            let thread_handle = tokio::task::spawn_blocking(move || {
                let mut thread_guard = thread_mutex.blocking_lock();
                if let Some(join_handle) = thread_guard.take() {
                    join_handle
                        .join()
                        .map_err(|_| anyhow!("Failed to join stream thread"))
                } else {
                    Ok(())
                }
            });

            thread_handle.await??;
        }

        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn get_windows_device(
    audio_device: &AudioDevice,
) -> Result<(cpal::Device, cpal::SupportedStreamConfig)> {
    let wasapi_host = cpal::host_from_id(cpal::HostId::Wasapi)
        .map_err(|e| anyhow!("Failed to create WASAPI host: {}", e))?;

    let base_name = &audio_device.name;
    info!("Looking for Windows device with name: {}", base_name);

    match audio_device.device_type {
        DeviceType::Input => {
            for device in wasapi_host.input_devices()? {
                if let Ok(name) = device.name() {
                    info!("Checking input device: {}", name);
                    if name == *base_name || name.contains(base_name) {
                        info!("Found matching input device: {}", name);

                        match device.default_input_config() {
                            Ok(default_config) => {
                                info!("Using default input config: {:?}", default_config);
                                return Ok((device, default_config));
                            }
                            Err(e) => {
                                warn!("Failed to get default input config: {}. Trying supported configs...", e);

                                if let Ok(supported_configs) = device.supported_input_configs() {
                                    let mut configs: Vec<_> = supported_configs.collect();
                                    if !configs.is_empty() {
                                        info!(
                                            "Found {} supported input configurations",
                                            configs.len()
                                        );

                                        // Try F32 format with 2 channels first
                                        for config in &configs {
                                            if config.sample_format() == cpal::SampleFormat::F32
                                                && config.channels() == 2
                                            {
                                                let config = config.with_max_sample_rate();
                                                info!(
                                                    "Using stereo F32 input config: {:?}",
                                                    config
                                                );
                                                return Ok((device, config));
                                            }
                                        }

                                        // Then try any F32 format
                                        for config in &configs {
                                            if config.sample_format() == cpal::SampleFormat::F32 {
                                                let config = config.with_max_sample_rate();
                                                info!("Using F32 input config: {:?}", config);
                                                return Ok((device, config));
                                            }
                                        }

                                        // Use first available config
                                        let config = configs[0].with_max_sample_rate();
                                        info!("Using fallback input config: {:?}", config);
                                        return Ok((device, config));
                                    }
                                }

                                return Err(anyhow!(
                                    "No compatible input configuration found for device: {}",
                                    name
                                ));
                            }
                        }
                    }
                }
            }

            // Try default input device as fallback
            info!("No matching input device found, trying default input device");
            if let Some(default_device) = wasapi_host.default_input_device() {
                if let Ok(name) = default_device.name() {
                    info!("Using default input device: {}", name);
                    if let Ok(config) = default_device.default_input_config() {
                        return Ok((default_device, config));
                    } else if let Ok(supported_configs) = default_device.supported_input_configs() {
                        if let Some(config) = supported_configs.into_iter().next() {
                            return Ok((default_device, config.with_max_sample_rate()));
                        }
                    }
                }
            }
        }
        DeviceType::Output => {
            for device in wasapi_host.output_devices()? {
                if let Ok(name) = device.name() {
                    info!("Checking output device: {}", name);
                    if name == *base_name || name.contains(base_name) {
                        info!("Found matching output device: {}", name);

                        // For output devices, use them in loopback mode
                        if let Ok(supported_configs) = device.supported_output_configs() {
                            let mut configs: Vec<_> = supported_configs.collect();
                            if !configs.is_empty() {
                                info!("Found {} supported output configurations", configs.len());

                                // Try F32 format with 2 channels first
                                for config in &configs {
                                    if config.sample_format() == cpal::SampleFormat::F32
                                        && config.channels() == 2
                                    {
                                        let config = config.with_max_sample_rate();
                                        info!("Using stereo F32 output config: {:?}", config);
                                        return Ok((device, config));
                                    }
                                }

                                // Then try any F32 format
                                for config in &configs {
                                    if config.sample_format() == cpal::SampleFormat::F32 {
                                        let config = config.with_max_sample_rate();
                                        info!("Using F32 output config: {:?}", config);
                                        return Ok((device, config));
                                    }
                                }

                                // Use first available config
                                let config = configs[0].with_max_sample_rate();
                                info!("Using fallback output config: {:?}", config);
                                return Ok((device, config));
                            }
                        }

                        // Try default config as fallback
                        if let Ok(default_config) = device.default_output_config() {
                            info!("Using default output config: {:?}", default_config);
                            return Ok((device, default_config));
                        }
                    }
                }
            }

            // Try default output device as fallback
            info!("No matching output device found, trying default output device");
            if let Some(default_device) = wasapi_host.default_output_device() {
                if let Ok(name) = default_device.name() {
                    info!("Using default output device: {}", name);
                    if let Ok(config) = default_device.default_output_config() {
                        return Ok((default_device, config));
                    } else if let Ok(supported_configs) = default_device.supported_output_configs()
                    {
                        if let Some(config) = supported_configs.into_iter().next() {
                            return Ok((default_device, config.with_max_sample_rate()));
                        }
                    }
                }
            }
        }
    }

    Err(anyhow!(
        "Device not found or no compatible configuration available: {}",
        audio_device.name
    ))
}

pub async fn get_device_and_config(
    audio_device: &AudioDevice,
) -> Result<(cpal::Device, cpal::SupportedStreamConfig)> {
    #[cfg(target_os = "windows")]
    {
        return get_windows_device(audio_device);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let host = cpal::default_host();

        match audio_device.device_type {
            DeviceType::Input => {
                for device in host.input_devices()? {
                    if let Ok(name) = device.name() {
                        if name == audio_device.name {
                            let default_config = device.default_input_config().map_err(|e| {
                                anyhow!("Failed to get default input config: {}", e)
                            })?;
                            return Ok((device, default_config));
                        }
                    }
                }
            }
            DeviceType::Output => {
                #[cfg(target_os = "macos")]
                {
                    if let Ok(host) = cpal::host_from_id(cpal::HostId::ScreenCaptureKit) {
                        for device in host.input_devices()? {
                            if let Ok(name) = device.name() {
                                if name == audio_device.name {
                                    let default_config =
                                        device.default_input_config().map_err(|e| {
                                            anyhow!("Failed to get default input config: {}", e)
                                        })?;
                                    return Ok((device, default_config));
                                }
                            }
                        }
                    }
                }

                #[cfg(target_os = "linux")]
                {
                    // For Linux, use PulseAudio monitor sources
                    if let Ok(pulse_host) = cpal::host_from_id(cpal::HostId::Pulse) {
                        for device in pulse_host.input_devices()? {
                            if let Ok(name) = device.name() {
                                if name == audio_device.name {
                                    let default_config =
                                        device.default_input_config().map_err(|e| {
                                            anyhow!("Failed to get default input config: {}", e)
                                        })?;
                                    return Ok((device, default_config));
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(anyhow!("Device not found: {}", audio_device.name))
    }
}

pub fn audio_to_mono(input: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        input.to_vec()
    } else {
        let mono_len = input.len() / channels as usize;
        let mut mono = Vec::with_capacity(mono_len);

        for frame in input.chunks_exact(channels as usize) {
            let sum: f32 = frame.iter().sum();
            mono.push(sum / channels as f32);
        }

        mono
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_audio_chunk_without_receivers_is_not_an_error() {
        let (sender, _) = broadcast::channel(1);

        assert!(publish_audio_chunk(&sender, vec![0.1, -0.1]));
    }
}
