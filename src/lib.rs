//! Turns a home thermometer's CSV export into an hour-by-hour check against
//! New York City's heat law (NYC Admin. Code §27-2029):
//!
//! - Heat season is October 1 to May 31.
//! - 6am to 10pm: at least 68°F inside whenever it is below 55°F outside.
//! - 10pm to 6am: at least 62°F inside, whatever the outdoor temperature.
//!
//! All parsing and checking happens here, in plain Rust; `web/` is a thin page
//! around [`check_csv`] and [`date_range`].

use std::collections::BTreeMap;
use wasm_bindgen::prelude::*;

/// One clock hour, in the logger's local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hour {
    pub y: i32,
    pub m: u32,
    pub d: u32,
    pub h: u32,
}

/// Hourly mean temperatures in °F, plus anything the reader should know.
#[derive(Debug, Default)]
pub struct Series {
    pub hours: BTreeMap<Hour, f64>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    /// Below the legal minimum.
    Violation,
    /// Daytime and below 68°F, but no outdoor temperature for that hour.
    Possible,
    OutOfSeason,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Violation => "violation",
            Status::Possible => "possible (no outdoor temperature)",
            Status::OutOfSeason => "outside heat season",
        }
    }
}

/// The legal minimum for an hour (if any), and whether the reading breaks it.
pub fn check(hour: Hour, indoor: f64, outdoor: Option<f64>) -> (Status, Option<f64>) {
    if !(hour.m >= 10 || hour.m <= 5) {
        return (Status::OutOfSeason, None);
    }
    let required = if (6..22).contains(&hour.h) {
        match outdoor {
            Some(o) if o < 55.0 => Some(68.0),
            Some(_) => None,
            None => {
                return (
                    if indoor < 68.0 {
                        Status::Possible
                    } else {
                        Status::Ok
                    },
                    None,
                )
            }
        }
    } else {
        Some(62.0)
    };
    match required {
        Some(r) if indoor < r => (Status::Violation, required),
        _ => (Status::Ok, required),
    }
}

// ---------- CSV reading ----------

fn split_line(line: &str, sep: char) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => out.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).collect()
}

/// Date and time as (y, m, d, h). Accepts `2026-11-10 14:05[:00]`, `2026-11-10T14:05`,
/// `2026/11/10 14:05`, `11/10/2026 2:05[:00] PM`, and a date alone (midnight).
/// Day-first dates (`03.11.2026`) are refused, since `03/11` can't be told apart.
pub fn parse_time(s: &str) -> Option<(i32, u32, u32, u32)> {
    let s = s.trim().trim_matches('"');
    let (date, rest) = match s.find(['T', ' ']) {
        Some(i) => (&s[..i], s[i + 1..].trim()),
        None => (s, ""),
    };
    let parts: Vec<&str> = date.split(['-', '/', '.']).collect();
    if parts.len() != 3 {
        return None;
    }
    let n = |p: &str| p.parse::<u32>().ok();
    let (y, m, d) = if parts[0].len() == 4 {
        (n(parts[0])?, n(parts[1])?, n(parts[2])?)
    } else if parts[2].len() == 4 && !date.contains('.') {
        // US order, MM/DD/YYYY. DD.MM.YYYY is refused rather than misread.
        (n(parts[2])?, n(parts[0])?, n(parts[1])?)
    } else {
        return None;
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut h = 0;
    if !rest.is_empty() {
        let upper = rest.to_ascii_uppercase();
        h = n(upper.split(':').next()?.trim())?;
        if upper.ends_with("PM") && h < 12 {
            h += 12;
        } else if upper.ends_with("AM") && h == 12 {
            h = 0;
        }
        if h > 23 {
            return None;
        }
    }
    Some((y as i32, m, d, h))
}

fn parse_number(s: &str, decimal_comma: bool) -> Option<f64> {
    let s: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | ','))
        .collect();
    let s = if decimal_comma {
        s.replace(',', ".")
    } else {
        s
    };
    s.parse().ok()
}

enum Unit {
    F,
    C,
    Unknown,
}

fn unit_of(header: &str) -> Unit {
    let h = header.to_lowercase();
    if h.contains("°c")
        || h.contains("(c)")
        || h.contains("[c]")
        || h.contains("celsius")
        || h.ends_with("_c")
        || h.ends_with(" c")
    {
        Unit::C
    } else if h.contains("°f")
        || h.contains("(f)")
        || h.contains("[f]")
        || h.contains("fahrenheit")
        || h.ends_with("_f")
        || h.ends_with(" f")
    {
        Unit::F
    } else {
        Unit::Unknown
    }
}

