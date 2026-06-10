use rodio::{Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::Duration;

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
        let (_stream, handle) = match OutputStream::try_default() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("audio: no output device available: {e}");
                return;
            }
        };
        let mut current: Option<Sink> = None;
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(AudioCmd::SetVolume(v)) => {
                    if let Some(s) = current.as_ref() {
                        s.set_volume(v.clamp(0.0, 1.0));
                    }
                }
                Ok(AudioCmd::Stop) => {
                    if let Some(s) = current.take() {
                        s.stop();
                        on_ended();
                    }
                }
                Ok(AudioCmd::Play { paths, volume }) => {
                    if let Some(s) = current.take() {
                        s.stop();
                    }
                    match Sink::try_new(&handle) {
                        Ok(sink) => {
                            sink.set_volume(volume.clamp(0.0, 1.0));
                            for p in paths {
                                match File::open(&p) {
                                    Ok(f) => match Decoder::new(BufReader::new(f)) {
                                        Ok(src) => sink.append(src),
                                        Err(e) => eprintln!("audio: decode {p:?}: {e}"),
                                    },
                                    Err(e) => eprintln!("audio: open {p:?}: {e}"),
                                }
                            }
                            current = Some(sink);
                        }
                        Err(e) => eprintln!("audio: create sink: {e}"),
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Detect the queue draining on its own.
                    if current.as_ref().is_some_and(|s| s.empty()) {
                        current = None;
                        on_ended();
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    tx
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
