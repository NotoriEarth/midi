#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

pub fn gif_file_name(at: Timestamp, is_taken: impl Fn(&str) -> bool) -> String {
    let Timestamp { year, month, day, hour, minute, second } = at;
    let stem = format!("midi-{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}");

    let first = format!("{stem}.gif");
    if !is_taken(&first) {
        return first;
    }
    (2..).map(|number| format!("{stem}-{number}.gif")).find(|name| !is_taken(name)).expect("some number is free")
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVENING: Timestamp = Timestamp { year: 2026, month: 11, day: 22, hour: 19, minute: 4, second: 9 };

    #[test]
    fn the_name_is_the_time_the_recording_was_saved() {
        assert_eq!(gif_file_name(EVENING, |_| false), "midi-20261122-190409.gif");
    }

    #[test]
    fn single_digits_are_padded_so_names_sort_by_time() {
        let new_year = Timestamp { year: 2027, month: 1, day: 2, hour: 3, minute: 4, second: 5 };
        assert_eq!(gif_file_name(new_year, |_| false), "midi-20270102-030405.gif");
    }

    #[test]
    fn a_second_file_in_the_same_second_gets_a_number() {
        let taken = |name: &str| name == "midi-20261122-190409.gif";
        assert_eq!(gif_file_name(EVENING, taken), "midi-20261122-190409-2.gif");
    }

    #[test]
    fn the_number_goes_up_until_a_name_is_free() {
        let taken = |name: &str| !name.ends_with("-4.gif");
        assert_eq!(gif_file_name(EVENING, taken), "midi-20261122-190409-4.gif");
    }
}