/// Reads a temperature log: finds the time column(s) and the temperature column by
/// their headers, and averages readings into clock hours, in °F. When the header
/// has no unit, `guess_unit` decides between failing and guessing from the values
/// (fine indoors, where °C and °F ranges don't overlap; never outdoors).
pub fn parse_series(text: &str, guess_unit: bool) -> Result<Series, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header_line = lines.next().ok_or("the file is empty")?;
    let sep = [',', ';', '\t']
        .into_iter()
        .max_by_key(|s| header_line.matches(*s).count())
        .unwrap_or(',');
    let header = split_line(header_line, sep);
    let lower: Vec<String> = header.iter().map(|h| h.to_lowercase()).collect();
    let find = |pred: &dyn Fn(&str) -> bool| lower.iter().position(|h| pred(h));
    let temp = find(&|h| {
        (h.contains("temp") || h.contains("°f") || h.contains("°c"))
            && !["dew", "index", "feel", "probe 2"]
                .iter()
                .any(|x| h.contains(x))
    });
    let stamp = find(&|h| h.contains("timestamp") || (h.contains("date") && h.contains("time")))
        .or_else(|| find(&|h| h.contains("time") || h.contains("date")));
    let (Some(temp), Some(stamp)) = (temp, stamp) else {
        return Err(format!(
            "could not find a time column and a temperature column in this file. Its columns are: {}",
            header.join(" | ")
        ));
    };
    // "Date" and "Time" in separate columns.
    let time_col = (lower[stamp] == "date")
        .then(|| find(&|h| h == "time"))
        .flatten();

    // Hours are compared in local time; UTC or offset times would shift every hour.
    let utc = |t: &str| {
        let t = t.trim().trim_matches('"');
        t.ends_with('Z') || t.len() > 16 && t[16..].contains(['+', '-'])
    };
    if let Some(first) = text.lines().filter(|l| !l.trim().is_empty()).nth(1) {
        if split_line(first, sep).get(stamp).is_some_and(|t| utc(t)) {
            return Err(format!(
                "the times in '{}' are in UTC or carry a time-zone offset; export the log in local time",
                header[stamp]
            ));
        }
    }
    let mut sums: BTreeMap<Hour, (f64, u32)> = BTreeMap::new();
    let mut bad = 0;
    for line in lines {
        let row = split_line(line, sep);
        let when = match time_col {
            Some(t) => format!(
                "{} {}",
                row.get(stamp).map_or("", |s| s),
                row.get(t).map_or("", |s| s)
            ),
            None => row.get(stamp).cloned().unwrap_or_default(),
        };
        let parsed = parse_time(&when).zip(row.get(temp).and_then(|v| parse_number(v, sep == ';')));
        match parsed {
            Some(((y, m, d, h), v)) => {
                let e = sums.entry(Hour { y, m, d, h }).or_insert((0.0, 0));
                e.0 += v;
                e.1 += 1;
            }
            None => bad += 1,
        }
    }
    if sums.is_empty() {
        return Err(format!(
            "found the columns '{}' and '{}', but no rows with a readable date and temperature",
            header[stamp], header[temp]
        ));
    }
    let mut s = Series::default();
    let mut hours: BTreeMap<Hour, f64> = sums
        .into_iter()
        .map(|(k, (sum, n))| (k, sum / n as f64))
        .collect();
    let celsius = match unit_of(&header[temp]) {
        Unit::C => true,
        Unit::F => false,
        Unit::Unknown if guess_unit => {
            let mut v: Vec<f64> = hours.values().copied().collect();
            v.sort_by(f64::total_cmp);
            let c = v[v.len() / 2] < 40.0;
            s.warnings.push(format!(
                "the temperature column '{}' does not say °F or °C; the values look like {}",
                header[temp],
                if c { "°C" } else { "°F" }
            ));
            c
        }
        Unit::Unknown => {
            return Err(format!(
                "the temperature column '{}' does not say °F or °C; add the unit to its header, e.g. 'Temperature (F)'",
                header[temp]
            ))
        }
    };
    if celsius {
        hours.values_mut().for_each(|v| *v = *v * 9.0 / 5.0 + 32.0);
    }
    if bad > 0 {
        s.warnings.push(format!(
            "{bad} row(s) had no readable date or temperature and were skipped"
        ));
    }
    s.hours = hours;
    Ok(s)
}

// ---------- report ----------

