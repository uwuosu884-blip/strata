/// Planar stereo work buffer, allocated once up front.
///
/// The engine works on separate per-channel arrays ("planar"), which is
/// faster for DSP and SIMD than the interleaved L R L R layout that sound
/// cards use. Conversion happens once, at the very end.
pub struct StereoBuffer {
    left: Vec<f32>,
    right: Vec<f32>,
}

impl StereoBuffer {
    pub fn new(max_frames: usize) -> Self {
        Self { left: vec![0.0; max_frames], right: vec![0.0; max_frames] }
    }

    pub fn capacity(&self) -> usize {
        self.left.len()
    }

    pub fn clear(&mut self, frames: usize) {
        self.left[..frames].fill(0.0);
        self.right[..frames].fill(0.0);
    }

    pub fn channels_mut(&mut self, frames: usize) -> (&mut [f32], &mut [f32]) {
        (&mut self.left[..frames], &mut self.right[..frames])
    }

    pub fn range_mut(&mut self, offset: usize, n: usize) -> (&mut [f32], &mut [f32]) {
        (&mut self.left[offset..offset + n], &mut self.right[offset..offset + n])
    }

    pub fn peak(&self, frames: usize) -> [f32; 2] {
        let p = |ch: &[f32]| ch.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        [p(&self.left[..frames]), p(&self.right[..frames])]
    }

    /// Write `frames` frames to an interleaved buffer with `channels` channels.
    /// Mono outputs get (L+R)/2; channels beyond 2 get silence.
    pub fn write_interleaved(&self, out: &mut [f32], channels: usize, frames: usize) {
        let (l, r) = (&self.left[..frames], &self.right[..frames]);
        match channels {
            1 => {
                for (o, (a, b)) in out.iter_mut().zip(l.iter().zip(r)) {
                    *o = 0.5 * (a + b);
                }
            }
            2 => {
                for (frame, (a, b)) in out.chunks_exact_mut(2).zip(l.iter().zip(r)) {
                    frame[0] = *a;
                    frame[1] = *b;
                }
            }
            _ => {
                for (frame, (a, b)) in out.chunks_exact_mut(channels).zip(l.iter().zip(r)) {
                    frame[0] = *a;
                    frame[1] = *b;
                    frame[2..].fill(0.0);
                }
            }
        }
    }
}
