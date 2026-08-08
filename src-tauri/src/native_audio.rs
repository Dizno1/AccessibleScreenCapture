// Native Windows system-audio capture via WASAPI loopback.
//
// Kept as its own module, isolated from native_capture.rs's video
// pipeline: if anything in here fails to compile or fails at runtime,
// the video-only proof (already independently verified on real
// hardware: ~5s, 2560x1600, 60fps, 299 HEVC frames) is never at risk.
// native_capture.rs calls start_loopback_capture() and treats any
// Err as "video succeeded, audio did not" rather than a hard failure.
//
// DIAGNOSTIC ONLY - NOT WIRED INTO THE ENCODER. This module proves
// (or disproves) that this app's own WASAPI capture mechanism works -
// initializes, finds the right device, and receives real audio
// buffers. It does not feed those buffers into the video encoder.
//
// THREE COMPILE-CORRECTNESS FIXES THIS ROUND:
//
//   1. initialize_mta() returns a raw HRESULT, not a Result - the
//      previous `if let Err(e) = initialize_mta()` couldn't have
//      compiled. Fixed using HRESULT's own `.ok()` conversion (the
//      standard windows-rs pattern: HRESULT::ok(self) ->
//      windows::core::Result<()>), then mapped to our own String
//      error type.
//
//   2. COM/object lifetime and thread ownership. `AudioClient` is
//      documented `!Send` (and `!Sync`) - it cannot be created on one
//      thread and then captured into a `std::thread::spawn` closure,
//      which requires everything it captures to be `Send`. The
//      previous version did exactly that (created Device/AudioClient/
//      AudioCaptureClient on the calling thread, then implicitly
//      moved them into the spawned worker) - this would not have
//      compiled. Every WASAPI/COM object is now created *and* used
//      entirely inside the worker thread itself: the thread calls
//      initialize_mta(), builds the DeviceEnumerator, gets the
//      device, the AudioClient, the mix format, initializes capture,
//      gets the AudioCaptureClient, starts the stream, reads packets,
//      and stops the stream - all without any of those objects ever
//      crossing a thread boundary as values. The caller still needs
//      to know whether initialization succeeded before treating the
//      feature as available, so a one-shot channel carries just the
//      Result<AudioCaptureDiagnostics, String> back once
//      initialization finishes, before the worker moves on to the
//      read loop.
//
//   3. Mix format. The device's own `get_mixformat()` result is now
//      passed directly to `initialize_client()`, per wasapi's own
//      documentation that it "should always be accepted" in shared
//      mode - no longer discarded in favor of constructing a new
//      WaveFormat that forced SampleType::Float. No format conversion
//      is implemented in this pass; whatever format the device
//      reports is what gets captured.
//
// The rest of the pipeline (open the RENDER endpoint but initialize
// in the CAPTURE direction for loopback, get_next_packet_size() then
// read_from_device() for buffer reads, get_audiocaptureclient(),
// EventsShared streaming) is unchanged from the previous round's
// correction against the real wasapi 0.23.0 API.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wasapi::{initialize_mta, DeviceEnumerator, Direction, StreamMode};

pub struct AudioChunk {
    pub pcm: Vec<u8>,
    pub frames: u32,
    pub elapsed: Duration,
}

#[derive(Clone, serde::Serialize)]
pub struct AudioCaptureDiagnostics {
    #[serde(rename = "audioRequested")]
    pub audio_requested: bool,
    #[serde(rename = "wasapiInitialized")]
    pub wasapi_initialized: bool,
    #[serde(rename = "renderEndpointName")]
    pub render_endpoint_name: Option<String>,
    #[serde(rename = "mixSampleRate")]
    pub mix_sample_rate: Option<u32>,
    #[serde(rename = "mixChannels")]
    pub mix_channels: Option<u16>,
    #[serde(rename = "mixBitsPerSample")]
    pub mix_bits_per_sample: Option<u16>,
    #[serde(rename = "buffersCaptured")]
    pub buffers_captured: u32,
    #[serde(rename = "framesCaptured")]
    pub frames_captured: u64,
    #[serde(rename = "audioError")]
    pub audio_error: Option<String>,
}

impl Default for AudioCaptureDiagnostics {
    fn default() -> Self {
        AudioCaptureDiagnostics {
            audio_requested: false,
            wasapi_initialized: false,
            render_endpoint_name: None,
            mix_sample_rate: None,
            mix_channels: None,
            mix_bits_per_sample: None,
            buffers_captured: 0,
            frames_captured: 0,
            audio_error: None,
        }
    }
}