pub struct Row {
    pub hour: Hour,
    pub indoor: f64,
    pub outdoor: Option<f64>,
    pub required: Option<f64>,
    pub status: Status,
}

pub fn rows(indoor: &Series, outdoor: Option<&Series>) -> Vec<Row> {
    indoor
        .hours
        .iter()
        .map(|(&hour, &t)| {
            let o = outdoor.and_then(|s| s.hours.get(&hour).copied());
            let (status, required) = check(hour, t, o);
            Row {
                hour,
                indoor: t,
                outdoor: o,
                required,
                status,
            }
        })
        .collect()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn date(h: Hour) -> String {
    format!("{:04}-{:02}-{:02}", h.y, h.m, h.d)
}

fn ampm(h: u32) -> String {
    match h % 24 {
        0 => "12am".into(),
        12 => "12pm".into(),
        h if h < 12 => format!("{h}am"),
        h => format!("{}pm", h - 12),
    }
}

/// "6am–9am, 11pm–12am" for the hours 6, 7, 8 and 23 (each range ends where the hour ends).
fn ranges(hours: &[u32]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < hours.len() {
        let mut j = i;
        while j + 1 < hours.len() && hours[j + 1] == hours[j] + 1 {
            j += 1;
        }
        out.push(format!("{}–{}", ampm(hours[i]), ampm(hours[j] + 1)));
        i = j + 1;
    }
    out.join(", ")
}

/// Printable HTML: summary, then one line per day with a problem hour.
pub fn report_html(rows: &[Row], warnings: &[String], have_outdoor: bool) -> String {
    let count = |s: Status| rows.iter().filter(|r| r.status == s).count();
    let (first, last) = (rows.first().map(|r| r.hour), rows.last().map(|r| r.hour));
    let mut out = String::new();
    out.push_str("<h2>Summary</h2><table class=\"summary\">");
    if let (Some(a), Some(b)) = (first, last) {
        out.push_str(&format!(
            "<tr><th>Period</th><td>{} {} to {} {}</td></tr>",
            date(a),
            ampm(a.h),
            date(b),
            ampm(b.h)
        ));
    }
    let violations = count(Status::Violation);
    let possible = count(Status::Possible);
    let mut by_day: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for r in rows
        .iter()
        .filter(|r| matches!(r.status, Status::Violation | Status::Possible))
    {
        by_day.entry(date(r.hour)).or_default().push(r);
    }
    for (k, v) in [
        ("Hours with a reading", rows.len().to_string()),
        ("Hours below the legal minimum", violations.to_string()),
        (
            "Days with at least one such hour",
            by_day
                .values()
                .filter(|v| v.iter().any(|r| r.status == Status::Violation))
                .count()
                .to_string(),
        ),
    ] {
        out.push_str(&format!("<tr><th>{k}</th><td>{v}</td></tr>"));
    }
    if possible > 0 {
        out.push_str(&format!("<tr><th>Daytime hours below 68°F with no outdoor temperature</th><td>{possible}</td></tr>"));
    }
    if let Some(low) = rows
        .iter()
        .filter(|r| r.status == Status::Violation)
        .map(|r| r.indoor)
        .min_by(f64::total_cmp)
    {
        out.push_str(&format!(
            "<tr><th>Lowest indoor temperature in a violation hour</th><td>{low:.1}°F</td></tr>"
        ));
    }
    out.push_str("</table>");
    if !have_outdoor {
        out.push_str("<p class=\"note\">No outdoor temperatures were given, so daytime hours can only be marked \"possible\". Night hours (10pm–6am) need no outdoor temperature.</p>");
    }
    for w in warnings {
        out.push_str(&format!("<p class=\"note\">Note: {}</p>", esc(w)));
    }
    if out_of_season(rows) {
        out.push_str("<p class=\"note\">Some readings fall outside heat season (October 1 to May 31) and are not checked.</p>");
    }
    out.push_str("<h2>Days with problem hours</h2>");
    if by_day.is_empty() {
        out.push_str("<p>No hour was below the legal minimum.</p>");
        return out;
    }
    out.push_str("<table class=\"days\"><thead><tr><th>Date</th><th>Hours below the minimum</th><th>Lowest inside</th><th>Outside then</th></tr></thead><tbody>");
    for (day, rs) in &by_day {
        let v: Vec<&&Row> = rs
            .iter()
            .filter(|r| r.status == Status::Violation)
            .collect();
        let p: Vec<&&Row> = rs.iter().filter(|r| r.status == Status::Possible).collect();
        let mut hours = ranges(&v.iter().map(|r| r.hour.h).collect::<Vec<_>>());
        if !p.is_empty() {
            let ps = ranges(&p.iter().map(|r| r.hour.h).collect::<Vec<_>>());
            hours = if hours.is_empty() {
                format!("possible: {ps}")
            } else {
                format!("{hours}; possible: {ps}")
            };
        }
        let low = rs
            .iter()
            .min_by(|a, b| a.indoor.total_cmp(&b.indoor))
            .unwrap();
        let outside = low.outdoor.map_or("–".to_string(), |o| format!("{o:.0}°F"));
        out.push_str(&format!(
            "<tr><td>{day}</td><td>{hours}</td><td>{:.1}°F at {}</td><td>{outside}</td></tr>",
            low.indoor,
            ampm(low.hour.h)
        ));
    }
    out.push_str("</tbody></table>");
    out
}

fn out_of_season(rows: &[Row]) -> bool {
    rows.iter().any(|r| r.status == Status::OutOfSeason)
}

/// Every hour, for a spreadsheet or a lawyer.
pub fn report_csv(rows: &[Row]) -> String {
    let mut out = String::from("date,hour,indoor_f,outdoor_f,legal_minimum_f,status\n");
    for r in rows {
        let opt = |v: Option<f64>| v.map_or(String::new(), |v| format!("{v:.1}"));
        out.push_str(&format!(
            "{},{:02}:00,{:.1},{},{},{}\n",
            date(r.hour),
            r.hour.h,
            r.indoor,
            opt(r.outdoor),
            opt(r.required),
            r.status.label()
        ));
    }
    out
}

// ---------- browser API ----------

/// First and last date in an indoor log, as "YYYY-MM-DD YYYY-MM-DD" (to fetch outdoor data for).
#[wasm_bindgen]
pub fn date_range(indoor_csv: &str) -> Result<String, String> {
    let s = parse_series(indoor_csv, true)?;
    let (a, b) = (
        s.hours.keys().next().unwrap(),
        s.hours.keys().last().unwrap(),
    );
    Ok(format!("{} {}", date(*a), date(*b)))
}

#[wasm_bindgen]
pub struct Checked {
    html: String,
    csv: String,
}

#[wasm_bindgen]
impl Checked {
    #[wasm_bindgen(getter)]
    pub fn html(&self) -> String {
        self.html.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn csv(&self) -> String {
        self.csv.clone()
    }
}

/// Checks an indoor log against the law. `outdoor_csv` may be empty.
#[wasm_bindgen]
pub fn check_csv(indoor_csv: &str, outdoor_csv: &str) -> Result<Checked, String> {
    let indoor = parse_series(indoor_csv, true).map_err(|e| format!("Indoor file: {e}"))?;
    let outdoor = if outdoor_csv.trim().is_empty() {
        None
    } else {
        Some(parse_series(outdoor_csv, false).map_err(|e| format!("Outdoor file: {e}"))?)
    };
    let mut warnings = indoor.warnings.clone();
    if let Some(o) = &outdoor {
        warnings.extend(o.warnings.iter().map(|w| format!("outdoor file: {w}")));
    }
    let rows = rows(&indoor, outdoor.as_ref());
    Ok(Checked {
        html: report_html(&rows, &warnings, outdoor.is_some()),
        csv: report_csv(&rows),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hr(m: u32, d: u32, h: u32) -> Hour {
        Hour { y: 2026, m, d, h }
    }

    #[test]
    fn law() {
        // Day, cold outside: 68 needed.
        assert_eq!(
            check(hr(11, 3, 9), 67.9, Some(40.0)),
            (Status::Violation, Some(68.0))
        );
        assert_eq!(
            check(hr(11, 3, 9), 68.0, Some(40.0)),
            (Status::Ok, Some(68.0))
        );
        // Day, 55 or warmer outside: no minimum.
        assert_eq!(check(hr(11, 3, 9), 60.0, Some(55.0)), (Status::Ok, None));
        // Day boundaries: 6am is day, 10pm is night.
        assert_eq!(check(hr(11, 3, 6), 65.0, Some(40.0)).0, Status::Violation);
        assert_eq!(check(hr(11, 3, 5), 65.0, Some(40.0)).0, Status::Ok);
        assert_eq!(check(hr(11, 3, 22), 63.0, Some(40.0)).0, Status::Ok);
        assert_eq!(check(hr(11, 3, 21), 63.0, Some(40.0)).0, Status::Violation);
        // Night: 62 whatever the outdoor temperature, known or not.
        assert_eq!(
            check(hr(11, 3, 2), 61.0, Some(70.0)),
            (Status::Violation, Some(62.0))
        );
        assert_eq!(check(hr(11, 3, 2), 61.0, None).0, Status::Violation);
        // Day, outdoor unknown.
        assert_eq!(check(hr(11, 3, 12), 66.0, None).0, Status::Possible);
        assert_eq!(check(hr(11, 3, 12), 70.0, None).0, Status::Ok);
        // Season: October 1 to May 31.
        assert_eq!(check(hr(6, 1, 2), 50.0, None).0, Status::OutOfSeason);
        assert_eq!(check(hr(9, 30, 2), 50.0, None).0, Status::OutOfSeason);
        assert_eq!(check(hr(10, 1, 2), 50.0, None).0, Status::Violation);
        assert_eq!(check(hr(5, 31, 2), 50.0, None).0, Status::Violation);
    }

    #[test]
    fn times() {
        assert_eq!(parse_time("2026-11-10 14:05:00"), Some((2026, 11, 10, 14)));
        assert_eq!(parse_time("2026-11-10T00:00"), Some((2026, 11, 10, 0)));
        assert_eq!(parse_time("11/10/2026 2:05 PM"), Some((2026, 11, 10, 14)));
        assert_eq!(
            parse_time("11/10/2026 12:30:00 AM"),
            Some((2026, 11, 10, 0))
        );
        assert_eq!(parse_time("11/10/2026 12:30 PM"), Some((2026, 11, 10, 12)));
        assert_eq!(parse_time("2026/11/10 23:59"), Some((2026, 11, 10, 23)));
        assert_eq!(parse_time("2026-11-10"), Some((2026, 11, 10, 0)));
        assert_eq!(parse_time("13/10/2026 10:00"), None);
        assert_eq!(parse_time("yesterday"), None);
    }

    #[test]
    fn averages_readings_into_hours_and_converts_celsius() {
        let s = parse_series(
            "Timestamp,Temperature (°C),Humidity\n2026-11-03 02:00,16,40\n2026-11-03 02:30,18,40\n",
            false,
        )
        .unwrap();
        assert_eq!(s.hours[&hr(11, 3, 2)], 62.6); // mean 17°C
    }

    #[test]
    fn separate_date_and_time_columns_semicolons_and_decimal_commas() {
        let s = parse_series("Date;Time;Temp °F\n03.11.2026;02:10;61,5\n", false);
        // DD.MM.YYYY is not accepted (ambiguous with US order), so this must fail loudly, not misread.
        assert!(s.is_err());
        let s = parse_series("Date;Time;Temp °F\n2026.11.03;02:10;61,5\n", false).unwrap();
        assert_eq!(s.hours[&hr(11, 3, 2)], 61.5);
    }

    #[test]
    fn unit_guessing_only_where_allowed() {
        let text = "time,temperature\n2026-11-03 02:00,17\n";
        let s = parse_series(text, true).unwrap();
        assert_eq!(s.hours[&hr(11, 3, 2)], 62.6);
        assert!(s.warnings[0].contains("°C"));
        assert!(parse_series(text, false).is_err());
    }

    #[test]
    fn ignores_dew_point_and_reports_missing_columns() {
        let s = parse_series(
            "Time,Dew point (F),Temperature (F)\n2026-11-03 02:00,30,61\n",
            false,
        )
        .unwrap();
        assert_eq!(s.hours[&hr(11, 3, 2)], 61.0);
        let e = parse_series("when,reading\n", true).unwrap_err();
        assert!(e.contains("when | reading"), "{e}");
    }

    #[test]
    fn refuses_utc_times() {
        for t in [
            "2026-11-03T07:00:00Z",
            "2026-11-03T02:00:00-05:00",
            "2026-11-03 02:00:00+0000",
        ] {
            let e = parse_series(&format!("time,temp (F)\n{t},61\n"), false).unwrap_err();
            assert!(e.contains("UTC"), "{t}: {e}");
        }
        assert!(parse_series("time,temp (F)\n11/03/2026 2:00:00 PM,61\n", false).is_ok());
    }

    #[test]
    fn hour_ranges() {
        assert_eq!(ranges(&[6, 7, 8, 23]), "6am–9am, 11pm–12am");
        assert_eq!(ranges(&[0, 1, 12]), "12am–2am, 12pm–1pm");
    }
}
