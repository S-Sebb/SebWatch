//! The tiles' last minute: a fixed ring of readings per tile, in memory only and never saved
//! (`docs/decisions.md`). A pause (minimised window, failed reads) leaves a gap, never a line
//! across it.

use std::time::Instant;

use crate::REFRESH;

/// Graphed series, one per `Measure`.
pub const MEASURES: usize = 5;

/// Readings in a tile's graph: the last 60 s at one per refresh.
pub const SAMPLES: usize = 30;

/// The graphed tiles, in `History`'s order.
#[derive(Clone, Copy)]
pub enum Measure {
    Cpu,
    Gpu,
    Vram,
    Disk,
    /// The Intel or AMD GPU (`drm.rs`), beside the NVIDIA one.
    Igpu,
}

/// Each series' readings as a share of its whole (Disk in bytes/s), `None` where there was none.
pub struct History {
    rings: [[Option<f32>; SAMPLES]; MEASURES],
    /// The slot of the newest reading.
    newest: usize,
    last: Option<Instant>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            rings: [[None; SAMPLES]; MEASURES],
            newest: 0,
            last: None,
        }
    }
}

impl History {
    /// Adds a reading taken at `at`, after one gap per refresh missed since the last. A reading
    /// less than half a refresh after the last (an app expanded) replaces it.
    pub fn push(&mut self, at: Instant, readings: [Option<f32>; MEASURES]) {
        let steps = match self.last {
            Some(last) => {
                let elapsed = at.saturating_duration_since(last).as_secs_f32();
                (elapsed / REFRESH.as_secs_f32()).round() as usize
            }
            None => 1,
        };
        for _ in 1..steps.min(SAMPLES + 1) {
            self.advance([None; MEASURES]);
        }
        if steps == 0 {
            self.set(readings);
        } else {
            self.advance(readings);
            self.last = Some(at);
        }
    }

    fn advance(&mut self, readings: [Option<f32>; MEASURES]) {
        self.newest = (self.newest + 1) % SAMPLES;
        self.set(readings);
    }

    fn set(&mut self, readings: [Option<f32>; MEASURES]) {
        for (ring, reading) in self.rings.iter_mut().zip(readings) {
            ring[self.newest] = reading;
        }
    }

    /// The series' readings, oldest first.
    pub fn readings(&self, measure: Measure) -> impl Iterator<Item = Option<f32>> + '_ {
        let ring = &self.rings[measure as usize];
        (1..=SAMPLES).map(move |i| ring[(self.newest + i) % SAMPLES])
    }

    /// The series as a line and the area under it, in SVG path commands for a 290 x 100 box with
    /// the newest reading on the right edge; empty without two readings in a row. CPU, GPU and
    /// VRAM as their share of the whole, Disk scaled to its own peak in the minute.
    pub fn graph(&self, measure: Measure) -> (String, String) {
        let scale = match measure {
            Measure::Disk => {
                let peak = self.readings(measure).flatten().fold(0.0, f32::max);
                if peak > 0.0 { 0.9 / peak } else { 0.0 }
            }
            _ => 1.0,
        };
        let point = |i: usize, v: f32| {
            // A floor of 2 % keeps an idle line visible along the bottom.
            let y = 100.0 - (v * scale).clamp(0.02, 1.0) * 100.0;
            (i as f32 * 10.0, y)
        };
        let (mut line, mut area) = (String::new(), String::new());
        // The previous point, and whether its run is drawn: a run starts at its second point.
        let mut previous: Option<((f32, f32), bool)> = None;
        let close = |area: &mut String, previous: Option<((f32, f32), bool)>| {
            if let Some(((x, _), true)) = previous {
                area.push_str(&format!("L {x} 100 Z "));
            }
        };
        for (i, reading) in self.readings(measure).enumerate() {
            let Some(v) = reading else {
                close(&mut area, previous.take());
                continue;
            };
            let (x, y) = point(i, v);
            if let Some(((x0, y0), drawn)) = previous {
                if !drawn {
                    line.push_str(&format!("M {x0} {y0:.1} "));
                    area.push_str(&format!("M {x0} 100 L {x0} {y0:.1} "));
                }
                line.push_str(&format!("L {x} {y:.1} "));
                area.push_str(&format!("L {x} {y:.1} "));
            }
            previous = Some(((x, y), previous.is_some()));
        }
        close(&mut area, previous);
        (line.trim_end().into(), area.trim_end().into())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn cpu(history: &History) -> Vec<Option<f32>> {
        history.readings(Measure::Cpu).collect()
    }

    #[test]
    fn keeps_the_last_minute_newest_last() {
        let start = Instant::now();
        let mut history = History::default();
        for i in 0..40 {
            history.push(
                start + REFRESH * i,
                [Some(i as f32), None, None, None, None],
            );
        }
        let expected: Vec<_> = (10..40).map(|i| Some(i as f32)).collect();
        assert_eq!(cpu(&history), expected);
    }

    #[test]
    fn a_pause_leaves_a_gap_and_a_long_one_restarts() {
        let start = Instant::now();
        let mut history = History::default();
        history.push(start, [Some(1.0); MEASURES]);
        history.push(start + REFRESH, [Some(2.0); MEASURES]);
        // Minimised for three refreshes.
        history.push(start + REFRESH * 4, [Some(3.0); MEASURES]);
        let readings = cpu(&history);
        assert_eq!(
            readings[SAMPLES - 5..],
            [Some(1.0), Some(2.0), None, None, Some(3.0)]
        );
        let (line, area) = history.graph(Measure::Cpu);
        assert_eq!(
            line.matches('M').count(),
            1,
            "no line across the gap: {line}"
        );
        assert_eq!(area.matches('Z').count(), 1);

        // Ten minutes away: only the newest reading is left, no line yet.
        history.push(start + REFRESH * 300, [Some(4.0); MEASURES]);
        assert_eq!(cpu(&history).iter().flatten().count(), 1);
        assert_eq!(history.graph(Measure::Cpu), (String::new(), String::new()));
    }

    #[test]
    fn a_reading_right_after_another_replaces_it() {
        let start = Instant::now();
        let mut history = History::default();
        history.push(start, [Some(1.0); MEASURES]);
        history.push(start + Duration::from_millis(300), [Some(2.0); MEASURES]);
        history.push(start + REFRESH, [Some(3.0); MEASURES]);
        assert_eq!(cpu(&history)[SAMPLES - 2..], [Some(2.0), Some(3.0)]);
    }

    #[test]
    fn disk_scales_to_its_peak_and_idle_stays_visible() {
        let start = Instant::now();
        let mut history = History::default();
        for (i, disk) in [0.0, 5e6, 10e6].into_iter().enumerate() {
            history.push(
                start + REFRESH * i as u32,
                [None, None, None, Some(disk), None],
            );
        }
        let (line, _) = history.graph(Measure::Disk);
        assert_eq!(line, "M 270 98.0 L 280 55.0 L 290 10.0");
    }
}
