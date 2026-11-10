---
slug: heat-log
title: Web page that turns a home thermometer's CSV export into a printable NYC heat-law violation log
verdict: build
---

## Problem

NYC tenants without heat must document it themselves to get 311/HPD action
or win in Housing Court. Tenant Triage NYC tells them to "Write down the
date, time, indoor temp, and outdoor temp", "a few times a day", and to
"Photograph your thermometer next to something showing the time"
(https://tenanttriagenyc.org/scenarios/heat-hot-water), on Met Council's
paper heat log. Handwritten logs are easy for a landlord to dismiss and hard
to keep up, especially for older tenants. Lead found by a RustForge cycle
on 2026-11-09.

## Who benefits

Tenants in cold apartments, and the tenant organizers and legal-services
staff who help them. They find it through tenant organizations, 311 guidance
and search; it needs a browser and a thermometer's export file.

## Existing solutions

- Heat Seek NYC: free mesh-networked sensors and a web app that flagged
  violations. "This project has been put to rest" (Civic Tech Field Guide,
  https://civictech.guide/projects/heatseek; closing post dated 2024-06-25).
- NYC's Heat Sensors Program: the city makes certain landlords install
  sensors; tenants don't control it.
- JustFix: photo/timeline evidence, not temperature logs.
- Logger apps (SensorPush, Govee, ThermoPro): graphs and CSV export, no
  legal check.

## Why build anything

The hardware is now cheap and logs hourly by itself; the missing piece is
applying the law (time of day, outdoor temperature, season) and producing a
readable log. No free tool does that since Heat Seek closed.

## Smallest useful intervention

A static page: open the CSV, optionally fetch Central Park outdoor
temperatures, get a printable summary with each day's violation hours and a
CSV of every hour. Rust (WebAssembly) for parsing and the rules.

## Success criterion

A tenant gets from export file to printed log in under 5 minutes, and a
tenant organization links to it.

## Maintenance

Static page on GitHub Pages; no server. Breaks if the heat law changes, if
logger export formats change in ways the header detection misses, or if
Open-Meteo's free archive API changes (manual outdoor files still work).

## Decision

Build. Built in Claude Code rather than a forge cycle.
