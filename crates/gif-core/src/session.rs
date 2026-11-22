#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub monitor: MonitorId,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordControl {
    Immediate,
    Button,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    Drag,
    Frame { initial: ScreenRect },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionConfig {
    pub selection: SelectionMode,
    pub control: RecordControl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Selecting,
    Armed { region: ScreenRect },
    Recording { region: ScreenRect },
    Encoding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
    BeginSelection,
    RegionSelected { region: ScreenRect },
    RegionChanged { region: ScreenRect },
    StartClicked,
    Stop,
    Cancel,
}

pub fn step(state: SessionState, event: SessionEvent, config: SessionConfig) -> SessionState {
    use SessionEvent::*;
    use SessionState::*;

    match (state, event) {
        (Idle, BeginSelection) => match config.selection {
            SelectionMode::Drag => Selecting,
            SelectionMode::Frame { initial } => Armed { region: initial },
        },
        (Selecting, RegionSelected { region }) => match config.control {
            RecordControl::Immediate => Recording { region },
            RecordControl::Button => Armed { region },
        },
        (Armed { .. }, RegionChanged { region }) => Armed { region },
        (Armed { region }, StartClicked) => Recording { region },
        (Recording { .. }, Stop) => Encoding,
        (Selecting | Armed { .. } | Recording { .. }, Cancel) => Idle,
        (unchanged, _) => unchanged,
    }
}

#[cfg(test)]
mod tests {
    use super::RecordControl::{Button, Immediate};
    use super::SessionEvent::{
        BeginSelection, Cancel, RegionChanged, RegionSelected, StartClicked, Stop,
    };
    use super::SessionState::{Armed, Encoding, Idle, Recording, Selecting};
    use super::*;

    fn region() -> ScreenRect {
        ScreenRect {
            monitor: MonitorId(1),
            x: 100,
            y: 50,
            width: 640,
            height: 360,
        }
    }

    fn resized() -> ScreenRect {
        ScreenRect {
            width: 800,
            height: 450,
            ..region()
        }
    }

    fn drag(control: RecordControl) -> SessionConfig {
        SessionConfig {
            selection: SelectionMode::Drag,
            control,
        }
    }

    fn frame(control: RecordControl) -> SessionConfig {
        SessionConfig {
            selection: SelectionMode::Frame { initial: region() },
            control,
        }
    }

    #[test]
    fn beginning_a_drag_selection_leaves_idle() {
        assert_eq!(step(Idle, BeginSelection, drag(Immediate)), Selecting);
    }

    #[test]
    fn immediate_mode_records_as_soon_as_the_drag_ends() {
        let region = region();
        assert_eq!(
            step(Selecting, RegionSelected { region }, drag(Immediate)),
            Recording { region }
        );
    }

    #[test]
    fn button_mode_waits_for_the_start_button() {
        let region = region();
        assert_eq!(
            step(Selecting, RegionSelected { region }, drag(Button)),
            Armed { region }
        );
        assert_eq!(
            step(Armed { region }, StartClicked, drag(Button)),
            Recording { region }
        );
    }

    #[test]
    fn frame_mode_skips_the_drag_and_shows_the_frame_where_it_was_left() {
        for control in [Immediate, Button] {
            assert_eq!(
                step(Idle, BeginSelection, frame(control)),
                Armed { region: region() }
            );
        }
    }

    #[test]
    fn the_frame_can_be_moved_and_resized_until_recording_starts() {
        let armed = step(
            Armed { region: region() },
            RegionChanged { region: resized() },
            frame(Button),
        );
        assert_eq!(armed, Armed { region: resized() });
        assert_eq!(
            step(armed, StartClicked, frame(Button)),
            Recording { region: resized() }
        );
    }

    #[test]
    fn the_region_is_locked_while_recording() {
        let recording = Recording { region: region() };
        assert_eq!(
            step(
                recording,
                RegionChanged { region: resized() },
                frame(Button)
            ),
            recording
        );
    }

    #[test]
    fn stopping_a_recording_starts_encoding() {
        let region = region();
        for config in [
            drag(Immediate),
            drag(Button),
            frame(Immediate),
            frame(Button),
        ] {
            assert_eq!(step(Recording { region }, Stop, config), Encoding);
        }
    }

    #[test]
    fn cancel_returns_to_idle_from_every_state_before_encoding() {
        let region = region();
        for state in [Selecting, Armed { region }, Recording { region }] {
            assert_eq!(step(state, Cancel, drag(Button)), Idle, "{state:?}");
        }
    }

    #[test]
    fn events_that_do_not_apply_leave_the_state_unchanged() {
        let region = region();
        let ignored = [
            (Idle, Stop),
            (Idle, Cancel),
            (Idle, StartClicked),
            (Idle, RegionSelected { region }),
            (Idle, RegionChanged { region }),
            (Selecting, BeginSelection),
            (Selecting, StartClicked),
            (Selecting, Stop),
            (Selecting, RegionChanged { region }),
            (Armed { region }, BeginSelection),
            (Armed { region }, Stop),
            (Armed { region }, RegionSelected { region }),
            (Recording { region }, BeginSelection),
            (Recording { region }, StartClicked),
            (Recording { region }, RegionSelected { region }),
            (Encoding, BeginSelection),
            (Encoding, Stop),
            (Encoding, Cancel),
        ];
        for (state, event) in ignored {
            assert_eq!(
                step(state, event, drag(Button)),
                state,
                "{state:?} + {event:?}"
            );
        }
    }
}
