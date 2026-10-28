//! Minimal Standard MIDI File reader (formats 0/1, PPQ timing): note events only.

use crate::Note;

/// One SMF track that contains notes.
#[derive(Debug, Clone, PartialEq)]
pub struct SmfTrack {
    /// Track name meta event, if any.
    pub name: Option<String>,
    /// Notes in beats from the file start, sorted by start.
    pub notes: Vec<Note>,
}

fn be(b: &[u8]) -> u32 {
    b.iter().fold(0, |a, &x| (a << 8) | x as u32)
}

fn varlen(d: &[u8], i: &mut usize) -> Result<u32, String> {
    let mut v = 0u32;
    for _ in 0..4 {
        let b = *d.get(*i).ok_or("truncated variable-length value")?;
        *i += 1;
        v = (v << 7) | (b & 0x7f) as u32;
        if b & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err("bad variable-length value".into())
}

/// Parse `.mid` bytes into the tracks that contain notes.
pub fn parse_smf(d: &[u8]) -> Result<Vec<SmfTrack>, String> {
    if d.len() < 14 || &d[0..4] != b"MThd" {
        return Err("not a MIDI file".into());
    }
    let hlen = be(&d[4..8]) as usize;
    let ntrk = be(&d[10..12]) as usize;
    let div = be(&d[12..14]);
    if div & 0x8000 != 0 || div == 0 {
        return Err("SMPTE-timed MIDI files are not supported".into());
    }
    let ppq = div as f64;
    let mut pos = 8 + hlen;
    let mut out = Vec::new();
    for _ in 0..ntrk {
        if pos + 8 > d.len() {
            break;
        }
        let len = be(&d[pos + 4..pos + 8]) as usize;
        let body = &d[pos + 8..(pos + 8 + len).min(d.len())];
        let is_track = &d[pos..pos + 4] == b"MTrk";
        pos += 8 + len;
        if !is_track {
            continue;
        }
        let (mut i, mut tick, mut status) = (0usize, 0u64, 0u8);
        let mut name = None;
        let mut open: Vec<(u8, u8, u64, u8)> = Vec::new(); // (channel, pitch, start tick, velocity)
        let mut notes = Vec::new();
        while i < body.len() {
            tick += varlen(body, &mut i)? as u64;
            let mut b = *body.get(i).ok_or("truncated event")?;
            if b & 0x80 != 0 {
                i += 1;
                if b < 0xf0 {
                    status = b;
                }
            } else {
                b = status; // running status
            }
            match b {
                0xff => {
                    let ty = *body.get(i).ok_or("truncated meta event")?;
                    i += 1;
                    let l = varlen(body, &mut i)? as usize;
                    let data = body.get(i..i + l).ok_or("truncated meta event")?;
                    if ty == 0x03 && name.is_none() {
                        name = Some(String::from_utf8_lossy(data).trim().to_string()).filter(|s| !s.is_empty());
                    }
                    i += l;
                    if ty == 0x2f {
                        break;
                    }
                }
                0xf0 | 0xf7 => {
                    let l = varlen(body, &mut i)? as usize;
                    i += l;
                }
                0x80..=0xef => {
                    let n = if (0xc0..0xe0).contains(&b) { 1 } else { 2 };
                    let data = body.get(i..i + n).ok_or("truncated channel event")?;
                    i += n;
                    let ch = b & 0x0f;
                    let (on, off) = (b & 0xf0 == 0x90 && data[1] > 0, b & 0xf0 == 0x80 || (b & 0xf0 == 0x90 && data[1] == 0));
                    if on {
                        open.push((ch, data[0], tick, data[1]));
                    } else if off && let Some(k) = open.iter().position(|o| o.0 == ch && o.1 == data[0]) {
                        let (_, pitch, t0, vel) = open.remove(k);
                        let len = ((tick - t0) as f64 / ppq).max(1.0 / 64.0);
                        notes.push(Note { start: t0 as f64 / ppq, length: len, pitch, velocity: vel.max(1), ..Default::default() });
                    }
                }
                _ => return Err(format!("unexpected MIDI status byte {b:#04x}")),
            }
        }
        // Hanging notes end at the last event.
        for (_, pitch, t0, vel) in open {
            let len = ((tick.saturating_sub(t0)) as f64 / ppq).max(1.0 / 16.0);
            notes.push(Note { start: t0 as f64 / ppq, length: len, pitch, velocity: vel.max(1), ..Default::default() });
        }
        if !notes.is_empty() {
            notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
            out.push(SmfTrack { name, notes });
        }
    }
    if out.is_empty() {
        return Err("the MIDI file contains no notes".into());
    }
    Ok(out)
}

impl crate::Project {
    /// Place a parsed MIDI file at `at` beats. `target` = a MIDI track: all
    /// notes merged into one clip there (overwriting); else one new MIDI track
    /// per file track, named after it (or `stem`). Returns `(track, clip id)`s.
    pub fn import_midi(&mut self, file: &[SmfTrack], target: Option<usize>, at: f64, stem: &str) -> Vec<(usize, u64)> {
        let bpb = self.beats_per_bar.max(1) as f64;
        let clip_len = |notes: &[Note]| {
            let end = notes.iter().map(|n| n.start + n.length).fold(0.0, f64::max);
            ((end / bpb - 1e-9).ceil().max(1.0)) * bpb
        };
        let place = |p: &mut Self, t: usize, notes: Vec<Note>, name: String| {
            let length = clip_len(&notes);
            let id = p.alloc_id();
            let color = p.new_clip_color(t);
            p.clear_arrangement_range(t, at, at + length, |_| 1.0);
            let tr = &mut p.tracks[t];
            tr.arrangement.push(crate::ArrangementClip {
                start: at,
                clip: crate::Clip {
                    id,
                    name,
                    color,
                    length,
                    looping: false,
                    content: crate::ClipContent::Midi { notes },
                    ..Default::default()
                },
            });
            tr.arrangement.sort_by(|a, b| a.start.total_cmp(&b.start));
            (t, id)
        };
        match target.filter(|&t| self.tracks.get(t).is_some_and(|x| x.kind == crate::TrackKind::Midi)) {
            Some(t) => {
                let mut notes: Vec<Note> = file.iter().flat_map(|x| x.notes.iter().copied()).collect();
                notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.cmp(&b.pitch)));
                vec![place(self, t, notes, stem.to_string())]
            }
            None => {
                let mut out = Vec::new();
                for x in file {
                    if self.tracks.len() >= daw_engine::MAX_TRACKS {
                        break;
                    }
                    let name = x.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| stem.to_string());
                    let t = self.add_track(crate::TrackKind::Midi);
                    self.tracks[t].name = name.clone();
                    out.push(place(self, t, x.notes.clone(), name));
                }
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smf(tracks: &[&[u8]]) -> Vec<u8> {
        let mut v = b"MThd\0\0\0\x06\0\x01".to_vec();
        v.extend((tracks.len() as u16).to_be_bytes());
        v.extend(96u16.to_be_bytes());
        for t in tracks {
            v.extend(b"MTrk");
            v.extend((t.len() as u32).to_be_bytes());
            v.extend(*t);
        }
        v
    }

    #[test]
    fn reads_notes_names_and_running_status() {
        // Name "Lead"; C4 at 0 for 1 beat; E4 at beat 1 (running status, note-on vel 0 = off).
        let t: &[u8] = &[
            0x00, 0xff, 0x03, 4, b'L', b'e', b'a', b'd', 0x00, 0x90, 60, 100, 0x60, 0x80, 60, 0, 0x00, 0x90, 64, 90, 0x30, 64, 0, 0x00,
            0xff, 0x2f, 0x00,
        ];
        let empty: &[u8] = &[0x00, 0xff, 0x2f, 0x00];
        let tr = parse_smf(&smf(&[empty, t])).unwrap();
        assert_eq!(tr.len(), 1, "tracks without notes are skipped");
        assert_eq!(tr[0].name.as_deref(), Some("Lead"));
        let n = &tr[0].notes;
        assert_eq!((n[0].pitch, n[0].start, n[0].length, n[0].velocity), (60, 0.0, 1.0, 100));
        assert_eq!((n[1].pitch, n[1].start, n[1].length), (64, 1.0, 0.5));
        assert!(parse_smf(b"RIFF....").is_err());
    }

    fn file() -> Vec<SmfTrack> {
        let n = |pitch, start, length| Note { pitch, start, length, velocity: 100, muted: false, mpe: 0 };
        vec![
            SmfTrack { name: Some("Lead".into()), notes: vec![n(60, 0.0, 1.0), n(64, 4.5, 1.0)] },
            SmfTrack { name: None, notes: vec![n(36, 0.0, 0.5)] },
        ]
    }

    #[test]
    fn import_creates_one_midi_track_per_file_track() {
        let mut p = crate::Project::new_default();
        let before = p.tracks.len();
        let out = p.import_midi(&file(), None, 8.0, "song");
        assert_eq!(out.len(), 2);
        assert_eq!(p.tracks.len(), before + 2);
        let (t0, id0) = out[0];
        assert_eq!(p.tracks[t0].kind, crate::TrackKind::Midi);
        assert_eq!(p.tracks[t0].name, "Lead");
        assert_eq!(p.tracks[out[1].0].name, "song", "unnamed tracks use the file stem");
        let a = &p.tracks[t0].arrangement[0];
        assert_eq!((a.clip.id, a.start), (id0, 8.0));
        assert_eq!(a.clip.length, 8.0, "5.5 beats rounded up to whole 4/4 bars");
    }

    #[test]
    fn import_onto_midi_track_merges_into_one_clip() {
        let mut p = crate::Project::new_default();
        let t = p.add_track(crate::TrackKind::Midi);
        let before = p.tracks.len();
        let out = p.import_midi(&file(), Some(t), 0.0, "song");
        assert_eq!(p.tracks.len(), before, "no new tracks");
        assert_eq!(out, vec![(t, p.tracks[t].arrangement[0].clip.id)]);
        let crate::ClipContent::Midi { notes } = &p.tracks[t].arrangement[0].clip.content else { panic!() };
        assert_eq!(notes.iter().map(|n| n.pitch).collect::<Vec<_>>(), vec![36, 60, 64]);
        assert_eq!(p.tracks[t].arrangement[0].clip.name, "song");
    }
}
