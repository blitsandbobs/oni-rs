//! Oni's ending movie decoded as FFmpeg decodes it: every frame to the pixel
//! (in YUV, before any conversion) and the audio to a millionth. Needs the
//! movie (`ONI_OUTRO`, or `generated/oni/movies/outro.bik` beside the
//! repository) and `ffmpeg` on the `PATH`; skipped quietly without either.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use oni_bink::Bink;

fn movie() -> Option<PathBuf> {
    let path = std::env::var_os("ONI_OUTRO")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../../generated/oni/movies/outro.bik")
        });
    path.is_file().then_some(path)
}

fn ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn outro_is_ffmpegs() {
    let (Some(path), true) = (movie(), ffmpeg()) else {
        eprintln!("no outro.bik or no ffmpeg: skipping");
        return;
    };
    let bink = Bink::parse(std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        (
            bink.revision,
            bink.width,
            bink.height,
            bink.rate,
            bink.frame_count()
        ),
        (b'i', 512, 384, (15, 1), 3165)
    );
    assert_eq!(
        (
            bink.tracks.len(),
            bink.tracks[0].sample_rate,
            bink.tracks[0].channels()
        ),
        (1, 44100, 2)
    );

    // The video.
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&path)
        .args(["-map", "0:v", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = std::io::BufReader::with_capacity(1 << 20, child.stdout.take().unwrap());
    let mut decoder = bink.video_decoder().unwrap();
    for frame in 0..bink.frame_count() {
        let picture = decoder.decode(bink.packets(frame).unwrap().video).unwrap();
        for (plane, name) in picture.planes.iter().zip(["Y", "U", "V"]) {
            for (y, row) in plane.rows().enumerate() {
                let mut expected = vec![0u8; row.len()];
                out.read_exact(&mut expected)
                    .expect("FFmpeg has fewer frames");
                if let Some(x) = (0..row.len()).find(|&x| row[x] != expected[x]) {
                    panic!(
                        "frame {frame} {name} ({x}, {y}): {} for FFmpeg's {}",
                        row[x], expected[x]
                    );
                }
            }
        }
    }
    assert_eq!(
        out.read(&mut [0u8; 1]).unwrap(),
        0,
        "FFmpeg has more frames"
    );
    drop(out);
    child.wait().unwrap();

    // The audio.
    let expected = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&path)
        .args(["-map", "0:a", "-f", "f32le", "-"])
        .output()
        .unwrap()
        .stdout;
    let expected: Vec<f32> = expected
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let mut decoder = bink.audio_decoder(0).unwrap();
    let mut ours = Vec::new();
    for frame in 0..bink.frame_count() {
        decoder.decode(bink.packets(frame).unwrap().audio[0], &mut ours);
    }
    assert_eq!(ours.len(), expected.len());
    let worst = ours
        .iter()
        .zip(&expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(worst <= 1e-6, "audio off by {worst}");
}
