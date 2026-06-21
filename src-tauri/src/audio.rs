use rodio::{Decoder, OutputStream, Sink, Source};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// Windows power-state control so playback can never be cut short by the machine
/// or monitor sleeping. Many setups (this one included) have the speakers wired
/// into the monitor, so when the display sleeps its audio endpoint powers down
/// and there is no device left to play through. We therefore keep the *display*
/// awake for the duration of the athan (`ES_DISPLAY_REQUIRED`) and nudge it awake
/// at the start in case it was already asleep when the athan fired.
#[cfg(windows)]
mod keepawake {
    type ExecutionState = u32;
    const ES_CONTINUOUS: ExecutionState = 0x8000_0000;
    const ES_SYSTEM_REQUIRED: ExecutionState = 0x0000_0001;
    const ES_DISPLAY_REQUIRED: ExecutionState = 0x0000_0002;
    const ES_AWAYMODE_REQUIRED: ExecutionState = 0x0000_0040;

    const VK_F15: u8 = 0x7E; // does nothing in apps; just registers as input
    const KEYEVENTF_KEYUP: u32 = 0x0002;

    extern "system" {
        fn SetThreadExecutionState(es_flags: ExecutionState) -> ExecutionState;
        fn keybd_event(b_vk: u8, b_scan: u8, dw_flags: u32, dw_extra_info: usize);
    }

    /// Wake the display (if it was off) and pin the system + monitor awake until
    /// `end`, so audio routed through the monitor keeps flowing.
    pub fn begin() {
        unsafe {
            // Synthesize a harmless F15 tap to wake an already-sleeping monitor;
            // SetThreadExecutionState only *prevents* sleep, it can't undo it.
            keybd_event(VK_F15, 0, 0, 0);
            keybd_event(VK_F15, 0, KEYEVENTF_KEYUP, 0);
            SetThreadExecutionState(
                ES_CONTINUOUS
                    | ES_SYSTEM_REQUIRED
                    | ES_DISPLAY_REQUIRED
                    | ES_AWAYMODE_REQUIRED,
            );
        }
    }

    /// Release the wake lock; the machine and monitor may sleep normally again.
    pub fn end() {
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}
#[cfg(not(windows))]
mod keepawake {
    pub fn begin() {}
    pub fn end() {}
}

pub enum AudioCmd {
    /// Play the given files in sequence (e.g. athan then dua) at `volume`.
    Play { paths: Vec<PathBuf>, volume: f32 },
    /// Adjust the volume of whatever is currently playing.
    SetVolume(f32),
    /// Stop whatever is playing.
    Stop,
}

/// Map an athan style key to its bundled filename.
pub fn style_file(style: &str) -> &'static str {
    match style {
        "madina" => "athan_madina.mp3",
        "egypt" => "athan_egypt.mp3",
        "alaqsa" => "athan_alaqsa.mp3",
        _ => "athan_makkah.mp3",
    }
}

pub const FAJR_FILE: &str = "athan_fajr.mp3";
pub const DUA_FILE: &str = "dua_after_athan.mp3";

