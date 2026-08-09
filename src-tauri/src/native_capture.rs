// Native Windows screen capture - EXPERIMENTAL PROOF, still not wired
// into the working recorder.
//
// STOP MECHANISM REDESIGNED THIS ROUND - confirmed root cause of the
// ~24-second test. The previous version evaluated the 5-second stop
// condition *inside* on_frame_arrived() - the only code that runs
// while ProofHandler::start() blocks the calling thread. Since
// windows-capture only delivers a callback "when required" (its own
// documented behavior, not a bug), a static desktop with no second
// callback for many seconds meant the time-check simply never ran
// until whatever eventually triggered another frame - matching
// exactly what the real Windows test showed (first frame started the
// window, nothing else happened for ~19 more seconds, then a frame
// arrived, the check finally fired, and only then did stop/finalize
// happen). No amount of adjusting *what* the check compared against
// could fix this - the check itself was only reachable from inside a
// callback that might not fire.
//
// Fixed by using windows-capture's own non-blocking entry point,
// `start_free_threaded()`, which runs the capture on its own
// dedicated thread and returns a `CaptureControl` handle usable from
// the *calling* thread - completely independent of whether any frame
// callback ever fires. The calling thread now does a plain
// `std::thread::sleep` for the requested duration, then calls
// `.stop()` (and `.wait()`, to block until the capture thread and its
// on_closed cleanup have genuinely finished) on that handle. This is
// the crate's own documented mechanism for exactly this situation,
// not a homegrown thread/timer workaround - `start_free_threaded()`
// exists specifically so capture can be controlled from outside its
// own callback. The exact `CaptureControl` method signatures
// (`stop`/`wait`) were not confirmed against Rust source directly -
// they're inferred from the crate's own Python bindings, which wrap
// this same Rust type and expose `stop()`/`wait()`/`is_finished()`
// under those exact names. If the real signatures differ, expect a
// scoped, easily-isolated compiler error here, same as every other
// round.
//
// Encoder finalization moved to on_closed() (fires when the session
// actually ends) rather than being decided inside on_frame_arrived(),
// since stopping is no longer something on_frame_arrived() decides at
// all.
//
// FRAME DELIVERY RATE. windows-capture's own README lists "Only
// updates the frame when required" as a headline feature in every
// version checked - deliberate, documented, change-driven delivery.
// MinimumUpdateIntervalSettings::Custom(Duration) caps the MAXIMUM
// rate at which real changes get reported (a ceiling on how often a
// genuine content change can produce a new callback) - it does NOT
// force or guarantee a callback when nothing on screen has actually
// changed. On a fully static desktop, Custom(33ms) does not by itself
// make callbacks arrive every 33ms; it only prevents them from
// arriving faster than that when real changes are happening. It is
// left set here (~30fps ceiling) as a reasonable maximum rate for the
// proof, but it is not what fixes - and does not claim to fix - the
// low-callback-count problem; the external stop mechanism above is
// what makes this proof stop reliably regardless of callback
// frequency. Whether WGC delivers a genuinely continuous sequence of
// callbacks on real desktop activity (as opposed to a static one) is
// still an open question this round's diagnostics are meant to help
// answer, not something changed or assumed fixed here.
//
// DirtyRegionSettings is left at ::Default -
// it governs *how* changed regions are reported (report-only vs.
// report-and-render), not *whether* delivery happens at all, so it
// isn't the lever for this problem.
//
// Same dependency-isolation note as every round: this module only
// calls windows-capture's own public API, never windows::Win32::*
// directly, so there remains no boundary where our own
// windows = "0.58" and windows-capture's internal windows-rs version
// could conflict.

use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

const REQUESTED_CAPTURE_SECS: u64 = 5;
const FIRST_FRAME_TIMEOUT_SECS: u64 = 10; // generous margin over observed real init times (~1.5s), fails cleanly rather than hanging forever
const TAIL_GAP_GRACE_THRESHOLD_MS: u64 = 200; // only wait the grace period if the tail gap is already meaningfully large
const TAIL_GAP_GRACE_PERIOD_SECS: u64 = 1; // bounded extra wait for one more real frame - never fabricates content, see the mitigation comment at its call site
const TARGET_UPDATE_INTERVAL_MS: u64 = 33; // ~30fps ceiling on real-change reporting, not a forced/guaranteed rate - see FRAME DELIVERY RATE note above
const OUTPUT_FILE_NAME: &str = "native-capture-test.mp4";
const AUDIO_FILE_NAME: &str = "native-capture-test-audio.wav";
const FINAL_MUX_FILE_NAME: &str = "native-capture-test-final.mp4";

static FRAME_COUNT: AtomicU32 = AtomicU32::new(0);
static FRAMES_SUBMITTED: AtomicU32 = AtomicU32::new(0);
static FIRST_FRAME_SIZE: Mutex<Option<(u32, u32)>> = Mutex::new(None);
static CAPTURE_ERROR: Mutex<Option<String>> = Mutex::new(None);
static FIRST_FRAME_AT: Mutex<Option<Instant>> = Mutex::new(None);
static LAST_REAL_FRAME_AT: Mutex<Option<Instant>> = Mutex::new(None);
static DUPLICATED_FRAMES_SUBMITTED: AtomicU32 = AtomicU32::new(0);
static ENCODER_FINISH_DURATION: Mutex<Option<Duration>> = Mutex::new(None);
static AUDIO_BUFFERS_CAPTURED: AtomicU32 = AtomicU32::new(0);
static AUDIO_FRAMES_CAPTURED: AtomicU32 = AtomicU32::new(0);
static AUDIO_FIRST_CHUNK_ELAPSED: Mutex<Option<Duration>> = Mutex::new(None);
static AUDIO_LAST_CHUNK_ELAPSED: Mutex<Option<Duration>> = Mutex::new(None);