/// Starts WASAPI loopback capture of the default render (playback)
/// endpoint on a dedicated background thread - every WASAPI/COM
/// object is created and used entirely within that thread (see the
/// COM/thread ownership note above). Blocks briefly waiting for the
/// worker to report whether initialization succeeded, then returns a
/// receiver for captured PCM chunks and a handle to request stop;
/// capture continues on the worker thread until `stop_flag` is set.
/// Never panics; every failure path returns Err with a specific
/// message instead.
pub fn start_loopback_capture() -> Result<(Receiver<AudioChunk>, Arc<AtomicBool>, AudioCaptureDiagnostics), String> {
    let (chunk_tx, chunk_rx): (Sender<AudioChunk>, Receiver<AudioChunk>) = mpsc::channel();
    let (init_tx, init_rx) = mpsc::channel::<Result<AudioCaptureDiagnostics, String>>();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_for_thread = stop_flag.clone();

    std::thread::spawn(move || {
        // initialize_mta() returns a raw HRESULT, not a Result -
        // .ok() is the standard windows-rs conversion to
        // windows::core::Result<()>.
        if let Err(e) = initialize_mta().ok() {
            let _ = init_tx.send(Err(format!("Could not initialize COM (MTA) for WASAPI: {e}")));
            return;
        }

        let enumerator = match DeviceEnumerator::new() {
            Ok(enumerator) => enumerator,
            Err(e) => {
                let _ = init_tx.send(Err(format!("Could not create a device enumerator: {e}")));
                return;
            }
        };

        let device = match enumerator.get_default_device(&Direction::Render) {
            Ok(device) => device,
            Err(e) => {
                let _ = init_tx.send(Err(format!("Could not get the default playback device: {e}")));
                return;
            }
        };
        let render_endpoint_name = device.get_friendlyname().ok();

        let mut audio_client = match device.get_iaudioclient() {
            Ok(client) => client,
            Err(e) => {
                let _ = init_tx.send(Err(format!("Could not open an audio client on the default playback device: {e}")));
                return;
            }
        };

        // The device's own mix format, used directly and unmodified -
        // "should always be accepted" in shared mode per wasapi's own
        // documentation. No format conversion in this pass.
        let mix_format = match audio_client.get_mixformat() {
            Ok(format) => format,
            Err(e) => {
                let _ = init_tx.send(Err(format!("Could not read the device's mix format: {e}")));
                return;
            }
        };

        let sample_rate = mix_format.get_samplespersec();
        let channels = mix_format.get_nchannels();
        let bits_per_sample = mix_format.get_bitspersample();
        let block_align = mix_format.get_blockalign();

        let (_default_period, min_period) = match audio_client.get_device_period() {
            Ok(periods) => periods,
            Err(e) => {
                let _ = init_tx.send(Err(format!("Could not read the device's audio period: {e}")));
                return;
            }
        };

        // PollingShared, not EventsShared: EventsShared is
        // event-driven and requires AudioClient::set_get_eventhandle()
        // followed by waiting on the returned handle - neither of
        // which this capture loop does. The loop below is a plain
        // poll (get_next_packet_size/read_from_device/sleep), which
        // is exactly what PollingShared is for. No event-handle
        // machinery is introduced in this pass.
        let mode = StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns: min_period,
        };

        // The classic WASAPI loopback trick, per Microsoft's own
        // documented guidance: open the RENDER endpoint (the device
        // that's actually playing sound) but initialize the client in
        // the CAPTURE direction - this is what makes it a loopback
        // capture of "whatever this device is currently playing"
        // rather than a normal playback stream.
        if let Err(e) = audio_client.initialize_client(&mix_format, &Direction::Capture, &mode) {
            let _ = init_tx.send(Err(format!("Could not initialize the loopback capture stream: {e}")));
            return;
        }

        let capture_client = match audio_client.get_audiocaptureclient() {
            Ok(client) => client,
            Err(e) => {
                let _ = init_tx.send(Err(format!("Could not obtain the audio capture client: {e}")));
                return;
            }
        };

        if let Err(e) = audio_client.start_stream() {
            let _ = init_tx.send(Err(format!("Could not start the loopback capture stream: {e}")));
            return;
        }

        let diagnostics = AudioCaptureDiagnostics {
            audio_requested: true,
            wasapi_initialized: true,
            render_endpoint_name,
            mix_sample_rate: Some(sample_rate),
            mix_channels: Some(channels),
            mix_bits_per_sample: Some(bits_per_sample),
            buffers_captured: 0,
            frames_captured: 0,
            audio_error: None,
        };

        // Initialization succeeded - report that back before entering
        // the read loop. All WASAPI objects (audio_client,
        // capture_client) stay right here on this thread for the rest
        // of its life; they're never sent anywhere.
        if init_tx.send(Ok(diagnostics)).is_err() {
            // Caller already gave up waiting - nothing further to do.
            return;
        }

        let capture_start = Instant::now();
        while !stop_flag_for_thread.load(Ordering::SeqCst) {
            match capture_client.get_next_packet_size() {
                Ok(Some(frames_available)) if frames_available > 0 => {
                    let mut buffer = vec![0u8; frames_available as usize * block_align as usize];
                    match capture_client.read_from_device(&mut buffer) {
                        Ok(_flags) => {
                            let _ = chunk_tx.send(AudioChunk {
                                pcm: buffer,
                                frames: frames_available,
                                elapsed: capture_start.elapsed(),
                            });
                        }
                        Err(_) => {
                            // A transient read failure here doesn't end
                            // the whole capture attempt - whatever was
                            // captured before this point is still kept.
                        }
                    }
                }
                _ => {
                    // Nothing waiting yet, or the packet-size query
                    // itself failed transiently - either way, just
                    // wait for the next poll rather than treating this
                    // as fatal.
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let _ = audio_client.stop_stream();
    });

    match init_rx.recv() {
        Ok(Ok(diagnostics)) => Ok((chunk_rx, stop_flag, diagnostics)),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("Audio worker thread ended unexpectedly during initialization.".to_string()),
    }
}
