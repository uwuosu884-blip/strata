//! Project files on disk. In memory every sample reference is absolute (or
//! `factory:` / `recording:`); on disk, files inside the project folder are
//! stored relative so a project folder can be moved or shared.

use crate::export::write_wav_channels;
use crate::samples::SampleCache;
use daw_project::{ClipContent, FACTORY_PREFIX, InstrumentDef, Project, ProjectError, RECORDING_PREFIX};
use std::path::{Path, PathBuf};

fn for_each_ref(p: &mut Project, mut f: impl FnMut(&mut String)) {
    for t in &mut p.tracks {
        match &mut t.instrument {
            InstrumentDef::Sampler { sample: Some(s), .. } => f(s),
            InstrumentDef::Drums { pads } => pads.iter_mut().filter_map(|d| d.sample.as_mut()).for_each(&mut f),
            _ => {}
        }
        for c in t.slots.iter_mut().flatten().chain(t.arrangement.iter_mut().map(|a| &mut a.clip)) {
            if let ClipContent::Audio { sample, .. } = &mut c.content {
                f(sample);
            }
        }
    }
}

fn is_special(s: &str) -> bool {
    s.starts_with(FACTORY_PREFIX) || s.starts_with(RECORDING_PREFIX)
}

/// Folder where recordings of `project_path` are written.
pub fn samples_dir(project_path: &Path) -> PathBuf {
    let stem = project_path.file_stem().and_then(|s| s.to_str()).unwrap_or("project");
    project_path.with_file_name(format!("{stem} Samples"))
}

pub fn load_project(path: &Path) -> Result<Project, ProjectError> {
    let mut p = Project::load(path)?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    for_each_ref(&mut p, |s| {
        if !is_special(s) && Path::new(s).is_relative() {
            *s = dir.join(&*s).to_string_lossy().into_owned();
        }
    });
    Ok(p)
}

/// Save `project` to `path`. Recordings still in memory are written as WAV
/// into the samples folder first and `project` (and `cache`) are re-pointed to them.
pub fn save_project(project: &mut Project, path: &Path, cache: &mut SampleCache) -> Result<(), ProjectError> {
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let sdir = samples_dir(path);
    let mut err: Option<ProjectError> = None;
    let mut renames: Vec<(String, String)> = Vec::new();
    for_each_ref(project, |s| {
        if err.is_some() || !s.starts_with(RECORDING_PREFIX) {
            return;
        }
        if let Some((_, to)) = renames.iter().find(|(from, _)| from == s) {
            *s = to.clone();
            return;
        }
        let Some(buf) = cache.peek(s).cloned() else { return };
        if let Err(e) = std::fs::create_dir_all(&sdir) {
            err = Some(ProjectError::Io(e));
            return;
        }
        let name = s.trim_start_matches(RECORDING_PREFIX).replace(['/', '\\', ':'], "_");
        // Live's layout: recordings in <Samples>/Recorded; mono takes stay mono.
        let rdir = sdir.join("Recorded");
        if let Err(e) = std::fs::create_dir_all(&rdir) {
            err = Some(ProjectError::Io(e));
            return;
        }
        let file = rdir.join(format!("{name}.wav"));
        let (data, channels): (Vec<f32>, u16) = if buf.left == buf.right {
            (buf.left.clone(), 1)
        } else {
            (buf.left.iter().zip(&buf.right).flat_map(|(l, r)| [*l, *r]).collect(), 2)
        };
        if let Err(e) = write_wav_channels(&file, &data, buf.sample_rate, 32, channels) {
            err = Some(ProjectError::Io(std::io::Error::other(e)));
            return;
        }
        let to = file.to_string_lossy().into_owned();
        renames.push((s.clone(), to.clone()));
        *s = to;
    });
    if let Some(e) = err {
        return Err(e);
    }
    for (from, to) in &renames {
        cache.rename(from, to);
    }
    let mut on_disk = project.clone();
    for_each_ref(&mut on_disk, |s| {
        if !is_special(s)
            && let Ok(rel) = Path::new(&*s).strip_prefix(&dir)
        {
            // Always '/' on disk so projects move between Windows/Linux/macOS
            // (Windows accepts '/' too).
            *s = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
        }
    });
    on_disk.save(path)
}

/// Crash-recovery copy of `project` at `path`. Unlike [`save_project`] it never
/// changes `project` or `cache`; recordings are written once (files of an
/// earlier recovery save are reused) and the project file is replaced atomically.
pub fn save_recovery(project: &Project, path: &Path, cache: &SampleCache) -> Result<(), ProjectError> {
    let rdir = samples_dir(path).join("Recorded");
    let mut p = project.clone();
    let mut err: Option<ProjectError> = None;
    for_each_ref(&mut p, |s| {
        if err.is_some() || !s.starts_with(RECORDING_PREFIX) {
            return;
        }
        let Some(buf) = cache.peek(s) else { return };
        let name = s.trim_start_matches(RECORDING_PREFIX).replace(['/', '\\', ':'], "_");
        let file = rdir.join(format!("{name}.wav"));
        if !file.exists() {
            let (data, channels): (Vec<f32>, u16) = if buf.left == buf.right {
                (buf.left.clone(), 1)
            } else {
                (buf.left.iter().zip(&buf.right).flat_map(|(l, r)| [*l, *r]).collect(), 2)
            };
            let tmp = file.with_extension("wav.part");
            let r = std::fs::create_dir_all(&rdir)
                .map_err(ProjectError::Io)
                .and_then(|()| {
                    write_wav_channels(&tmp, &data, buf.sample_rate, 32, channels).map_err(|e| ProjectError::Io(std::io::Error::other(e)))
                })
                .and_then(|()| std::fs::rename(&tmp, &file).map_err(ProjectError::Io));
            if let Err(e) = r {
                err = Some(e);
                return;
            }
        }
        *s = file.to_string_lossy().into_owned();
    });
    if let Some(e) = err {
        return Err(e);
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(ProjectError::Io)?;
    }
    let tmp = path.with_extension("part");
    p.save(&tmp)?;
    std::fs::rename(&tmp, path).map_err(ProjectError::Io)?;
    Ok(())
}