#[derive(Serialize)]
pub struct NativeCaptureProof {
    #[serde(rename = "framesReceived")]
    frames_received: u32,
    #[serde(rename = "framesSubmittedToEncoder")]
    frames_submitted_to_encoder: u32,
    #[serde(rename = "duplicatedFramesSubmitted")]
    duplicated_frames_submitted: u32,
    #[serde(rename = "lastRealFrameSeconds")]
    last_real_frame_seconds: Option<f64>,
    #[serde(rename = "tailGapSeconds")]
    tail_gap_seconds: Option<f64>,
    #[serde(rename = "frameWidth")]
    frame_width: Option<u32>,
    #[serde(rename = "frameHeight")]
    frame_height: Option<u32>,
    #[serde(rename = "requestedCaptureSeconds")]
    requested_capture_seconds: u64,
    #[serde(rename = "initializationSeconds")]
    initialization_seconds: Option<f64>,
    #[serde(rename = "captureDurationSeconds")]
    capture_duration_seconds: f64,
    #[serde(rename = "encoderFinalizationSeconds")]
    encoder_finalization_seconds: Option<f64>,
    #[serde(rename = "totalCommandSeconds")]
    total_command_seconds: f64,
    #[serde(rename = "approximateFps")]
    approximate_fps: Option<f64>,
    #[serde(rename = "endedNormally")]
    ended_normally: bool,
    #[serde(rename = "videoPath")]
    video_path: Option<String>,
    #[serde(rename = "captureError")]
    capture_error: Option<String>,
    #[serde(flatten)]
    audio: crate::native_audio::AudioCaptureDiagnostics,
    #[serde(rename = "audioWavPath")]
    audio_wav_path: Option<String>,
    #[serde(rename = "preRollDiscardedSeconds")]
    pre_roll_discarded_seconds: Option<f64>,
    #[serde(rename = "postRollDiscardedSeconds")]
    post_roll_discarded_seconds: Option<f64>,
    #[serde(rename = "retainedAudioFrames")]
    retained_audio_frames: u64,
    #[serde(rename = "expectedWavDurationSeconds")]
    expected_wav_duration_seconds: Option<f64>,
    #[serde(flatten)]
    mux: Option<crate::native_mux::MuxResult>,
}

#[derive(Clone)]
struct CaptureFlags {
    output_path: PathBuf,
    first_frame_tx: Sender<Instant>,
}

struct ProofHandler {
    output_path: PathBuf,
    encoder: Option<VideoEncoder>,
    last_frame_at: Option<Instant>,
    first_frame_tx: Option<Sender<Instant>>,
}

