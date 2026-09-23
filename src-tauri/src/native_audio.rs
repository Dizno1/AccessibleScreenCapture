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
// AUDIO-DURATION FIX THIS ROUND. A real test captured only ~3.73s of
// audio from a requested ~5s window, despite the stream running the
// whole time. Root cause: the read loop read at most ONE queued
// packet per poll tick (previously every 10ms) before sleeping again -
// if WASAPI queued more than one packet's worth of audio between
// ticks, only the first got read and the rest were silently lost when
// the device's internal buffer wrapped around. Fixed by draining every
// currently-queued packet in a tight inner loop before each sleep, so
// the poll tick only determines how often we check for new work, not
// how much of it we're allowed to consume once we find it. Also added
// a final drain pass after stop is signaled but before the stream is
// actually stopped, to catch anything that arrived in the last brief
// window. Poll interval also tightened from 10ms to 5ms as a modest,
// low-risk additional safety margin.
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
use wasapi::{initialize_mta, AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

pub struct AudioChunk {
    pub pcm: Vec<u8>,
    pub frames: u32,
    pub elapsed: Duration,
    // Was WASAPI's own silent flag set on this packet - see the
    // BufferFlags handling in the read loop below. Tracked
    // separately from has_signal so diagnostics can report both the
    // raw WASAPI signal and the actual byte-level result distinctly.
    pub wasapi_silent: bool,
    // Does this chunk's PCM contain at least one non-zero byte, after
    // any WASAPI-silent zero-fill has already been applied. A
    // WASAPI-silent chunk is always false here (it was just zeroed).
    // A chunk WASAPI did not flag silent could still be all zeros in
    // practice (e.g. a genuinely quiet moment) - that's fine and
    // expected, not itself a failure signal; what matters for the
    // overall recording is whether ANY retained chunk has real
    // non-zero content, checked downstream in native_recording.rs.
    pub has_signal: bool,
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
    // WASAPI-silent tracking, distinguished from real signal presence
    // (see AudioChunk's own field docs in this module) - a packet
    // being present is not proof it contains audible content.
    #[serde(rename = "wasapiSilentPackets")]
    pub wasapi_silent_packets: u32,
    #[serde(rename = "wasapiSilentFrames")]
    pub wasapi_silent_frames: u64,
    #[serde(rename = "nonSilentSignalDetected")]
    pub non_silent_signal_detected: bool,
    #[serde(rename = "capturedSpanSeconds")]
    pub captured_span_seconds: Option<f64>,
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
            wasapi_silent_packets: 0,
            wasapi_silent_frames: 0,
            non_silent_signal_detected: false,
            captured_span_seconds: None,
            audio_error: None,
        }
    }
}

/// Which kind of WASAPI capture to start - the two differ only in
/// which endpoint is opened and how it's labeled; the rest of the
/// pipeline (mix format, PollingShared mode, read loop, drain-on-stop)
/// is identical, since both are, from WASAPI's perspective, just a
/// capture-direction stream on some endpoint.
enum CaptureKind {
    /// The classic WASAPI loopback trick: open the RENDER endpoint
    /// (what's currently playing sound) but initialize in the CAPTURE
    /// direction - captures system audio, per Microsoft's own
    /// documented guidance.
    SystemLoopback,
    /// Process-scoped loopback. Captures audio rendered by the selected
    /// application process and its child processes, excluding unrelated
    /// system audio such as screen-reader speech from another process.
    ApplicationProcess(u32),
    /// A genuine capture/input endpoint (a real microphone or other
    /// recording device) - opened directly in the CAPTURE direction,
    /// no loopback trick needed since the device already is a capture
    /// endpoint. Some(id) selects a specific device by its real
    /// WASAPI device ID (see list_microphone_devices below) - if
    /// that device can't be resolved (unplugged, disabled), this
    /// fails explicitly rather than silently falling back to the
    /// Windows default. None uses the Windows default recording
    /// device.
    Microphone(Option<String>),
}