/// Load a recovery copy written by [`save_recovery`]. Its recordings become
/// in-memory recordings again, so the next real save copies them into that
/// project's own folder (the recovery folder can then be deleted).
pub fn load_recovery(path: &Path, cache: &mut SampleCache) -> Result<Project, ProjectError> {
    let mut p = Project::load(path)?;
    let rdir = samples_dir(path).join("Recorded");
    let mut renames: Vec<(String, String)> = Vec::new();
    for_each_ref(&mut p, |s| {
        if let Some((_, to)) = renames.iter().find(|(from, _)| from == s) {
            *s = to.clone();
            return;
        }
        let f = Path::new(&*s);
        if f.parent() != Some(rdir.as_path()) {
            return;
        }
        let Some(stem) = f.file_stem().and_then(|x| x.to_str()).map(str::to_string) else { return };
        if cache.load_blocking(s).is_ok() {
            let key = format!("{RECORDING_PREFIX}{stem}");
            cache.rename(s, &key);
            renames.push((s.clone(), key.clone()));
            *s = key;
        }
    });
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_engine::SampleBuffer;
    use daw_project::Clip;

    #[test]
    fn recordings_are_written_and_paths_made_relative() {
        let root = std::env::temp_dir().join(format!("daw-persist-{}", std::process::id()));
        let dir = root.join("songs");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("My Song.dawproj");
        let mut cache = SampleCache::new(48_000);
        cache.insert("recording:take1", SampleBuffer::new("take1", 48_000, vec![0.25; 4800], vec![-0.25; 4800]));
        cache.insert("recording:mono1", SampleBuffer::mono("mono1", 48_000, vec![0.5; 480]));
        let mut p = Project::new_default();
        let audio = |s: &str| {
            Some(Clip {
                content: ClipContent::Audio { sample: s.into(), offset_frames: 0.0, warp_bpm: None, gain_db: 0.0 },
                ..Default::default()
            })
        };
        p.tracks[3].slots[0] = audio("recording:take1");
        p.tracks[3].slots[1] = audio("recording:take1");
        p.tracks[3].slots[2] = audio("/elsewhere/loop.wav");
        p.tracks[2].slots[0] = audio("recording:mono1");

        save_project(&mut p, &path, &mut cache).unwrap();
        let wav = dir.join("My Song Samples").join("Recorded").join("take1.wav");
        assert!(wav.exists());
        // (Reader closed right away: Windows can't rename a folder with open files.)
        let mono_ch = hound::WavReader::open(dir.join("My Song Samples/Recorded/mono1.wav")).unwrap().spec().channels;
        assert_eq!(mono_ch, 1, "mono input → mono file");
        let wav_s = wav.to_string_lossy().into_owned();
        let refs: Vec<String> = p.tracks[3]
            .slots
            .iter()
            .flatten()
            .map(|c| match &c.content {
                ClipContent::Audio { sample, .. } => sample.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(refs, vec![wav_s.clone(), wav_s.clone(), "/elsewhere/loop.wav".into()]);
        assert!(cache.peek(&wav_s).is_some());

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"My Song Samples/Recorded/take1.wav\""), "stored relative");
        assert!(text.contains("factory:Kick"));

        // Move the whole folder: still loads.
        let moved = root.join("moved");
        std::fs::rename(&dir, &moved).unwrap();
        let back = load_project(&moved.join("My Song.dawproj")).unwrap();
        match &back.tracks[3].slots[0].as_ref().unwrap().content {
            ClipContent::Audio { sample, .. } => {
                assert!(Path::new(sample).exists(), "{sample}");
                assert_eq!(crate::decode_file(Path::new(sample)).unwrap().left[10], 0.25);
            }
            _ => unreachable!(),
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn recovery_round_trip_keeps_recordings_in_memory() {
        let dir = std::env::temp_dir().join(format!("daw-recovery-{}", std::process::id()));
        let path = dir.join("Recovery.dawproj");
        let mut cache = SampleCache::new(48_000);
        cache.insert("recording:take 1", SampleBuffer::new("take 1", 48_000, vec![0.25; 480], vec![-0.25; 480]));
        let mut p = Project::new_default();
        let audio = |s: &str| {
            Some(Clip {
                content: ClipContent::Audio { sample: s.into(), offset_frames: 0.0, warp_bpm: None, gain_db: 0.0 },
                ..Default::default()
            })
        };
        p.tracks[3].slots[0] = audio("recording:take 1");
        p.tracks[3].slots[1] = audio("recording:take 1");
        let before = p.clone();
        save_recovery(&p, &path, &cache).unwrap();
        save_recovery(&p, &path, &cache).unwrap(); // second save reuses the WAV
        assert_eq!(p, before, "the open project is untouched");
        assert!(cache.peek("recording:take 1").is_some());
        let mut fresh = SampleCache::new(48_000);
        let r = load_recovery(&path, &mut fresh).unwrap();
        let refs = |p: &Project| -> Vec<ClipContent> { p.tracks[3].slots.iter().flatten().map(|c| c.content.clone()).collect() };
        assert_eq!(refs(&r), refs(&before), "refs are recordings again");
        let b = fresh.peek("recording:take 1").expect("recording back in memory");
        assert_eq!((b.len(), b.left[0], b.right[0]), (480, 0.25, -0.25));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
