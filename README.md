# heat-log

**Use it: https://signalforhumanity.github.io/heat-log/**

New York City tenants without heat are told to keep a log: date, time,
indoor and outdoor temperature, a few times a day, with photos of the
thermometer next to a clock. Thermometers that record the temperature every
few minutes and export a CSV file now cost $15 to $50, but there was nothing
to turn that file into a log. Heat Seek NYC, the nonprofit whose sensors did
this, has closed.

This page reads the file, checks every hour against the heat law, and prints
a log for 311, HPD, a lawyer, or Housing Court. The file is read in the
browser and never uploaded. It works on a phone.

## The law it checks

NYC Housing Maintenance Code §27-2029, October 1 to May 31:

- **6am to 10pm:** at least 68°F inside whenever it is below 55°F outside.
- **10pm to 6am:** at least 62°F inside, whatever the weather.

Hot water (120°F all year) is not checked.

## How it counts

- Readings are averaged into clock hours, in the logger's local time. An hour
  counts as below the minimum when its average is.
- The daytime rule needs the outdoor temperature. The page can fetch hourly
  Central Park temperatures from [Open-Meteo](https://open-meteo.com/) (it
  sends only the first and last date of the log), or read an outdoor CSV.
  Open-Meteo's values are modelled, not an official station record. Without
  outdoor data, night hours are still checked and daytime hours below 68°F
  are marked "possible".
- Hours outside heat season are listed but not checked.

## Files it reads

Any CSV with a time column and a temperature column, found by their names
(`Timestamp`, `Date`+`Time`, `Temperature_Fahrenheit`, `Temp (°C)` ...).
Comma, semicolon and tab separators work. Dates may be `2026-11-10 14:05`,
`2026/11/10`, or US `11/10/2026 2:05 PM`; day-first dates (`10.11.2026`)
are refused rather than misread. °C is converted; when the indoor file
doesn't name its unit, the values decide (indoor °C and °F don't overlap),
and the report says so. An outdoor file must name its unit.

The exact export formats of SensorPush, Govee, ThermoPro, Inkbird and
SwitchBot are not published. If your file isn't read, please open an issue
with its first two lines.

## Limits

- The tenant controls the thermometer, so this is the tenant's own record,
  like a handwritten log, not independent evidence. Keep the original file,
  note where the thermometer was, and keep it away from windows, radiators
  and the floor. Consumer thermometers are typically accurate to about ±1°F.
- New York City only, for now. Other cities' rules would be one more row.
- Not legal advice.

## Build

```
cargo test
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130
cargo build --lib --release --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir docs/pkg target/wasm32-unknown-unknown/release/heat_log.wasm
cp -R web/. docs/
```

`docs/` is the published page (GitHub Pages, `main` branch, `/docs`). Edit
`web/`, not `docs/`. All logic is in `src/lib.rs`; the page is a thin shell.

See [PROBLEM.md](PROBLEM.md) for the research behind it.

## License

MIT OR Apache-2.0.