/// Starts WASAPI capture on a dedicated background thread - every
/// WASAPI/COM object is created and used entirely within that thread
/// (see the COM/thread ownership note above). Blocks briefly waiting
/// for the worker to report whether initialization succeeded, then
/// returns a receiver for captured PCM chunks, a handle to request
/// stop, and the Instant the worker began capturing - the caller uses
/// that Instant plus each AudioChunk's `elapsed` to convert chunk
/// timestamps into absolute Instants comparable to video's own
/// first-frame timestamp, for capture-origin alignment. Capture
/// continues on the worker thread until `stop_flag` is set. Never
/// panics; every failure path returns Err with a specific message
/// instead.
fn start_capture(kind: CaptureKind) -> Result<(Receiver<AudioChunk>, Arc<AtomicBool>, AudioCaptureDiagnostics, Instant), String> {
    let (chunk_tx, chunk_rx): (Sender<AudioChunk>, Receiver<AudioChunk>) = mpsc::channel();
    let (init_tx, init_rx) = mpsc::channel::<Result<(AudioCaptureDiagnostics, Instant), String>>();
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

        // Beta 24 adds Windows process-loopback capture for a selected application.
        // Unlike ordinary endpoint loopback, process loopback does not expose a usable
        // MixFormat or device period, so it uses an explicit 48 kHz, stereo, 32-bit
        // float format with WASAPI autoconversion. The existing endpoint paths remain
        // unchanged.
        let (mut audio_client, mix_format, endpoint_name, device_kind_label, buffer_duration_hns) =
            match &kind {
                CaptureKind::ApplicationProcess(pid) => {
                    let client = match AudioClient::new_application_loopback_client(*pid, true) {
                        Ok(client) => client,
                        Err(e) => {
                            let _ = init_tx.send(Err(format!("Could not open selected application audio for process {pid}: {e}")));
                            return;
                        }
                    };
                    let format = WaveFormat::new(32, 32, &SampleType::Float, 48_000, 2, None);
                    (client, format, Some(format!("Selected application process {pid}")), "selected application", 200_000)
                }
                CaptureKind::SystemLoopback | CaptureKind::Microphone(_) => {
                    let enumerator = match DeviceEnumerator::new() {
                        Ok(enumerator) => enumerator,
                        Err(e) => {
                            let _ = init_tx.send(Err(format!("Could not create a device enumerator: {e}")));
                            return;
                        }
                    };
                    let enumerator_direction = match &kind {
                        CaptureKind::SystemLoopback => Direction::Render,
                        CaptureKind::Microphone(_) => Direction::Capture,
                        CaptureKind::ApplicationProcess(_) => unreachable!(),
                    };
                    let label = match &kind {
                        CaptureKind::SystemLoopback => "playback",
                        CaptureKind::Microphone(_) => "microphone",
                        CaptureKind::ApplicationProcess(_) => unreachable!(),
                    };
                    let selected_device_id = match &kind {
                        CaptureKind::Microphone(Some(id)) => Some(id.clone()),
                        _ => None,
                    };
                    let device = if let Some(id) = &selected_device_id {
                        match enumerator.get_device(id) {
                            Ok(device) => device,
                            Err(e) => {
                                let _ = init_tx.send(Err(format!("The selected microphone device is unavailable: {e}")));
                                return;
                            }
                        }
                    } else {
                        match enumerator.get_default_device(&enumerator_direction) {
                            Ok(device) => device,
                            Err(e) => {
                                let _ = init_tx.send(Err(format!("Could not get the default {label} device: {e}")));
                                return;
                            }
                        }
                    };
                    let endpoint_name = device.get_friendlyname().ok();
                    let client = match device.get_iaudioclient() {
                        Ok(client) => client,
                        Err(e) => {
                            let _ = init_tx.send(Err(format!("Could not open an audio client on the default {label} device: {e}")));
                            return;
                        }
                    };
                    let format = match client.get_mixformat() {
                        Ok(format) => format,
                        Err(e) => {
                            let _ = init_tx.send(Err(format!("Could not read the device's mix format: {e}")));
                            return;
                        }
                    };
                    let (_default_period, min_period) = match client.get_device_period() {
                        Ok(periods) => periods,
                        Err(e) => {
                            let _ = init_tx.send(Err(format!("Could not read the device's audio period: {e}")));
                            return;
                        }
                    };
                    (client, format, endpoint_name, label, min_period)
                }
            };

        let sample_rate = mix_format.get_samplespersec();
        let channels = mix_format.get_nchannels();
        let bits_per_sample = mix_format.get_bitspersample();
        let block_align = mix_format.get_blockalign();

        let mode = StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns,
        };

        if let Err(e) = audio_client.initialize_client(&mix_format, &Direction::Capture, &mode) {
            let _ = init_tx.send(Err(format!("Could not initialize the {device_kind_label} capture stream: {e}")));
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
            let _ = init_tx.send(Err(format!("Could not start the {device_kind_label} capture stream: {e}")));
            return;
        }

        let diagnostics = AudioCaptureDiagnostics {
            audio_requested: true,
            wasapi_initialized: true,
            render_endpoint_name: endpoint_name,
            mix_sample_rate: Some(sample_rate),
            mix_channels: Some(channels),
            mix_bits_per_sample: Some(bits_per_sample),
            buffers_captured: 0,
            frames_captured: 0,
            wasapi_silent_packets: 0,
            wasapi_silent_frames: 0,
            non_silent_signal_detected: false,
            captured_span_seconds: None,
            audio_error: None,
        };

        // Initialization succeeded - report that back before entering
        // the read loop. All WASAPI objects (audio_client,
        // capture_client) stay right here on this thread for the rest
        // of its life; they're never sent anywhere. capture_start is
        // sent back too, so the caller can convert each AudioChunk's
        // elapsed-since-capture_start into an absolute Instant
        // comparable to video's own FIRST_FRAME_AT, for capture-origin
        // alignment.
        let capture_start = Instant::now();
        if init_tx.send(Ok((diagnostics, capture_start))).is_err() {
            // Caller already gave up waiting - nothing further to do.
            return;
        }

        while !stop_flag_for_thread.load(Ordering::SeqCst) {
            // Drain every packet currently queued before sleeping -
            // reading at most one packet per poll tick could let
            // the device's buffer fill faster than we drain it,
            // silently losing packets and shortening the captured
            // audio even while the stream keeps running the whole
            // time. This inner loop keeps reading until the queue is
            // genuinely empty, then the outer loop sleeps once.
            loop {
                match capture_client.get_next_packet_size() {
                    Ok(Some(frames_available)) if frames_available > 0 => {
                        let mut buffer = vec![0u8; frames_available as usize * block_align as usize];
                        match capture_client.read_from_device(&mut buffer) {
                            Ok((frames_read, buffer_info)) => {
                                // BufferFlags - confirmed real API
                                // (docs.rs/wasapi's own BufferFlags
                                // page: data_discontinuity, silent,
                                // timestamp_error). Previously
                                // discarded via a wildcard binding,
                                // never inspected. Per Microsoft's own
                                // WASAPI documentation, when the
                                // silent flag is set the buffer's
                                // actual byte content is not
                                // guaranteed to be real silence (it
                                // may be stale data from a previous
                                // buffer) - the correct handling is to
                                // treat it as silence explicitly, not
                                // trust whatever bytes happen to be
                                // there. Zeroed here rather than
                                // trusting unverified buffer content.
                                if buffer_info.flags.silent {
                                    buffer.fill(0);
                                }
                                // Checked AFTER any silent-flag
                                // zero-fill above, so a WASAPI-silent
                                // chunk is always has_signal=false -
                                // see the AudioChunk field docs for
                                // why this specific ordering matters.
                                let has_signal = !buffer_info.flags.silent && buffer.iter().any(|&b| b != 0);
                                let _ = chunk_tx.send(AudioChunk {
                                    pcm: buffer,
                                    frames: frames_read,
                                    elapsed: capture_start.elapsed(),
                                    wasapi_silent: buffer_info.flags.silent,
                                    has_signal,
                                });
                            }
                            Err(_) => {
                                // A transient read failure here doesn't end
                                // the whole capture attempt - whatever was
                                // captured before this point is still kept.
                                break;
                            }
                        }
                    }
                    _ => break, // queue empty, or the size query itself failed transiently
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        // Stop was signaled - one last drain pass to catch anything
        // that arrived in the brief window between the previous
        // drain and the outer loop noticing stop_flag, before the
        // stream is actually stopped.
        loop {
            match capture_client.get_next_packet_size() {
                Ok(Some(frames_available)) if frames_available > 0 => {
                    let mut buffer = vec![0u8; frames_available as usize * block_align as usize];
                    match capture_client.read_from_device(&mut buffer) {
                        Ok((frames_read, buffer_info)) => {
                            // Same silent-flag handling as the main
                            // drain loop above - see that comment for
                            // the full explanation.
                            if buffer_info.flags.silent {
                                buffer.fill(0);
                            }
                            let has_signal = !buffer_info.flags.silent && buffer.iter().any(|&b| b != 0);
                            let _ = chunk_tx.send(AudioChunk {
                                pcm: buffer,
                                frames: frames_read,
                                elapsed: capture_start.elapsed(),
                                wasapi_silent: buffer_info.flags.silent,
                                has_signal,
                            });
                        }
                        Err(_) => break,
                    }
                }
                _ => break,
            }
        }

        let _ = audio_client.stop_stream();
    });

    match init_rx.recv() {
        Ok(Ok((diagnostics, capture_start))) => Ok((chunk_rx, stop_flag, diagnostics, capture_start)),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("Audio worker thread ended unexpectedly during initialization.".to_string()),
    }
}

