//! Simulated time: play, pause, speed and row stepping. Never eased: every
//! change lands on the frame it is made.

/// Playback speeds, simulated seconds per wall second.
pub const SPEEDS: [f64; 7] = [1.0, 10.0, 60.0, 600.0, 3600.0, 1e4, 1e5];
/// Default: 60× (viewer-design skill).
pub const DEFAULT_SPEED: usize = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct Clock {
    /// Seconds after the table epoch.
    pub t: f64,
    pub playing: bool,
    /// Index into [`SPEEDS`].
    pub speed: usize,
    t0: f64,
    t1: f64,
}

impl Clock {
    pub fn new(t0: f64, t1: f64) -> Self {
        Self {
            t: t0,
            playing: false,
            speed: DEFAULT_SPEED,
            t0,
            t1,
        }
    }

    pub fn range(&self) -> (f64, f64) {
        (self.t0, self.t1)
    }

    /// Advances by `dt_wall` seconds of wall time while playing; stops at
    /// the end of the table.
    pub fn tick(&mut self, dt_wall: f64) {
        if self.playing {
            self.t += dt_wall * SPEEDS[self.speed];
            if self.t >= self.t1 {
                self.t = self.t1;
                self.playing = false;
            }
        }
    }

    /// Play or pause; playing from the end restarts at the beginning.
    pub fn toggle(&mut self) {
        if !self.playing && self.t >= self.t1 {
            self.t = self.t0;
        }
        self.playing = !self.playing;
    }

    pub fn seek(&mut self, t: f64) {
        self.t = t.clamp(self.t0, self.t1);
    }

    pub fn faster(&mut self) {
        self.speed = (self.speed + 1).min(SPEEDS.len() - 1);
    }

    pub fn slower(&mut self) {
        self.speed = self.speed.saturating_sub(1);
    }

    /// Moves `n` rows (negative: back) and pauses. From between two rows,
    /// one step forward lands on the next row and one step back on the
    /// previous one.
    pub fn step_rows(&mut self, times: &[f64], n: i64) {
        self.playing = false;
        if times.is_empty() || n == 0 {
            return;
        }
        let k = times.partition_point(|&x| x <= self.t);
        let i = k as i64 - 1;
        let on_row = k > 0 && times[k - 1] == self.t;
        let target = if on_row || n > 0 { i + n } else { i + 1 + n };
        let target = target.clamp(0, times.len() as i64 - 1) as usize;
        self.t = times[target];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_lands_on_rows() {
        let times = [0.0, 60.0, 120.0, 180.0];
        let mut c = Clock::new(0.0, 180.0);
        c.step_rows(&times, 1);
        assert_eq!(c.t, 60.0);
        c.seek(90.0);
        c.step_rows(&times, 1);
        assert_eq!(c.t, 120.0);
        c.seek(90.0);
        c.step_rows(&times, -1);
        assert_eq!(c.t, 60.0);
        c.step_rows(&times, -10);
        assert_eq!(c.t, 0.0);
        c.step_rows(&times, 10);
        assert_eq!(c.t, 180.0);
    }

    #[test]
    fn playback_stops_at_the_end_and_restarts() {
        let mut c = Clock::new(0.0, 100.0);
        c.toggle();
        c.tick(1.0); // 60×
        assert_eq!(c.t, 60.0);
        c.tick(1.0);
        assert_eq!((c.t, c.playing), (100.0, false));
        c.toggle();
        assert_eq!((c.t, c.playing), (0.0, true));
    }
}
