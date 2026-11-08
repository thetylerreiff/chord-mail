//! Turns what people type into a snooze box ("2h", "tom 9am", "fri",
//! "jan 15 3pm") into a moment, and offers the usual presets.

use chrono::{
    Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Weekday,
};

/// Mornings start here when only a day is given.
const MORNING: u32 = 8;

/// Parses `input` relative to now, in local time. Returns milliseconds since
/// the epoch, or `None` when the input isn't understood or is in the past.
pub fn parse(input: &str) -> Option<i64> {
    let now = Local::now().naive_local();
    to_millis(parse_at(input, now)?)
}

/// The presets the snooze picker offers, as (label, millis).
pub fn presets() -> Vec<(String, i64)> {
    let now = Local::now().naive_local();
    presets_at(now)
        .into_iter()
        .filter_map(|(label, at)| Some((label.to_string(), to_millis(at)?)))
        .collect()
}

fn to_millis(at: NaiveDateTime) -> Option<i64> {
    Local
        .from_local_datetime(&at)
        .earliest()
        .map(|dt| dt.timestamp_millis())
}

fn at_hour(date: NaiveDate, hour: u32) -> NaiveDateTime {
    date.and_time(NaiveTime::from_hms_opt(hour, 0, 0).unwrap())
}

fn next_weekday(from: NaiveDate, day: Weekday) -> NaiveDate {
    let mut d = from + Duration::days(1);
    while d.weekday() != day {
        d += Duration::days(1);
    }
    d
}

pub(crate) fn presets_at(now: NaiveDateTime) -> Vec<(&'static str, NaiveDateTime)> {
    let today = now.date();
    let mut out = Vec::new();
    // "Later today" only makes sense with some day left.
    let later = now + Duration::hours(3);
    if later.date() == today && later.hour() <= 21 {
        // Round up to the next whole hour.
        out.push((
            "Later today",
            at_hour(today, later.hour()) + Duration::hours(1),
        ));
    }
    out.push(("Tomorrow", at_hour(today + Duration::days(1), MORNING)));
    let weekend = match today.weekday() {
        Weekday::Sat | Weekday::Sun => None,
        _ => Some(at_hour(next_weekday(today, Weekday::Sat), MORNING)),
    };
    if let Some(w) = weekend {
        out.push(("This weekend", w));
    }
    out.push((
        "Next week",
        at_hour(next_weekday(today, Weekday::Mon), MORNING),
    ));
    out.push(("In a month", at_hour(add_months(today, 1), MORNING)));
    out
}

fn add_months(date: NaiveDate, months: u32) -> NaiveDate {
    let (mut y, mut m) = (date.year(), date.month() + months);
    while m > 12 {
        m -= 12;
        y += 1;
    }
    let mut day = date.day();
    loop {
        if let Some(d) = NaiveDate::from_ymd_opt(y, m, day) {
            return d;
        }
        day -= 1;
    }
}

fn weekday(word: &str) -> Option<Weekday> {
    let w = word.trim_end_matches('.');
    let days = [
        ("mon", Weekday::Mon),
        ("tue", Weekday::Tue),
        ("wed", Weekday::Wed),
        ("thu", Weekday::Thu),
        ("fri", Weekday::Fri),
        ("sat", Weekday::Sat),
        ("sun", Weekday::Sun),
    ];
    if w.len() < 3 {
        return None;
    }
    days.iter()
        .find(|(prefix, _)| w.starts_with(prefix) && full_day_name(prefix).starts_with(w))
        .map(|(_, d)| *d)
}

fn full_day_name(prefix: &str) -> &'static str {
    match prefix {
        "mon" => "monday",
        "tue" => "tuesday",
        "wed" => "wednesday",
        "thu" => "thursday",
        "fri" => "friday",
        "sat" => "saturday",
        _ => "sunday",
    }
}

fn month(word: &str) -> Option<u32> {
    let names = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    if word.len() < 3 {
        return None;
    }
    names
        .iter()
        .position(|n| n.starts_with(word))
        .map(|i| i as u32 + 1)
}

