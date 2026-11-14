//! Fixed, event-driven effects drawn over a terminal surface.
//!
//! GPUI 0.3.8 exposes its renderer's shaders internally, but does not expose a
//! public application-level offscreen texture/shader-pass API. This module is
//! the seam for that future pass: event mapping, timing, and redraw scheduling
//! stay independent from the current GPUI quad painter.

use gpui::{Bounds, Hsla, Pixels, Window, fill, point, px, size};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalEffectEvent {
    Focus,
    CommandStart,
    CommandFinish,
    Blocked,
    ExitCode(Option<i32>),
    TabSwitch,
}

impl TerminalEffectEvent {
    fn duration(self) -> Duration {
        match self {
            Self::Focus => Duration::from_millis(220),
            Self::CommandStart => Duration::from_millis(900),
            Self::CommandFinish => Duration::from_millis(520),
            Self::Blocked => Duration::from_millis(1_200),
            Self::ExitCode(_) => Duration::from_millis(700),
            Self::TabSwitch => Duration::from_millis(280),
        }
    }

    fn color(self) -> Hsla {
        match self {
            Self::Focus | Self::TabSwitch => crate::settings::color("accent"),
            Self::CommandStart => crate::settings::color("working"),
            Self::CommandFinish => crate::settings::color("done"),
            Self::Blocked => crate::settings::color("blocked"),
            Self::ExitCode(Some(code)) if code == 0 => crate::settings::color("done"),
            Self::ExitCode(Some(_)) => crate::settings::color("blocked"),
            Self::ExitCode(None) => crate::settings::color("accent"),
        }
    }
}

/// The event produced by an OMP/7501 status transition.
pub(crate) fn agent_event(prev: Option<&str>, new: Option<&str>) -> Option<TerminalEffectEvent> {
    let was_busy = matches!(prev, Some("working" | "blocked" | "error"));
    match new {
        Some("working") if !matches!(prev, Some("working")) => {
            Some(TerminalEffectEvent::CommandStart)
        }
        Some("blocked" | "error") if !matches!(prev, Some("blocked" | "error")) => {
            Some(TerminalEffectEvent::Blocked)
        }
        Some(_) if was_busy && !matches!(new, Some("working" | "blocked" | "error")) => {
            Some(TerminalEffectEvent::CommandFinish)
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug)]
struct Pulse {
    event: TerminalEffectEvent,
    started: Instant,
}

#[derive(Default)]
pub(crate) struct TerminalEffect {
    pulse: Option<Pulse>,
}

impl TerminalEffect {
    pub(crate) fn trigger(&mut self, event: TerminalEffectEvent) {
        self.pulse = Some(Pulse {
            event,
            started: Instant::now(),
        });
    }

    /// Paint one deterministic effect pass. Returns true while another frame is needed.
    pub(crate) fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window) -> bool {
        let Some(pulse) = self.pulse else {
            return false;
        };
        let duration = pulse.event.duration();
        let elapsed = pulse.started.elapsed();
        if elapsed >= duration {
            self.pulse = None;
            return false;
        }
        let progress = (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0);
        let fade = (1.0 - progress).powi(2);
        let color = pulse.event.color();
        self.paint_pass(bounds, pulse.event, progress, fade, color, window);
        true
    }

    fn paint_pass(
        &self,
        bounds: Bounds<Pixels>,
        event: TerminalEffectEvent,
        progress: f32,
        fade: f32,
        color: Hsla,
        window: &mut Window,
    ) {
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        if width <= 0.0 || height <= 0.0 {
            return;
        }

        let rect = |x: f32, y: f32, w: f32, h: f32, opacity: f32| {
            let x = x.clamp(0.0, width);
            let y = y.clamp(0.0, height);
            let right = (x + w).clamp(x, width);
            let bottom = (y + h).clamp(y, height);
            if right > x && bottom > y {
                window.paint_quad(fill(
                    Bounds::new(
                        bounds.origin + point(px(x), px(y)),
                        size(px(right - x), px(bottom - y)),
                    ),
                    color.opacity(opacity),
                ));
            }
        };

        match event {
            TerminalEffectEvent::Focus => {
                rect(0.0, 0.0, width, 2.0, 0.32 * fade);
                rect(0.0, 0.0, 2.0, height, 0.12 * fade);
            }
            TerminalEffectEvent::CommandStart => {
                let sweep = (width * 0.22).clamp(32.0, width);
                let x = width * (progress * 1.25 - 0.25);
                rect(x, 0.0, sweep, 2.0, 0.38 * fade);
                rect(x - sweep * 0.65, 2.0, sweep * 0.65, 1.0, 0.14 * fade);
            }
            TerminalEffectEvent::CommandFinish => {
                let pulse = (std::f32::consts::PI * progress).sin();
                rect(0.0, height - 2.0, width, 2.0, 0.36 * pulse);
                rect(width * 0.08, height - 4.0, width * 0.84, 1.0, 0.12 * pulse);
            }
            TerminalEffectEvent::Blocked => {
                let pulse = (std::f32::consts::PI * progress).sin();
                rect(0.0, 0.0, 3.0, height, 0.42 * pulse);
                rect(0.0, 0.0, width, 2.0, 0.22 * pulse);
                rect(0.0, height - 2.0, width, 2.0, 0.22 * pulse);
            }
            TerminalEffectEvent::ExitCode(_) => {
                let pulse = (std::f32::consts::PI * progress).sin();
                rect(0.0, height - 2.0, width, 2.0, 0.44 * pulse);
                rect(width * 0.28, height - 4.0, width * 0.44, 1.0, 0.16 * pulse);
            }
            TerminalEffectEvent::TabSwitch => {
                let inset = (progress * 3.0).min(3.0);
                rect(inset, inset, width - inset * 2.0, 1.0, 0.24 * fade);
                rect(inset, inset, 1.0, height - inset * 2.0, 0.12 * fade);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TerminalEffectEvent, agent_event};

    #[test]
    fn agent_status_transitions_map_to_effect_events() {
        assert_eq!(
            agent_event(None, Some("working")),
            Some(TerminalEffectEvent::CommandStart)
        );
        assert_eq!(agent_event(Some("working"), Some("working")), None);
        assert_eq!(
            agent_event(Some("working"), Some("blocked")),
            Some(TerminalEffectEvent::Blocked)
        );
        assert_eq!(
            agent_event(Some("working"), Some("idle")),
            Some(TerminalEffectEvent::CommandFinish)
        );
        assert_eq!(
            agent_event(Some("blocked"), Some("done")),
            Some(TerminalEffectEvent::CommandFinish)
        );
        assert_eq!(
            agent_event(Some("error"), Some("idle")),
            Some(TerminalEffectEvent::CommandFinish)
        );
    }

    #[test]
    fn effect_durations_are_short_and_finite() {
        for event in [
            TerminalEffectEvent::Focus,
            TerminalEffectEvent::CommandStart,
            TerminalEffectEvent::CommandFinish,
            TerminalEffectEvent::Blocked,
            TerminalEffectEvent::ExitCode(Some(1)),
            TerminalEffectEvent::TabSwitch,
        ] {
            assert!(event.duration().as_millis() <= 1_200);
            assert!(!event.duration().is_zero());
        }
    }
}