impl GraphicsCaptureApiHandler for ProofHandler {
    type Flags = CaptureFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
        FRAME_COUNT.store(0, Ordering::SeqCst);
        FRAMES_SUBMITTED.store(0, Ordering::SeqCst);
        *FIRST_FRAME_SIZE.lock().unwrap() = None;
        *CAPTURE_ERROR.lock().unwrap() = None;
        *FIRST_FRAME_AT.lock().unwrap() = None;
        *LAST_REAL_FRAME_AT.lock().unwrap() = None;
        DUPLICATED_FRAMES_SUBMITTED.store(0, Ordering::SeqCst);
        *ENCODER_FINISH_DURATION.lock().unwrap() = None;
        Ok(ProofHandler {
            output_path: context.flags.output_path,
            encoder: None,
            first_frame_tx: Some(context.flags.first_frame_tx),
            last_frame_at: None,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        // No stop-condition check here anymore - stopping is now
        // driven externally (see run_capture_proof), independent of
        // whether this callback fires at all.
        let count = FRAME_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        let width = frame.width();
        let height = frame.height();

        {
            let mut size = FIRST_FRAME_SIZE.lock().unwrap();
            if size.is_none() {
                let now = Instant::now();
                *size = Some((width, height));
                *FIRST_FRAME_AT.lock().unwrap() = Some(now);
                // Signal the controlling thread that the first real
                // frame has arrived - it's waiting on this before
                // starting the actual requested-duration countdown
                // (see run_capture_proof). take() ensures this only
                // ever sends once, on the true first frame.
                if let Some(tx) = self.first_frame_tx.take() {
                    let _ = tx.send(now);
                }
            }
        }

        // Created lazily here (not in new()) because the encoder
        // needs real pixel dimensions, and frame.width()/height() are
        // already proven correct on real hardware.
        if self.encoder.is_none() {
            let path_string = self.output_path.to_string_lossy().to_string();
            // ENCODER AUDIO - HYPOTHESIS CLOSED. An earlier round
            // tested whether simply enabling AudioSettingsBuilder
            // (without submitting any PCM) makes the encoder capture
            // system audio internally on its own. That was tested on
            // the real Windows machine this round and did not produce
            // audible audio - the hypothesis is disproven, not left
            // open. Encoder audio stays disabled unconditionally now;
            // there's no reason to keep producing an empty AAC track
            // shell for a path that's confirmed not to work. Real
            // captured system audio is written to a separate WAV file
            // instead - see the AUDIO INTEGRATION comment above
            // run_capture_proof for the full architecture reasoning.
            let audio_settings = AudioSettingsBuilder::default().disabled(true);
            match VideoEncoder::new(
                VideoSettingsBuilder::new(width, height),
                audio_settings,
                ContainerSettingsBuilder::default(),
                path_string.as_str(),
            ) {
                Ok(encoder) => self.encoder = Some(encoder),
                Err(e) => {
                    *CAPTURE_ERROR.lock().unwrap() = Some(format!("Could not create encoder: {e}"));
                }
            }
        }

        // VIDEO DURATION FIX. Root cause confirmed from real test
        // data: a requested 5-second capture with only 2 WGC
        // callbacks produced an MP4 with ~0.033s of media - exactly
        // 2 frames / 60fps. That's not a coincidence: it shows the
        // encoder times frames by a fixed assumed rate (60fps) times
        // sequential frame count, not by real elapsed wall-clock time
        // between callbacks - confirmed indirectly, since accurate
        // per-frame timestamps would have produced ~5s regardless of
        // how few callbacks arrived. windows-capture's send_frame()
        // takes no explicit timestamp parameter, and no confirmed API
        // exists to override this per Frame, so the fix has to work
        // within that constraint rather than against it: this
        // callback now sends the CURRENT frame repeatedly - once for
        // every ~1/60s of real time that elapsed since the previous
        // callback - so the encoder's own fixed-rate timeline
        // naturally accumulates to match real elapsed time instead of
        // only advancing once per (rare) callback. This is the
        // "generating appropriately timestamped duplicate frames"
        // mechanism, implemented the only way available: repeated
        // send_frame() calls on the same still-valid Frame reference,
        // all within this one callback (a Frame is not valid to reuse
        // once this callback returns, so catch-up can only happen
        // here, not from a separate timer). Capped at a reasonable
        // maximum so a very long gap (e.g. after an unusually static
        // stretch) can't produce an excessive burst of encoder calls
        // in one callback.
        //
        // FINAL-TAIL GAP - INVESTIGATED, CONFIRMED UNRESOLVABLE WITH
        // THE CURRENT API. This catch-up mechanism only runs inside
        // on_frame_arrived(), so it can only fill gaps BETWEEN real
        // callbacks - the gap between the LAST real callback and the
        // moment control.stop() is externally called is not
        // represented in the MP4. Investigated whether a copy of the
        // last frame's data could be resubmitted from outside this
        // callback (e.g. right before stop()) to close that gap:
        // frame.buffer() returns a FrameBuffer whose only public use
        // is save_as_image() - there is no way to extract raw pixel
        // data and construct a new Frame from it later, and
        // send_frame() only accepts the crate's own Frame type, which
        // has no public constructor. So there is no confirmed way to
        // "replay" a frame outside its own callback. This gap remains
        // unresolved, not silently accepted - it was not solved by
        // assuming active-screen tests (which happen to get a real
        // callback close to the stop moment) prove it doesn't matter.
        const ASSUMED_ENCODER_FPS: f64 = 60.0;
        const MAX_CATCHUP_FRAMES: u32 = 600; // 10s worth at 60fps - a sane ceiling, not a hard requirement

        let catchup_sends = match self.last_frame_at {
            Some(previous) => {
                let gap_secs = previous.elapsed().as_secs_f64();
                ((gap_secs * ASSUMED_ENCODER_FPS).round() as u32).clamp(1, MAX_CATCHUP_FRAMES)
            }
            None => 1, // first frame - just send it once
        };
        self.last_frame_at = Some(Instant::now());
        *LAST_REAL_FRAME_AT.lock().unwrap() = self.last_frame_at;

        if let Some(encoder) = self.encoder.as_mut() {
            let mut send_error: Option<String> = None;
            for i in 0..catchup_sends {
                match encoder.send_frame(frame) {
                    Ok(()) => {
                        FRAMES_SUBMITTED.fetch_add(1, Ordering::SeqCst);
                        if i > 0 {
                            DUPLICATED_FRAMES_SUBMITTED.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    Err(e) => {
                        send_error = Some(format!("Could not send frame {count} to encoder: {e}"));
                        break;
                    }
                }
            }
            if let Some(e) = send_error {
                *CAPTURE_ERROR.lock().unwrap() = Some(e);
                self.encoder = None; // stop trying to encode further frames after a failure
            }
        }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        // The session has actually ended (triggered externally by
        // run_capture_proof calling control.stop()) - finalize the
        // encoder here, now that no more frames will arrive.
        if let Some(encoder) = self.encoder.take() {
            let finish_start = Instant::now();
            if let Err(e) = encoder.finish() {
                *CAPTURE_ERROR.lock().unwrap() = Some(format!("Could not finish encoding: {e}"));
            }
            *ENCODER_FINISH_DURATION.lock().unwrap() = Some(finish_start.elapsed());
        }
        Ok(())
    }
}

// AUDIO INTEGRATION - architecture decision this round.
//
// Both halves are now independently proven on real hardware: native
// video (a real ~5.00s capture window, frames encoded to HEVC/MP4)
// and native WASAPI system audio (539 buffers, 189600 frames, 48kHz
// stereo, captured from the real default render device). The
// remaining question was how to combine them into one file.
//
// Investigated first, per instruction, rather than guessed:
// windows-capture's VideoEncoder was searched extensively across
// several rounds - its complete official README (every example it
// ships, including advanced DXGI Desktop Duplication and stream-based
// encoding use cases), its public error enum, and community
// discussion - and no confirmed, documented public method for
// supplying external PCM audio samples was ever found. The one
// plausible alternative hypothesis (that simply enabling
// AudioSettingsBuilder makes the encoder capture system audio
// internally, with no caller involvement at all) has now been tested
// on the real machine and did not produce audible audio - that
// hypothesis is closed.
//
// Given that, and given explicit instruction not to introduce a large
// multimedia framework casually (FFmpeg would mean shipping an
// external executable/runtime - a real, load-bearing consequence that
// hasn't been decided on, so it isn't introduced here), the smallest
// honest architecture this round is: keep the two proven capture
// paths as they are, and write the WASAPI PCM to its own real WAV
// file (write_wav_file() below - plain std, no new dependency) using
// the actual captured mix format, alongside the existing MP4. This is
// not the one-file muxed result the directive prefers, and that gap
// is reported honestly rather than papered over - but it's a real,
// inspectable, playable second file with genuine captured audio in
// it, not a faked integration. Muxing both into one container would
// need either a confirmed encoder audio-input API (still not found)
// or a real container-muxing component (a meaningfully larger
// undertaking, appropriately out of scope for a single pass per the
// "smallest reliable architecture" instruction) - both remain open
// for a dedicated future pass, not attempted here.
//
// CAPTURE-ORIGIN ALIGNMENT (this round). A real test found the WAV
// was capturing genuine audio but over the wrong window - it started
// before video was ready and kept running after video's whole
// lifecycle ended, producing ~1.5s more audio than the video's actual
// content, closely matching video's own reported initialization time.
// See the detailed CAPTURE-ORIGIN ALIGNMENT comment further below
// (right above where chunks are trimmed) for exactly how this is
// fixed: audio itself still starts early and stops late deliberately
// (so it never misses real content), but only the portion between
// video's real first frame and that plus the requested duration gets
// saved to the WAV - trimmed at the sample level, not by discarding
// whole chunks or an arbitrary fixed constant.
//
// WAV format note: the format tag is written as IEEE float (3) when
// bits_per_sample is 32, otherwise as integer PCM (1). WASAPI shared-
// mode mix formats on modern Windows are almost always 32-bit float -
// a well-established platform norm, not a guess specific to this
// project - so this covers the common case directly; other bit depths
// fall back to the PCM tag, which may not be byte-for-byte correct for
// every possible device format, but keeps the file structurally valid
// either way.
fn write_wav_file(path: &std::path::Path, pcm: &[u8], sample_rate: u32, channels: u16, bits_per_sample: u16) -> Result<(), String> {
    let format_tag: u16 = if bits_per_sample == 32 { 3 } else { 1 }; // 3 = IEEE float, 1 = integer PCM
    let block_align = channels * (bits_per_sample / 8);
    let byte_rate = sample_rate * block_align as u32;
    let data_len = pcm.len() as u32;
    let riff_len = 36 + data_len;

    let mut file = std::fs::File::create(path).map_err(|e| format!("Could not create WAV file: {e}"))?;
    use std::io::Write;

    file.write_all(b"RIFF").map_err(|e| e.to_string())?;
    file.write_all(&riff_len.to_le_bytes()).map_err(|e| e.to_string())?;
    file.write_all(b"WAVE").map_err(|e| e.to_string())?;
    file.write_all(b"fmt ").map_err(|e| e.to_string())?;
    file.write_all(&16u32.to_le_bytes()).map_err(|e| e.to_string())?; // fmt chunk size
    file.write_all(&format_tag.to_le_bytes()).map_err(|e| e.to_string())?;
    file.write_all(&channels.to_le_bytes()).map_err(|e| e.to_string())?;
    file.write_all(&sample_rate.to_le_bytes()).map_err(|e| e.to_string())?;
    file.write_all(&byte_rate.to_le_bytes()).map_err(|e| e.to_string())?;
    file.write_all(&block_align.to_le_bytes()).map_err(|e| e.to_string())?;
    file.write_all(&bits_per_sample.to_le_bytes()).map_err(|e| e.to_string())?;
    file.write_all(b"data").map_err(|e| e.to_string())?;
    file.write_all(&data_len.to_le_bytes()).map_err(|e| e.to_string())?;
    file.write_all(pcm).map_err(|e| e.to_string())?;

    Ok(())
}

fn run_capture_proof(app: &AppHandle, include_system_audio: bool) -> Result<NativeCaptureProof, String> {
    let command_start = Instant::now();
    crate::debug_log::log(app, "native_capture: sustained proof starting, acquiring primary monitor");

    let output_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Could not resolve config directory: {e}"))?;
    std::fs::create_dir_all(&output_dir).map_err(|e| format!("Could not create output directory: {e}"))?;
    let output_path = output_dir.join(OUTPUT_FILE_NAME);
    // Remove any previous run's file so a failed encode this time
    // can't be mistaken for a successful leftover from before.
    let _ = std::fs::remove_file(&output_path);

    // Start WASAPI loopback first, if requested, so it's already
    // running by the time video capture begins. Its captured PCM is
    // now accumulated (not just counted) and written to a separate
    // WAV file after capture ends - see the AUDIO INTEGRATION comment
    // below for why this is a second file rather than one muxed MP4
    // this round. A failure here never aborts the video proof -
    // recorded as audio diagnostics, the video-only path continues
    // exactly as before.
    AUDIO_BUFFERS_CAPTURED.store(0, Ordering::SeqCst);
    AUDIO_FRAMES_CAPTURED.store(0, Ordering::SeqCst);
    *AUDIO_FIRST_CHUNK_ELAPSED.lock().unwrap() = None;
    *AUDIO_LAST_CHUNK_ELAPSED.lock().unwrap() = None;

    let mut audio_diagnostics = crate::native_audio::AudioCaptureDiagnostics {
        audio_requested: include_system_audio,
        ..Default::default()
    };
    let mut audio_stop_flag: Option<Arc<AtomicBool>> = None;
    let mut audio_join_handle: Option<std::thread::JoinHandle<Vec<crate::native_audio::AudioChunk>>> = None;
    let mut audio_capture_start: Option<Instant> = None;

    if include_system_audio {
        match crate::native_audio::start_loopback_capture() {
            Ok((receiver, stop_flag, diagnostics, capture_start)) => {
                crate::debug_log::log(
                    app,
                    &format!(
                        "native_capture: WASAPI loopback started, device={:?}, rate={:?}, channels={:?}",
                        diagnostics.render_endpoint_name, diagnostics.mix_sample_rate, diagnostics.mix_channels
                    ),
                );
                audio_diagnostics = diagnostics;
                audio_stop_flag = Some(stop_flag.clone());
                audio_capture_start = Some(capture_start);
                // Accumulates every captured chunk AS ITS OWN OBJECT
                // (not flattened into one byte buffer) - capture-
                // origin alignment below needs each chunk's own
                // elapsed timestamp to decide whether it's pre-roll,
                // retained, or post-roll, and to trim a chunk that
                // straddles a boundary. Never touches the video
                // encoder (see the AUDIO INTEGRATION comment below
                // for why).
                audio_join_handle = Some(std::thread::spawn(move || {
                    let mut chunks: Vec<crate::native_audio::AudioChunk> = Vec::new();
                    while !stop_flag.load(Ordering::SeqCst) {
                        while let Ok(chunk) = receiver.try_recv() {
                            AUDIO_BUFFERS_CAPTURED.fetch_add(1, Ordering::SeqCst);
                            AUDIO_FRAMES_CAPTURED.fetch_add(chunk.frames, Ordering::SeqCst);
                            {
                                let mut first = AUDIO_FIRST_CHUNK_ELAPSED.lock().unwrap();
                                if first.is_none() {
                                    *first = Some(chunk.elapsed);
                                }
                            }
                            *AUDIO_LAST_CHUNK_ELAPSED.lock().unwrap() = Some(chunk.elapsed);
                            chunks.push(chunk);
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    // Drain whatever arrived in the brief window
                    // between the last poll and the thread actually
                    // observing stop_flag, so nothing captured is lost.
                    while let Ok(chunk) = receiver.try_recv() {
                        AUDIO_BUFFERS_CAPTURED.fetch_add(1, Ordering::SeqCst);
                        AUDIO_FRAMES_CAPTURED.fetch_add(chunk.frames, Ordering::SeqCst);
                        {
                            let mut first = AUDIO_FIRST_CHUNK_ELAPSED.lock().unwrap();
                            if first.is_none() {
                                *first = Some(chunk.elapsed);
                            }
                        }
                        *AUDIO_LAST_CHUNK_ELAPSED.lock().unwrap() = Some(chunk.elapsed);
                        chunks.push(chunk);
                    }
                    chunks
                }));
            }
            Err(e) => {
                crate::debug_log::log(app, &format!("native_capture: WASAPI loopback FAILED to start: {e}"));
                audio_diagnostics.audio_error = Some(e);
            }
        }
    }

    let primary_monitor = Monitor::primary().map_err(|e| format!("No primary monitor available: {e}"))?;

    let (first_frame_tx, first_frame_rx) = mpsc::channel::<Instant>();

    let settings = Settings::new(
        primary_monitor,
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Custom(Duration::from_millis(TARGET_UPDATE_INTERVAL_MS)),
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        CaptureFlags {
            output_path: output_path.clone(),
            first_frame_tx,
        },
    );

    crate::debug_log::log(
        app,
        &format!(
            "native_capture: calling Capture::start_free_threaded, requested {REQUESTED_CAPTURE_SECS}s of capture, target interval {TARGET_UPDATE_INTERVAL_MS}ms, output {}",
            output_path.display()
        ),
    );

    let capture_call_at = Instant::now();
    let mut ended_normally = false;
    let mut stop_requested_at: Option<Instant> = None;

    match ProofHandler::start_free_threaded(settings) {
        Ok(control) => {
            // Wait for the first real frame before starting the
            // requested-duration countdown - this is the actual fix.
            // Previously the countdown started right after
            // start_free_threaded() returned, which is BEFORE WGC
            // initialization and the first frame - meaning the
            // requested 5 seconds already included video's own
            // startup time, so the real content-capturing window
            // after the first frame was shorter than 5 seconds even
            // though audio's trimming logic assumed a full 5 seconds
            // from that same first-frame point. Waiting here first
            // makes both windows describe the same real interval.
            // recv_timeout (not recv) so a first frame that never
            // arrives fails cleanly instead of hanging forever - a
            // real possibility if WGC initialization itself fails
            // silently or the desktop can't be captured at all.
            match first_frame_rx.recv_timeout(Duration::from_secs(FIRST_FRAME_TIMEOUT_SECS)) {
                Ok(_first_frame_at) => {
                    std::thread::sleep(Duration::from_secs(REQUESTED_CAPTURE_SECS));

                    // TAIL-GAP MITIGATION. The catch-up mechanism in
                    // on_frame_arrived() can only fill gaps BETWEEN
                    // real callbacks - the interval between the LAST
                    // real callback and this stop point was, until
                    // now, never represented in the encoded media at
                    // all (confirmed: 239 submitted frames / 60fps ==
                    // 3.983s exactly, in a real test where the
                    // requested window was 5s - the entire ~1s
                    // shortfall was this unfilled tail, not a
                    // computation error in the catch-up math itself).
                    // This does not fabricate frames or guess a
                    // duplicate count - it waits, briefly and
                    // bounded, for one more GENUINE WGC callback,
                    // which (if it arrives) triggers the existing,
                    // legitimate catch-up mechanism to close most of
                    // the gap using real elapsed time. If the desktop
                    // stays completely static through this grace
                    // window too, no frame arrives, nothing is
                    // fabricated, and the remaining gap is reported
                    // honestly via the new tailGapSeconds diagnostic
                    // rather than hidden.
                    let last_before_grace = *LAST_REAL_FRAME_AT.lock().unwrap();
                    let gap_already_ms = last_before_grace.map(|last| last.elapsed().as_millis() as u64).unwrap_or(u64::MAX);
                    if gap_already_ms >= TAIL_GAP_GRACE_THRESHOLD_MS {
                        let grace_deadline = Instant::now() + Duration::from_secs(TAIL_GAP_GRACE_PERIOD_SECS);
                        while Instant::now() < grace_deadline {
                            std::thread::sleep(Duration::from_millis(50));
                            let latest = *LAST_REAL_FRAME_AT.lock().unwrap();
                            if latest != last_before_grace {
                                break; // a new real frame arrived and triggered catch-up
                            }
                        }
                    }

                    stop_requested_at = Some(Instant::now());

                    match control.stop() {
                        Ok(()) => {
                            // stop(self) consumes control and already
                            // requests shutdown and joins the capture
                            // thread - there is nothing left to
                            // wait() on afterward, and control itself
                            // is gone by this point. A successful
                            // return here is itself the confirmation
                            // that the capture thread (and its
                            // on_closed cleanup, i.e. encoder
                            // finalization) has finished.
                            ended_normally = true;
                        }
                        Err(e) => {
                            let mut error = CAPTURE_ERROR.lock().unwrap();
                            if error.is_none() {
                                *error = Some(format!("CaptureControl::stop returned an error: {e}"));
                            }
                        }
                    }
                }
                Err(_) => {
                    // No first frame within the timeout - fail
                    // cleanly rather than starting a countdown from
                    // an undefined origin. Still stop the capture
                    // thread so nothing is left running.
                    let _ = control.stop();
                    let mut error = CAPTURE_ERROR.lock().unwrap();
                    if error.is_none() {
                        *error = Some(format!(
                            "No video frame arrived within {FIRST_FRAME_TIMEOUT_SECS} seconds of starting capture - initialization may have failed."
                        ));
                    }
                }
            }
        }
        Err(e) => {
            let mut error = CAPTURE_ERROR.lock().unwrap();
            if error.is_none() {
                *error = Some(format!("start_free_threaded returned an error: {e}"));
            }
        }
    }

    // Stop the WASAPI capture thread now that video capture has
    // ended, however it ended - never leave it running.
    if let Some(stop_flag) = &audio_stop_flag {
        stop_flag.store(true, Ordering::SeqCst);
    }

    // Wait for the audio thread to actually finish and hand back
    // every captured chunk, then align it to video's actual capture
    // window before writing the WAV. See the AUDIO INTEGRATION
    // comment below for why this is a separate file rather than
    // muxed into the MP4 this round.
    //
    // CAPTURE-ORIGIN ALIGNMENT. Audio starts capturing before video's
    // WGC session is even created (so there's no startup gap once
    // video is ready), and stops only after video's entire lifecycle
    // ends - both deliberate, to make sure audio never misses real
    // content. That means the raw audio span is always longer than
    // the requested duration, by roughly video's own initialization
    // time (confirmed by a real test: ~1.5s pre-roll, matching video's
    // reported initialization_seconds almost exactly). The fix is not
    // to change when audio starts or stops, but to trim what gets
    // SAVED: capture_origin is defined as FIRST_FRAME_AT - the real
    // Instant video's first actual frame arrived, i.e. "when video
    // capture was actually ready" - and only audio between
    // capture_origin and capture_origin + REQUESTED_CAPTURE_SECS is
    // retained. Each chunk's own elapsed-since-audio-capture-start
    // timestamp (converted to an absolute Instant via
    // audio_capture_start) is compared against that window - chunks
    // entirely outside it are dropped, and a chunk straddling either
    // boundary is trimmed at the sample level using the real mix
    // format (sample rate, channels, bytes per sample), not discarded
    // whole, so no more real audio is lost than necessary.
    let mut audio_wav_path: Option<String> = None;
    let mut pre_roll_discarded_seconds: Option<f64> = None;
    let mut post_roll_discarded_seconds: Option<f64> = None;
    let mut retained_audio_frames: u64 = 0;
    let mut expected_wav_duration_seconds: Option<f64> = None;

    if let Some(handle) = audio_join_handle {
        match handle.join() {
            Ok(chunks) if !chunks.is_empty() => {
                let capture_origin = *FIRST_FRAME_AT.lock().unwrap();
                let sample_rate = audio_diagnostics.mix_sample_rate.unwrap_or(48_000);
                let channels = audio_diagnostics.mix_channels.unwrap_or(2);
                let bits_per_sample = audio_diagnostics.mix_bits_per_sample.unwrap_or(32);
                let block_align = (channels as usize) * (bits_per_sample as usize / 8);
                // The real measured stop point, not the nominal
                // requested constant - keeps audio's trim boundary
                // exactly consistent with where video actually
                // stopped, including any small scheduling jitter.
                let window_end = match (capture_origin, stop_requested_at) {
                    (Some(origin), Some(stop)) => stop.duration_since(origin).as_secs_f64(),
                    _ => REQUESTED_CAPTURE_SECS as f64,
                };

                let mut retained_pcm: Vec<u8> = Vec::new();
                let mut pre_roll_secs = 0.0f64;
                let mut post_roll_secs = 0.0f64;

                match (capture_origin, audio_capture_start) {
                    (Some(origin), Some(audio_start)) if block_align > 0 => {
                        for chunk in &chunks {
                            let chunk_frames = chunk.frames as f64;
                            let chunk_duration = chunk_frames / sample_rate as f64;
                            // chunk.elapsed marks when the packet was
                            // read, i.e. the END of the audio it
                            // contains - the chunk's content spans
                            // backwards from there.
                            let chunk_end_abs = audio_start + chunk.elapsed;
                            let chunk_start_abs = chunk_end_abs
                                .checked_sub(Duration::from_secs_f64(chunk_duration))
                                .unwrap_or(chunk_end_abs);

                            // Express both edges as offsets (seconds,
                            // possibly negative) from capture_origin.
                            let start_offset = if chunk_start_abs >= origin {
                                chunk_start_abs.duration_since(origin).as_secs_f64()
                            } else {
                                -origin.duration_since(chunk_start_abs).as_secs_f64()
                            };
                            let end_offset = if chunk_end_abs >= origin {
                                chunk_end_abs.duration_since(origin).as_secs_f64()
                            } else {
                                -origin.duration_since(chunk_end_abs).as_secs_f64()
                            };

                            if end_offset <= 0.0 || start_offset >= window_end {
                                // Entirely outside the retained window.
                                if end_offset <= 0.0 {
                                    pre_roll_secs += chunk_duration;
                                } else {
                                    post_roll_secs += chunk_duration;
                                }
                                continue;
                            }

                            // Trim frames before capture_origin
                            // (pre-roll) and after the window end
                            // (post-roll), sample-accurately.
                            let trim_start_secs = (0.0 - start_offset).max(0.0);
                            let trim_end_secs = (end_offset - window_end).max(0.0);
                            pre_roll_secs += trim_start_secs;
                            post_roll_secs += trim_end_secs;

                            let trim_start_frames = ((trim_start_secs * sample_rate as f64).round() as usize).min(chunk.frames as usize);
                            let trim_end_frames = ((trim_end_secs * sample_rate as f64).round() as usize).min(chunk.frames as usize - trim_start_frames.min(chunk.frames as usize));

                            let start_byte = trim_start_frames * block_align;
                            let end_byte = chunk.pcm.len().saturating_sub(trim_end_frames * block_align);

                            if start_byte < end_byte && end_byte <= chunk.pcm.len() {
                                retained_pcm.extend_from_slice(&chunk.pcm[start_byte..end_byte]);
                                retained_audio_frames += ((end_byte - start_byte) / block_align.max(1)) as u64;
                            }
                        }
                        pre_roll_discarded_seconds = Some(pre_roll_secs);
                        post_roll_discarded_seconds = Some(post_roll_secs);
                    }
                    _ => {
                        // No valid capture_origin (video never
                        // produced a frame) - can't align, so nothing
                        // is retained rather than guessing. Reported
                        // as an audio error so it's visible, not
                        // silently empty.
                        if audio_diagnostics.audio_error.is_none() {
                            audio_diagnostics.audio_error =
                                Some("Could not align audio to video's capture window - video never reported a first frame.".to_string());
                        }
                    }
                }

                if !retained_pcm.is_empty() {
                    expected_wav_duration_seconds = Some(retained_audio_frames as f64 / sample_rate as f64);
                    let wav_path = output_dir.join(AUDIO_FILE_NAME);
                    match write_wav_file(&wav_path, &retained_pcm, sample_rate, channels, bits_per_sample) {
                        Ok(()) => {
                            crate::debug_log::log(
                                app,
                                &format!(
                                    "native_capture: WAV written, {} retained frames ({:.2}s), pre_roll_discarded={:.2}s, post_roll_discarded={:.2}s, {}",
                                    retained_audio_frames, expected_wav_duration_seconds.unwrap_or(0.0), pre_roll_secs, post_roll_secs, wav_path.display()
                                ),
                            );
                            audio_wav_path = Some(wav_path.display().to_string());
                        }
                        Err(e) => {
                            crate::debug_log::log(app, &format!("native_capture: WAV write FAILED: {e}"));
                            if audio_diagnostics.audio_error.is_none() {
                                audio_diagnostics.audio_error = Some(format!("Could not write WAV file: {e}"));
                            }
                        }
                    }
                }
            }
            Ok(_) => {
                // Empty accumulation - audio was requested and WASAPI
                // initialized, but nothing was actually captured
                // (e.g. the render device was silent the whole time).
            }
            Err(_) => {
                if audio_diagnostics.audio_error.is_none() {
                    audio_diagnostics.audio_error = Some("Audio capture thread panicked.".to_string());
                }
            }
        }
    }

    audio_diagnostics.buffers_captured = AUDIO_BUFFERS_CAPTURED.load(Ordering::SeqCst);
    audio_diagnostics.frames_captured = AUDIO_FRAMES_CAPTURED.load(Ordering::SeqCst) as u64;
    // The real span between the first and last captured audio chunk -
    // this is what actually answers "how much of the requested window
    // did audio capture cover," distinct from wall-clock diagnostics
    // elsewhere that only bound when capture started/stopped being
    // requested, not when real packets were actually flowing.
    audio_diagnostics.captured_span_seconds = match (
        *AUDIO_FIRST_CHUNK_ELAPSED.lock().unwrap(),
        *AUDIO_LAST_CHUNK_ELAPSED.lock().unwrap(),
    ) {
        (Some(first), Some(last)) => Some(last.saturating_sub(first).as_secs_f64()),
        _ => None,
    };

    let total_command_seconds = command_start.elapsed().as_secs_f64();

    let frames_received = FRAME_COUNT.load(Ordering::SeqCst);
    let frames_submitted_to_encoder = FRAMES_SUBMITTED.load(Ordering::SeqCst);
    let (frame_width, frame_height) = FIRST_FRAME_SIZE
        .lock()
        .unwrap()
        .map(|(w, h)| (Some(w), Some(h)))
        .unwrap_or((None, None));
    let capture_error = CAPTURE_ERROR.lock().unwrap().clone();
    let first_frame_at = *FIRST_FRAME_AT.lock().unwrap();

    let initialization_seconds = first_frame_at.map(|first| first.duration_since(capture_call_at).as_secs_f64());
    // Measured, not assumed: the real elapsed time from the first
    // frame to the moment stop was requested. Now that the countdown
    // starts after waiting for the first frame (see the wait on
    // first_frame_rx above), this should read close to
    // REQUESTED_CAPTURE_SECS - but it's computed from real timestamps
    // rather than hardcoded, so any remaining discrepancy (e.g. from
    // scheduling jitter) is visible rather than papered over.
    let capture_duration_seconds = match (first_frame_at, stop_requested_at) {
        (Some(first), Some(stop)) => stop.duration_since(first).as_secs_f64(),
        _ => 0.0,
    };
    let encoder_finalization_seconds = ENCODER_FINISH_DURATION.lock().unwrap().map(|d| d.as_secs_f64());
    let duplicated_frames_submitted = DUPLICATED_FRAMES_SUBMITTED.load(Ordering::SeqCst);
    let last_real_frame_at = *LAST_REAL_FRAME_AT.lock().unwrap();
    let last_real_frame_seconds = match (first_frame_at, last_real_frame_at) {
        (Some(first), Some(last)) => Some(last.duration_since(first).as_secs_f64()),
        _ => None,
    };
    // How much of the requested window, after the last real WGC
    // callback, has no corresponding encoded media - the honest
    // measurement of the known, previously-unmitigated limitation.
    // Should now usually be small thanks to the grace-period wait
    // above, but is reported exactly as measured, not assumed zero.
    let tail_gap_seconds = match (last_real_frame_at, stop_requested_at) {
        (Some(last), Some(stop)) => Some(stop.duration_since(last).as_secs_f64()),
        _ => None,
    };

    let approximate_fps = if capture_duration_seconds > 0.0 {
        Some(frames_submitted_to_encoder as f64 / capture_duration_seconds)
    } else {
        None
    };

    let video_path = if capture_error.is_none() && output_path.exists() {
        Some(output_path.display().to_string())
    } else {
        None
    };

    crate::debug_log::log(
        app,
        &format!(
            "native_capture: proof finished, wgc_callbacks={frames_received}, submitted_to_encoder={frames_submitted_to_encoder}, capture_duration={capture_duration_seconds:.2}s, finalization={encoder_finalization_seconds:?}s, total_command={total_command_seconds:.2}s, ended_normally={ended_normally}, capture_error={capture_error:?}, video_path={video_path:?}, audio_requested={}, audio_buffers_captured={}, audio_frames_captured={}, audio_error={:?}",
            audio_diagnostics.audio_requested, audio_diagnostics.buffers_captured, audio_diagnostics.frames_captured, audio_diagnostics.audio_error
        ),
    );

    Ok(NativeCaptureProof {
        frames_received,
        frames_submitted_to_encoder,
        duplicated_frames_submitted,
        last_real_frame_seconds,
        tail_gap_seconds,
        frame_width,
        frame_height,
        requested_capture_seconds: REQUESTED_CAPTURE_SECS,
        initialization_seconds,
        capture_duration_seconds,
        encoder_finalization_seconds,
        total_command_seconds,
        approximate_fps,
        ended_normally,
        video_path,
        capture_error,
        audio: audio_diagnostics,
        audio_wav_path,
        pre_roll_discarded_seconds,
        post_roll_discarded_seconds,
        retained_audio_frames,
        expected_wav_duration_seconds,
        mux: None, // populated by test_native_capture after this function returns - see there
    })
}

/// Diagnostic-only command. Not called anywhere in the working
/// recording flow. Captures the primary monitor for a requested ~5
/// seconds - stopped by an external timer, independent of frame
/// arrival - via Windows Graphics Capture, attempts to encode a real
/// MP4 to this app's own config directory, and reports separately-
/// measured timing plus WGC-callback vs. encoder-submission frame
/// counts. Never opens any browser-style permission dialog. When
/// include_system_audio is true, also attempts WASAPI loopback
/// capture of the default playback device - a failure there is
/// reported in the result, never a hard error for the whole command.
#[tauri::command]
pub async fn test_native_capture(app: AppHandle, include_system_audio: bool) -> Result<NativeCaptureProof, String> {
    let app_for_mux = app.clone();
    let mut proof = tauri::async_runtime::spawn_blocking(move || run_capture_proof(&app, include_system_audio))
        .await
        .map_err(|e| format!("Native capture proof task failed: {e}"))??;

    // Muxing only attempted when both proven source files exist -
    // never a hard failure for the whole command either way, per
    // explicit error-handling instruction: the video/audio capture
    // result is preserved and returned regardless of whether muxing
    // succeeds. See native_mux.rs for the full architecture reasoning
    // and the real, currently-unresolved packaging gap (a real
    // ffmpeg.exe build must be bundled before this can succeed).
    if let (Some(video_path), Some(audio_path)) = (proof.video_path.clone(), proof.audio_wav_path.clone()) {
        match app_for_mux.path().app_config_dir() {
            Ok(output_dir) => {
                let output_path = output_dir.join(FINAL_MUX_FILE_NAME);
                let mux_result = crate::native_mux::mux_video_and_audio(
                    &app_for_mux,
                    std::path::Path::new(&video_path),
                    std::path::Path::new(&audio_path),
                    &output_path,
                )
                .await;
                crate::debug_log::log(
                    &app_for_mux,
                    &format!(
                        "native_capture: mux attempt finished, success={}, exit_code={:?}, path={:?}, error={:?}",
                        mux_result.muxing_success, mux_result.ffmpeg_exit_code, mux_result.final_muxed_path, mux_result.muxing_error
                    ),
                );
                proof.mux = Some(mux_result);
            }
            Err(e) => {
                // Previously this whole block was skipped silently on
                // this failure, leaving proof.mux as None with no
                // success or error message at all - exactly the
                // confusing "nothing happened" outcome a real test
                // showed. Now always reported explicitly.
                crate::debug_log::log(&app_for_mux, &format!("native_capture: mux attempt could not start, could not resolve config dir: {e}"));
                proof.mux = Some(crate::native_mux::MuxResult {
                    mux_attempted: true,
                    sidecar_invocation_succeeded: false,
                    ffmpeg_exit_code: None,
                    final_muxed_path: None,
                    muxing_method: "ffmpeg sidecar".to_string(),
                    video_stream_handling: "copy (no re-encode)".to_string(),
                    audio_codec_used: "aac".to_string(),
                    muxing_success: false,
                    final_file_size_bytes: None,
                    muxing_error: Some(format!("Could not resolve the app config directory for the muxed output path: {e}")),
                });
            }
        }
    } else {
        // Neither source file was available (e.g. video or audio
        // capture itself failed) - explicitly report that mux was
        // never attempted, rather than leaving proof.mux silently
        // None with no indication why.
        proof.mux = Some(crate::native_mux::MuxResult {
            mux_attempted: false,
            sidecar_invocation_succeeded: false,
            ffmpeg_exit_code: None,
            final_muxed_path: None,
            muxing_method: "ffmpeg sidecar".to_string(),
            video_stream_handling: "copy (no re-encode)".to_string(),
            audio_codec_used: "aac".to_string(),
            muxing_success: false,
            final_file_size_bytes: None,
            muxing_error: Some("Mux not attempted - the native video and/or WASAPI audio source file was not produced.".to_string()),
        });
    }

    Ok(proof)
}