/// "9", "9am", "9:30pm", "17:00", "noon". A bare hour from 1 to 7 means
/// afternoon, because nobody snoozes mail until 3 in the morning.
fn time_of_day(word: &str) -> Option<NaiveTime> {
    match word {
        "noon" => return NaiveTime::from_hms_opt(12, 0, 0),
        "midnight" => return NaiveTime::from_hms_opt(0, 0, 0),
        "morning" => return NaiveTime::from_hms_opt(MORNING, 0, 0),
        "afternoon" => return NaiveTime::from_hms_opt(14, 0, 0),
        "evening" | "tonight" => return NaiveTime::from_hms_opt(19, 0, 0),
        _ => {}
    }
    let (digits, suffix) = match word.find(|c: char| c.is_ascii_alphabetic()) {
        Some(i) => (&word[..i], &word[i..]),
        None => (word, ""),
    };
    let (h, m) = match digits.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => (digits.parse::<u32>().ok()?, 0),
    };
    if m > 59 {
        return None;
    }
    let h = match suffix {
        "am" | "a" => (h % 12, true),
        "pm" | "p" => (h % 12 + 12, true),
        "" => (h, false),
        _ => return None,
    };
    let hour = match h {
        (h, true) => h,
        (h @ 1..=7, false) if !digits.contains(':') => h + 12,
        (h, false) => h,
    };
    NaiveTime::from_hms_opt(hour, m, 0)
}

pub(crate) fn parse_at(input: &str, now: NaiveDateTime) -> Option<NaiveDateTime> {
    let input = input.trim().to_lowercase();
    let words: Vec<&str> = input
        .split_whitespace()
        .filter(|w| *w != "at" && *w != "on")
        .collect();
    if words.is_empty() {
        return None;
    }
    let today = now.date();

    // Relative: "2h", "3 days", "30m", "1w".
    let joined = words.join("");
    if let Some(i) = joined.find(|c: char| !c.is_ascii_digit())
        && i > 0
    {
        let n: i64 = joined[..i].parse().ok()?;
        let unit = &joined[i..];
        let delta = match unit {
            "m" | "min" | "mins" | "minute" | "minutes" => Some(Duration::minutes(n)),
            "h" | "hr" | "hrs" | "hour" | "hours" => Some(Duration::hours(n)),
            "d" | "day" | "days" => Some(Duration::days(n)),
            "w" | "wk" | "wks" | "week" | "weeks" => Some(Duration::weeks(n)),
            _ => None,
        };
        if let Some(d) = delta {
            return Some(now + d);
        }
    }

    let (day_words, time) = match words.last().and_then(|w| time_of_day(w)) {
        Some(t) if words.len() > 1 => (&words[..words.len() - 1], Some(t)),
        Some(t) => (&words[..0], Some(t)),
        None => (&words[..], None),
    };
    let morning = NaiveTime::from_hms_opt(MORNING, 0, 0).unwrap();

    let date = match day_words {
        [] => {
            // A time alone means the next time the clock shows it.
            let t = time?;
            let candidate = today.and_time(t);
            return Some(if candidate > now {
                candidate
            } else {
                candidate + Duration::days(1)
            });
        }
        ["today"] => today,
        ["tonight"] => {
            return Some(today.and_time(time.unwrap_or(NaiveTime::from_hms_opt(19, 0, 0)?)));
        }
        ["tomorrow" | "tom" | "tmrw" | "tmr"] => today + Duration::days(1),
        ["next", "week"] => next_weekday(today, Weekday::Mon),
        ["weekend"] | ["this", "weekend"] => next_weekday(today, Weekday::Sat),
        ["next", "month"] => add_months(today, 1),
        ["next", w] if weekday(w).is_some() => next_weekday(today + Duration::days(6), weekday(w)?),
        [w] if weekday(w).is_some() => next_weekday(today, weekday(w)?),
        [m, d] if month(m).is_some() => {
            month_day(today, month(m)?, d.trim_end_matches(',').parse().ok()?)?
        }
        [d, m] if month(m).is_some() => month_day(today, month(m)?, d.parse().ok()?)?,
        [md] if md.contains('/') => {
            let (m, d) = md.split_once('/')?;
            month_day(today, m.parse().ok()?, d.parse().ok()?)?
        }
        _ => return None,
    };
    let at = date.and_time(time.unwrap_or(morning));
    (at > now).then_some(at)
}

