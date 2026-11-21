//! Pomodoro state machine. Pure logic: it never reads the clock itself, every
//! call takes `now` so it can be driven by the UI loop and by tests alike.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Focus => "Foco",
            Phase::ShortBreak => "Pausa curta",
            Phase::LongBreak => "Pausa longa",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Phase::Focus => "focus",
            Phase::ShortBreak => "short_break",
            Phase::LongBreak => "long_break",
        }
    }

    pub fn is_break(self) -> bool {
        self != Phase::Focus
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Durations {
    pub focus: Duration,
    pub short_break: Duration,
    pub long_break: Duration,
    /// A long break replaces the short one after this many focus sessions.
    pub long_break_every: u32,
}

impl Durations {
    pub fn of(&self, phase: Phase) -> Duration {
        match phase {
            Phase::Focus => self.focus,
            Phase::ShortBreak => self.short_break,
            Phase::LongBreak => self.long_break,
        }
    }
}

impl Default for Durations {
    fn default() -> Self {
        Self {
            focus: Duration::from_secs(25 * 60),
            short_break: Duration::from_secs(5 * 60),
            long_break: Duration::from_secs(15 * 60),
            long_break_every: 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RunState {
    Idle,
    Running { since: Instant, banked: Duration },
    Paused { elapsed: Duration },
}

/// A phase that ran to the end (or was skipped after some time).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finished {
    pub phase: Phase,
    pub elapsed: Duration,
    /// False when the user skipped the phase before it ended.
    pub completed: bool,
}

#[derive(Clone, Debug)]
pub struct Timer {
    durations: Durations,
    phase: Phase,
    state: RunState,
    /// Focus sessions finished in the current set (resets after a long break).
    focus_in_set: u32,
    /// Extra time added to the current phase via [`Timer::extend`].
    extra: Duration,
}

impl Timer {
    pub fn new(durations: Durations) -> Self {
        Self {
            durations,
            phase: Phase::Focus,
            state: RunState::Idle,
            focus_in_set: 0,
            extra: Duration::ZERO,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn durations(&self) -> Durations {
        self.durations
    }

    /// Updates durations; an idle timer picks them up right away, a running
    /// one keeps its elapsed time against the new total.
    pub fn set_durations(&mut self, durations: Durations) {
        self.durations = durations;
        self.focus_in_set = self.focus_in_set.min(durations.long_break_every.max(1) - 1);
    }

    pub fn is_running(&self) -> bool {
        matches!(self.state, RunState::Running { .. })
    }

    pub fn is_paused(&self) -> bool {
        matches!(self.state, RunState::Paused { .. })
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, RunState::Idle)
    }

    /// 1-based position of the current focus session inside the set.
    pub fn cycle_position(&self) -> u32 {
        let every = self.durations.long_break_every.max(1);
        match self.phase {
            Phase::Focus => self.focus_in_set % every + 1,
            // During a break we are still "on" the session that just ended.
            _ => self.focus_in_set.max(1).min(every),
        }
    }

    pub fn cycle_length(&self) -> u32 {
        self.durations.long_break_every.max(1)
    }

    pub fn total(&self) -> Duration {
        self.durations.of(self.phase).saturating_add(self.extra)
    }

    /// Extends the current phase by `extra`. Cleared on reset, phase selection
    /// or advancing to the next phase.
    pub fn extend(&mut self, extra: Duration) {
        self.extra = self.extra.saturating_add(extra);
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        let elapsed = match self.state {
            RunState::Idle => Duration::ZERO,
            RunState::Running { since, banked } => banked + now.saturating_duration_since(since),
            RunState::Paused { elapsed } => elapsed,
        };
        elapsed.min(self.total())
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.total().saturating_sub(self.elapsed(now))
    }

    /// 0.0 at the start of the phase, 1.0 when it ends.
    pub fn progress(&self, now: Instant) -> f32 {
        let total = self.total().as_secs_f32();
        if total <= 0.0 {
            return 1.0;
        }
        (self.elapsed(now).as_secs_f32() / total).clamp(0.0, 1.0)
    }

    pub fn start(&mut self, now: Instant) {
        self.state = match self.state {
            RunState::Idle => RunState::Running {
                since: now,
                banked: Duration::ZERO,
            },
            RunState::Paused { elapsed } => RunState::Running {
                since: now,
                banked: elapsed,
            },
            running => running,
        };
    }

    pub fn pause(&mut self, now: Instant) {
        if let RunState::Running { .. } = self.state {
            self.state = RunState::Paused {
                elapsed: self.elapsed(now),
            };
        }
    }

    pub fn toggle(&mut self, now: Instant) {
        if self.is_running() {
            self.pause(now)
        } else {
            self.start(now)
        }
    }

    /// Back to the start of the current phase, stopped.
    pub fn reset(&mut self) {
        self.state = RunState::Idle;
        self.extra = Duration::ZERO;
    }

    /// Jumps to `phase` without counting anything as finished.
    pub fn select_phase(&mut self, phase: Phase) {
        self.phase = phase;
        self.state = RunState::Idle;
        self.extra = Duration::ZERO;
    }

    /// Ends the current phase early. Returns it when any time was spent in it.
    pub fn skip(&mut self, now: Instant) -> Option<Finished> {
        let elapsed = self.elapsed(now);
        let finished = (!elapsed.is_zero()).then_some(Finished {
            phase: self.phase,
            elapsed,
            completed: false,
        });
        self.advance(false);
        finished
    }

    /// Call periodically. When the phase has run out, moves to the next one
    /// and returns what finished. `auto_start` decides whether the next phase
    /// starts counting immediately.
    pub fn tick(&mut self, now: Instant, auto_start: impl Fn(Phase) -> bool) -> Option<Finished> {
        if !self.is_running() || self.remaining(now) > Duration::ZERO {
            return None;
        }
        let finished = Finished {
            phase: self.phase,
            elapsed: self.total(),
            completed: true,
        };
        self.advance(true);
        if auto_start(self.phase) {
            self.start(now);
        }
        Some(finished)
    }

    fn advance(&mut self, focus_counts: bool) {
        let every = self.durations.long_break_every.max(1);
        self.phase = match self.phase {
            Phase::Focus => {
                if focus_counts {
                    self.focus_in_set += 1;
                }
                if focus_counts && self.focus_in_set >= every {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            Phase::LongBreak => {
                self.focus_in_set = 0;
                Phase::Focus
            }
            Phase::ShortBreak => Phase::Focus,
        };
        self.state = RunState::Idle;
        self.extra = Duration::ZERO;
    }
}

pub fn format_clock(d: Duration) -> String {
    // Round up so the display never shows 00:00 while time is left.
    let secs = d.as_secs() + u64::from(d.subsec_nanos() > 0);
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick() -> Durations {
        Durations {
            focus: Duration::from_secs(10),
            short_break: Duration::from_secs(2),
            long_break: Duration::from_secs(5),
            long_break_every: 2,
        }
    }

    #[test]
    fn counts_down_and_pauses() {
        let t0 = Instant::now();
        let mut t = Timer::new(quick());
        assert_eq!(t.remaining(t0), Duration::from_secs(10));
        t.start(t0);
        assert_eq!(
            t.remaining(t0 + Duration::from_secs(3)),
            Duration::from_secs(7)
        );
        t.pause(t0 + Duration::from_secs(3));
        assert_eq!(
            t.remaining(t0 + Duration::from_secs(60)),
            Duration::from_secs(7)
        );
        t.start(t0 + Duration::from_secs(60));
        assert_eq!(
            t.remaining(t0 + Duration::from_secs(62)),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn cycles_into_long_break() {
        let t0 = Instant::now();
        let mut t = Timer::new(quick());
        let always = |_| true;
        t.start(t0);
        let f = t.tick(t0 + Duration::from_secs(10), always).unwrap();
        assert_eq!(f.phase, Phase::Focus);
        assert!(f.completed);
        assert_eq!(t.phase(), Phase::ShortBreak);
        assert!(t.is_running());
        t.tick(t0 + Duration::from_secs(12), always).unwrap();
        assert_eq!(t.phase(), Phase::Focus);
        assert_eq!(t.cycle_position(), 2);
        t.tick(t0 + Duration::from_secs(22), always).unwrap();
        assert_eq!(t.phase(), Phase::LongBreak);
        t.tick(t0 + Duration::from_secs(27), always).unwrap();
        assert_eq!(t.phase(), Phase::Focus);
        assert_eq!(t.cycle_position(), 1);
    }

    #[test]
    fn tick_without_auto_start_stops() {
        let t0 = Instant::now();
        let mut t = Timer::new(quick());
        t.start(t0);
        t.tick(t0 + Duration::from_secs(11), |_| false).unwrap();
        assert!(t.is_idle());
        assert_eq!(t.phase(), Phase::ShortBreak);
        assert!(t.tick(t0 + Duration::from_secs(30), |_| false).is_none());
    }

    #[test]
    fn skipping_focus_does_not_count_towards_long_break() {
        let t0 = Instant::now();
        let mut t = Timer::new(quick());
        t.start(t0);
        let f = t.skip(t0 + Duration::from_secs(4)).unwrap();
        assert!(!f.completed);
        assert_eq!(f.elapsed, Duration::from_secs(4));
        assert_eq!(t.phase(), Phase::ShortBreak);
        assert!(t.skip(t0).is_none());
        assert_eq!(t.phase(), Phase::Focus);
        assert_eq!(t.cycle_position(), 1);
    }

    #[test]
    fn extend_while_running_adds_time_and_clears_on_advance() {
        let t0 = Instant::now();
        let mut t = Timer::new(quick());
        t.start(t0);
        t.extend(Duration::from_secs(5));
        assert!(t.is_running());
        assert_eq!(t.total(), Duration::from_secs(15));
        assert_eq!(
            t.remaining(t0 + Duration::from_secs(8)),
            Duration::from_secs(7)
        );
        // Original end no longer finishes the phase.
        assert!(t.tick(t0 + Duration::from_secs(10), |_| false).is_none());
        assert_eq!(t.phase(), Phase::Focus);
        // Extended end finishes the phase and clears extra on advance.
        let f = t.tick(t0 + Duration::from_secs(15), |_| false).unwrap();
        assert!(f.completed);
        assert_eq!(f.elapsed, Duration::from_secs(15));
        assert_eq!(t.phase(), Phase::ShortBreak);
        assert_eq!(t.total(), Duration::from_secs(2));
    }

    #[test]
    fn extend_while_paused_adds_time() {
        let t0 = Instant::now();
        let mut t = Timer::new(quick());
        t.start(t0);
        t.pause(t0 + Duration::from_secs(4));
        t.extend(Duration::from_secs(5));
        t.extend(Duration::from_secs(5));
        assert!(t.is_paused());
        assert_eq!(t.total(), Duration::from_secs(20));
        assert_eq!(
            t.remaining(t0 + Duration::from_secs(100)),
            Duration::from_secs(16)
        );
    }

    #[test]
    fn extend_while_idle_adds_time_and_clears_on_reset_and_select() {
        let t0 = Instant::now();
        let mut t = Timer::new(quick());
        t.extend(Duration::from_secs(5));
        assert!(t.is_idle());
        assert_eq!(t.total(), Duration::from_secs(15));
        assert_eq!(t.remaining(t0), Duration::from_secs(15));

        t.reset();
        assert_eq!(t.total(), Duration::from_secs(10));

        t.start(t0);
        t.extend(Duration::from_secs(5));
        t.select_phase(Phase::Focus);
        assert!(t.is_idle());
        assert_eq!(t.total(), Duration::from_secs(10));

        t.start(t0);
        t.extend(Duration::from_secs(5));
        let f = t.skip(t0 + Duration::from_secs(12)).unwrap();
        assert!(!f.completed);
        assert_eq!(f.elapsed, Duration::from_secs(12));
        assert_eq!(t.phase(), Phase::ShortBreak);
        assert_eq!(t.total(), Duration::from_secs(2));
    }

    #[test]
    fn clock_format() {
        assert_eq!(format_clock(Duration::from_secs(25 * 60)), "25:00");
        assert_eq!(format_clock(Duration::from_millis(59_001)), "01:00");
        assert_eq!(format_clock(Duration::from_secs(3725)), "1:02:05");
        assert_eq!(format_clock(Duration::ZERO), "00:00");
    }
}
