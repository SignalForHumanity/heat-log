//! End to end: a logger export and an outdoor file in, the printed report and CSV out.
use heat_log::check_csv;

const INDOOR: &str = "\u{feff}Timestamp,Temperature_Fahrenheit,Relative_Humidity\n\
2026-11-03 01:00:00,61.0,40\n\
2026-11-03 01:30:00,60.0,40\n\
2026-11-03 02:10:00,63.0,40\n\
2026-11-03 09:00:00,66.0,40\n\
2026-11-03 10:00:00,66.0,40\n\
2026-11-03 14:00:00,64.0,40\n\
2026-11-04 12:00:00,66.0,40\n\
2026-06-10 02:00:00,50.0,40\n";

const OUTDOOR: &str = "time,temperature (F)\n\
2026-11-03T09:00,40\n2026-11-03T10:00,41\n2026-11-03T14:00,58\n";

#[test]
fn hours_below_the_minimum_are_found_and_reported() {
    let c = check_csv(INDOOR, OUTDOOR).unwrap();
    let csv = c.csv();
    // 1am averages 60.5 (< 62 at night); 2am is fine; 9 and 10am are cold outside and < 68;
    // 2pm is 58 outside, so no minimum; Nov 4 noon has no outdoor reading; June is off season.
    assert!(
        csv.contains("2026-11-03,01:00,60.5,,62.0,violation\n"),
        "{csv}"
    );
    assert!(csv.contains("2026-11-03,02:00,63.0,,62.0,ok\n"));
    assert!(csv.contains("2026-11-03,09:00,66.0,40.0,68.0,violation\n"));
    assert!(csv.contains("2026-11-03,14:00,64.0,58.0,,ok\n"));
    assert!(csv.contains("2026-11-04,12:00,66.0,,,possible (no outdoor temperature)\n"));
    assert!(csv.contains("2026-06-10,02:00,50.0,,,outside heat season\n"));

    let html = c.html();
    assert!(
        html.contains("<th>Hours below the legal minimum</th><td>3</td>"),
        "{html}"
    );
    assert!(html.contains("<th>Days with at least one such hour</th><td>1</td>"));
    assert!(html.contains("<td>2026-11-03</td><td>1am–2am, 9am–11am</td><td>60.5°F at 1am</td>"));
    assert!(html.contains("<td>2026-11-04</td><td>possible: 12pm–1pm</td>"));
    assert!(html.contains("outside heat season"));
}

#[test]
fn file_text_in_messages_is_escaped() {
    let c = check_csv("time,temp <b>x</b>\n2026-11-03 01:00,61\n", "").unwrap();
    assert!(!c.html().contains("<b>"));
    assert!(c.html().contains("&lt;b&gt;"));
}

#[test]
fn errors_say_which_file() {
    assert!(check_csv("a,b\n1,2\n", "")
        .err()
        .unwrap()
        .starts_with("Indoor file:"));
    let e = check_csv(INDOOR, "time,temperature\n2026-11-03T09:00,40\n")
        .err()
        .unwrap();
    assert!(
        e.starts_with("Outdoor file:") && e.contains("°F or °C"),
        "{e}"
    );
}