/// The next occurrence of a month and day, this year or next.
fn month_day(today: NaiveDate, month: u32, day: u32) -> Option<NaiveDate> {
    let this_year = NaiveDate::from_ymd_opt(today.year(), month, day)?;
    if this_year > today {
        Some(this_year)
    } else {
        NaiveDate::from_ymd_opt(today.year() + 1, month, day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wednesday, November 4, 2026, 10:30.
    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, 4)
            .unwrap()
            .and_hms_opt(10, 30, 0)
            .unwrap()
    }

    fn at(input: &str) -> String {
        parse_at(input, now())
            .map(|d| d.format("%a %b %-d %H:%M").to_string())
            .unwrap_or_default()
    }

    #[test]
    fn relative_times() {
        assert_eq!(at("2h"), "Wed Nov 4 12:30");
        assert_eq!(at("30 min"), "Wed Nov 4 11:00");
        assert_eq!(at("3d"), "Sat Nov 7 10:30");
        assert_eq!(at("1 week"), "Wed Nov 11 10:30");
    }

    #[test]
    fn days_default_to_morning() {
        assert_eq!(at("tomorrow"), "Thu Nov 5 08:00");
        assert_eq!(at("tom 9am"), "Thu Nov 5 09:00");
        assert_eq!(at("fri"), "Fri Nov 6 08:00");
        assert_eq!(at("friday at 5pm"), "Fri Nov 6 17:00");
        assert_eq!(
            at("wed"),
            "Wed Nov 11 08:00",
            "today's weekday means next week"
        );
        assert_eq!(at("next week"), "Mon Nov 9 08:00");
        assert_eq!(at("weekend"), "Sat Nov 7 08:00");
    }

    #[test]
    fn times_alone_mean_the_next_one() {
        assert_eq!(at("3pm"), "Wed Nov 4 15:00");
        assert_eq!(at("9am"), "Thu Nov 5 09:00");
        assert_eq!(at("4"), "Wed Nov 4 16:00", "a bare small hour is afternoon");
        assert_eq!(at("17:45"), "Wed Nov 4 17:45");
        assert_eq!(at("noon"), "Wed Nov 4 12:00");
        assert_eq!(at("tonight"), "Wed Nov 4 19:00");
    }

    #[test]
    fn dates_roll_into_next_year() {
        assert_eq!(at("dec 25"), "Fri Dec 25 08:00");
        assert_eq!(at("jan 15 3pm"), "Fri Jan 15 15:00");
        assert_eq!(at("1/15"), "Fri Jan 15 08:00");
        assert_eq!(at("15 march"), "Mon Mar 15 08:00");
    }

    #[test]
    fn nonsense_and_the_past_are_rejected() {
        assert_eq!(at(""), "");
        assert_eq!(at("whenever"), "");
        assert_eq!(at("today 9am"), "", "already past");
        assert_eq!(at("25:00"), "");
    }

    #[test]
    fn presets_skip_what_doesnt_apply() {
        let labels: Vec<_> = presets_at(now()).into_iter().map(|(l, _)| l).collect();
        assert_eq!(
            labels,
            [
                "Later today",
                "Tomorrow",
                "This weekend",
                "Next week",
                "In a month"
            ]
        );
        let late = NaiveDate::from_ymd_opt(2026, 11, 7)
            .unwrap()
            .and_hms_opt(20, 0, 0)
            .unwrap();
        let labels: Vec<_> = presets_at(late).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels, ["Tomorrow", "Next week", "In a month"]);
    }
}