/// Starts WASAPI loopback capture of the default render (playback)
/// endpoint - captures system audio. See `start_capture` for the
/// shared implementation.
pub fn start_loopback_capture() -> Result<(Receiver<AudioChunk>, Arc<AtomicBool>, AudioCaptureDiagnostics, Instant), String> {
    start_capture(CaptureKind::SystemLoopback)
}

/// Starts WASAPI capture of a microphone (capture) endpoint - a
/// specific device if `device_id` is Some (see list_microphone_devices
/// for how to obtain a real device ID), or the Windows default
/// recording device if None. See `start_capture` for the shared
/// implementation.
/// Starts process-scoped loopback capture for one selected application.
/// The process tree is included so browser/application helper processes that
/// belong beneath the selected audio process can be captured as well.
pub fn start_application_capture(process_id: u32) -> Result<(Receiver<AudioChunk>, Arc<AtomicBool>, AudioCaptureDiagnostics, Instant), String> {
    start_capture(CaptureKind::ApplicationProcess(process_id))
}

pub fn start_microphone_capture(device_id: Option<String>) -> Result<(Receiver<AudioChunk>, Arc<AtomicBool>, AudioCaptureDiagnostics, Instant), String> {
    start_capture(CaptureKind::Microphone(device_id))
}

/// One available microphone (capture-direction) device, for populating
/// a device-selection control. `id` is the real WASAPI device ID
/// (stable across app restarts, suitable for persisting a selection)
/// - pass it back to start_microphone_capture to use this specific
/// device.
#[derive(serde::Serialize)]
pub struct MicrophoneDeviceInfo {
    pub id: String,
    pub name: String,
}