/// Spawn a dedicated audio thread that owns the (non-Send) output stream and
/// plays/stops on command. Returns the command sender.
///
/// `on_ended` is invoked whenever playback stops — either naturally (the queue
/// drained) or via `AudioCmd::Stop` — so the UI can clear its "playing" state.
pub fn spawn(on_ended: impl Fn() + Send + 'static) -> Sender<AudioCmd> {
    let (tx, rx) = channel::<AudioCmd>();
    std::thread::spawn(move || {
        // Cache of each clip's exact playback length, computed once by decoding the
        // whole file. Used to tell a *premature* stop (device died) apart from the
        // queue finishing naturally.
        let mut durations: HashMap<PathBuf, Duration> = HashMap::new();
        // The live output stream + sink, plus the logical play it represents so we
        // can rebuild and resume it if the audio device disappears mid-athan.
        let mut current: Option<(OutputStream, Sink)> = None;
        let mut playing: Option<Playing> = None;
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(AudioCmd::SetVolume(v)) => {
                    if let Some(p) = playing.as_mut() {
                        p.volume = v.clamp(0.0, 1.0);
                    }
                    if let Some((_, s)) = current.as_ref() {
                        s.set_volume(v.clamp(0.0, 1.0));
                    }
                }
                Ok(AudioCmd::Stop) => {
                    let was_playing = current.is_some() || playing.is_some();
                    if let Some((_, s)) = current.take() {
                        s.stop();
                    }
                    playing = None;
                    if was_playing {
                        keepawake::end();
                        on_ended();
                    }
                }
                Ok(AudioCmd::Play { paths, volume }) => {
                    if let Some((_, s)) = current.take() {
                        s.stop();
                    }
                    let volume = volume.clamp(0.0, 1.0);
                    let total: Duration = paths
                        .iter()
                        .map(|p| duration_of(p, &mut durations))
                        .sum();
                    let play = Playing {
                        paths,
                        volume,
                        started: Instant::now(),
                        total,
                    };
                    keepawake::begin();
                    match build_sink(&play, Duration::ZERO, &mut durations) {
                        Some(sc) => {
                            current = Some(sc);
                            playing = Some(play);
                        }
                        None => {
                            // No device right now — keep the logical play alive and
                            // let the recovery tick retry until a device appears.
                            playing = Some(play);
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    let Some(play) = playing.as_ref() else { continue };
                    let elapsed = play.started.elapsed();
                    let drained = current.as_ref().is_none_or(|(_, s)| s.empty());
                    if !drained {
                        continue;
                    }
                    // The sink is empty (or never built). If we've reached the
                    // expected length, the athan finished cleanly. Otherwise the
                    // device died under us — rebuild on whatever device exists now
                    // and resume from where we were. Retries every tick until the
                    // device returns or the athan would be over.
                    if elapsed + RESUME_EPSILON >= play.total {
                        current = None;
                        playing = None;
                        keepawake::end();
                        on_ended();
                        continue;
                    }
                    current = None;
                    let play = playing.clone().unwrap();
                    match build_sink(&play, elapsed, &mut durations) {
                        Some(sc) => {
                            eprintln!(
                                "audio: device recovered, resuming athan at {:.0}s",
                                elapsed.as_secs_f32()
                            );
                            current = Some(sc);
                        }
                        None => { /* still no device; retry next tick */ }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        keepawake::end();
    });
    tx
}

/// Small grace window so reaching the very end of the queue isn't misread as a
/// premature device failure.
const RESUME_EPSILON: Duration = Duration::from_millis(750);

/// A logical playback request, retained so it can be rebuilt/resumed on a fresh
/// device if the current one disappears mid-athan.
#[derive(Clone)]
struct Playing {
    paths: Vec<PathBuf>,
    volume: f32,
    started: Instant,
    total: Duration,
}

/// Open a fresh default output stream and queue `play`'s files, skipping the
/// first `skip` of audio (used to resume after a device loss). Returns `None`
/// if no audio device is currently available.
fn build_sink(
    play: &Playing,
    skip: Duration,
    durations: &mut HashMap<PathBuf, Duration>,
) -> Option<(OutputStream, Sink)> {
    let (stream, handle) = match OutputStream::try_default() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("audio: no output device available: {e}");
            return None;
        }
    };
    let sink = match Sink::try_new(&handle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("audio: create sink: {e}");
            return None;
        }
    };
    sink.set_volume(play.volume);
    let mut remaining = skip;
    for p in &play.paths {
        let dur = duration_of(p, durations);
        if remaining >= dur {
            // This whole clip already played before the interruption.
            remaining -= dur;
            continue;
        }
        let f = match File::open(p) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("audio: open {p:?}: {e}");
                continue;
            }
        };
        match Decoder::new(BufReader::new(f)) {
            Ok(src) => {
                if remaining > Duration::ZERO {
                    sink.append(src.skip_duration(remaining));
                    remaining = Duration::ZERO;
                } else {
                    sink.append(src);
                }
            }
            Err(e) => eprintln!("audio: decode {p:?}: {e}"),
        }
    }
    Some((stream, sink))
}

/// Exact playback length of a clip, decoded once and cached. Falls back to the
/// container's reported duration, then to zero, if full decoding fails.
fn duration_of(path: &PathBuf, cache: &mut HashMap<PathBuf, Duration>) -> Duration {
    if let Some(d) = cache.get(path) {
        return *d;
    }
    let d = compute_duration(path).unwrap_or(Duration::ZERO);
    cache.insert(path.clone(), d);
    d
}

fn compute_duration(path: &PathBuf) -> Option<Duration> {
    let dec = Decoder::new(BufReader::new(File::open(path).ok()?)).ok()?;
    if let Some(td) = dec.total_duration() {
        return Some(td);
    }
    let sample_rate = dec.sample_rate() as u64;
    let channels = dec.channels().max(1) as u64;
    let frames = dec.count() as u64; // consumes the decoder, counting samples
    if sample_rate == 0 {
        return None;
    }
    Some(Duration::from_secs_f64(
        frames as f64 / (sample_rate * channels) as f64,
    ))
}

#[cfg(test)]
mod tests {
    use rodio::Decoder;
    use std::fs::File;
    use std::io::BufReader;
    use std::path::PathBuf;

    /// Every bundled clip must be decodable by rodio (validates the WMA->mp3 conversion).
    #[test]
    fn all_bundled_audio_decodes() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/audio");
        let files = [
            super::FAJR_FILE,
            super::DUA_FILE,
            super::style_file("makkah"),
            super::style_file("madina"),
            super::style_file("egypt"),
            super::style_file("alaqsa"),
        ];
        for f in files {
            let path = dir.join(f);
            let file = File::open(&path).unwrap_or_else(|e| panic!("open {path:?}: {e}"));
            Decoder::new(BufReader::new(file))
                .unwrap_or_else(|e| panic!("decode {path:?}: {e}"));
        }
    }
}
