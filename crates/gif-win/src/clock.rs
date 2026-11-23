use gif_core::output::Timestamp;
use windows::Win32::System::SystemInformation::GetLocalTime;

pub fn local_time() -> Timestamp {
    let now = unsafe { GetLocalTime() };
    Timestamp {
        year: now.wYear,
        month: now.wMonth as u8,
        day: now.wDay as u8,
        hour: now.wHour as u8,
        minute: now.wMinute as u8,
        second: now.wSecond as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_local_time_is_a_real_date_and_time() {
        let now = local_time();
        assert!(now.year >= 2026, "{now:?}");
        assert!((1..=12).contains(&now.month) && (1..=31).contains(&now.day), "{now:?}");
        assert!(now.hour < 24 && now.minute < 60 && now.second < 60, "{now:?}");
    }
}