/// Enumerates all active capture-direction (recording/input) devices,
/// via DeviceEnumerator::get_device_collection() - confirmed real API
/// (docs.rs/wasapi's own DeviceEnumerator page lists this method
/// explicitly: "Get an IMMDeviceCollection of all active playback or
/// capture devices"). A device whose name can't be read is skipped
/// rather than failing the whole enumeration - one bad device
/// shouldn't hide every other one from the picker.
pub fn list_microphone_devices() -> Result<Vec<MicrophoneDeviceInfo>, String> {
    // Tauri command handlers may run on a thread whose COM apartment has
    // already been initialized differently. Enumerate on our own worker
    // thread so WASAPI always gets the MTA apartment it expects instead of
    // failing with RPC_E_CHANGED_MODE and leaving the microphone picker hidden.
    let (tx, rx) = mpsc::channel::<Result<Vec<MicrophoneDeviceInfo>, String>>();
    std::thread::spawn(move || {
        let result = (|| -> Result<Vec<MicrophoneDeviceInfo>, String> {
            initialize_mta().ok().map_err(|e| format!("Could not initialize COM (MTA) for device enumeration: {e}"))?;

            let enumerator = DeviceEnumerator::new()
                .map_err(|e| format!("Could not create a device enumerator: {e}"))?;
            let collection = enumerator
                .get_device_collection(&Direction::Capture)
                .map_err(|e| format!("Could not enumerate capture devices: {e}"))?;

            let mut devices = Vec::new();
            for device_result in &collection {
                let device = match device_result {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let id = match device.get_id() {
                    Ok(id) => id,
                    Err(_) => continue,
                };
                let name = device
                    .get_friendlyname()
                    .unwrap_or_else(|_| "Unnamed device".to_string());
                devices.push(MicrophoneDeviceInfo { id, name });
            }
            Ok(devices)
        })();
        let _ = tx.send(result);
    });

    rx.recv()
        .map_err(|e| format!("Microphone enumeration worker ended unexpectedly: {e}"))?
}

#[tauri::command]
pub fn list_native_microphones() -> Result<Vec<MicrophoneDeviceInfo>, String> {
    list_microphone_devices()
}

#[derive(Clone, serde::Serialize)]
pub struct NativeAudioApplication {
    pub process_id: u32,
    pub name: String,
}

#[cfg(target_os = "windows")]
fn process_name_for_pid(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = vec![0u16; 32768];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, windows::core::PWSTR(buffer.as_mut_ptr()), &mut size);
        let _ = CloseHandle(handle);
        if result.is_err() || size == 0 { return None; }
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        let name = std::path::Path::new(&path).file_name()?.to_string_lossy().to_string();
        Some(name.trim_end_matches(".exe").to_string())
    }
}

