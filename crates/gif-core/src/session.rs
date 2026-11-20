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
    StartClicked,
    Stop,
    Cancel,
}

pub fn step(state: SessionState, event: SessionEvent, control: RecordControl) -> SessionState {
    use SessionEvent::*;
    use SessionState::*;

    match (state, event) {
        (Idle, BeginSelection) => Selecting,
        (Selecting, RegionSelected { region }) => match control {
            RecordControl::Immediate => Recording { region },
            RecordControl::Button => Armed { region },
        },
        (Armed { region }, StartClicked) => Recording { region },
        (Recording { .. }, Stop) => Encoding,
        (Selecting | Armed { .. } | Recording { .. }, Cancel) => Idle,
        (unchanged, _) => unchanged,
    }
}

#[cfg(test)]
mod tests {
    use super::RecordControl::{Button, Immediate};
    use super::SessionEvent::{BeginSelection, Cancel, RegionSelected, StartClicked, Stop};
    use super::SessionState::{Armed, Encoding, Idle, Recording, Selecting};
    use super::*;

    fn region() -> ScreenRect {
        ScreenRect { monitor: MonitorId(1), x: 100, y: 50, width: 640, height: 360 }
    }

    #[test]
    fn beginning_a_selection_leaves_idle() {
        assert_eq!(step(Idle, BeginSelection, Immediate), Selecting);
    }

    #[test]
    fn immediate_mode_records_as_soon_as_the_drag_ends() {
        let region = region();
        assert_eq!(step(Selecting, RegionSelected { region }, Immediate), Recording { region });
    }

    #[test]
    fn button_mode_waits_for_the_start_button() {
        let region = region();
        assert_eq!(step(Selecting, RegionSelected { region }, Button), Armed { region });
        assert_eq!(step(Armed { region }, StartClicked, Button), Recording { region });
    }

    #[test]
    fn stopping_a_recording_starts_encoding() {
        let region = region();
        for control in [Immediate, Button] {
            assert_eq!(step(Recording { region }, Stop, control), Encoding);
        }
    }

    #[test]
    fn cancel_returns_to_idle_from_every_state_before_encoding() {
        let region = region();
        for state in [Selecting, Armed { region }, Recording { region }] {
            assert_eq!(step(state, Cancel, Button), Idle, "{state:?}");
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
            (Selecting, BeginSelection),
            (Selecting, StartClicked),
            (Selecting, Stop),
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
            assert_eq!(step(state, event, Button), state, "{state:?} + {event:?}");
        }
    }
}