/// Lists applications that currently own an audio session on the default
/// Windows playback endpoint. This intentionally follows the Windows audio
/// session list rather than enumerating every running process, keeping the
/// selector short and relevant for Meet, Teams, Zoom, browsers, and media apps.
#[tauri::command]
pub fn list_native_audio_applications() -> Result<Vec<NativeAudioApplication>, String> {
    std::thread::spawn(|| -> Result<Vec<NativeAudioApplication>, String> {
        if let Err(e) = initialize_mta().ok() {
            return Err(format!("Could not initialize Windows audio application discovery: {e}"));
        }
        let enumerator = DeviceEnumerator::new().map_err(|e| format!("Could not create audio device enumerator: {e}"))?;
        let device = enumerator.get_default_device(&Direction::Render).map_err(|e| format!("Could not get default playback device: {e}"))?;
        let manager = device.get_iaudiosessionmanager().map_err(|e| format!("Could not open Windows audio session manager: {e}"))?;
        let sessions = manager.get_audiosessionenumerator().map_err(|e| format!("Could not enumerate Windows audio sessions: {e}"))?;
        let count = sessions.get_count().map_err(|e| format!("Could not count Windows audio sessions: {e}"))?;
        let mut apps = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for index in 0..count {
            let session = match sessions.get_session(index) { Ok(v) => v, Err(_) => continue };
            let pid = match session.get_process_id() { Ok(v) if v != 0 => v, _ => continue };
            if !seen.insert(pid) { continue; }
            let display = session.get_display_name().ok().filter(|v| !v.trim().is_empty());
            let name = display.or_else(|| process_name_for_pid(pid)).unwrap_or_else(|| format!("Process {pid}"));
            apps.push(NativeAudioApplication { process_id: pid, name });
        }
        apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.process_id.cmp(&b.process_id)));
        Ok(apps)
    }).join().map_err(|_| "Windows audio application discovery thread failed.".to_string())?
}

