// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! `aarnn-mujoco --wall-report TRACE --half-m H [--contact-m C] [--clear-m D] [--json]`:
//! how a body copes with the walls of a square enclosure centred on the
//! origin (inner half-size `H`), from the root pose in a recorded trace.
//!
//! The wall distance is `H - max(|x|, |y|)` at the root. The root sits inside
//! the body, so `--contact-m` gives the distance at which the body touches
//! (zebrafish root near the snout: 0.02 m; fly thorax: 0.04 m).
//!
//! Per release window (`NM_MUJOCO_RELEASE_*`; the whole run if there are
//! none):
//! - first contact after the release;
//! - time in contact;
//! - escape: after the first contact, the root reaching `--clear-m` (default
//!   0.10 m) from every wall, and how long that took;
//! - the lowest up-vector z (1 upright, 0 on its side: a fly rearing against
//!   a wall pitches up).
use std::io::BufRead;

struct Window {
    start: f64,
    end: f64,
    contact_at: Option<f64>,
    contact_s: f64,
    escaped_after: Option<f64>,
    min_up: f64,
    end_distance: f64,
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut trace = None;
    let mut half = None;
    let mut contact = 0.02f64;
    let mut clear = 0.10f64;
    let mut json = false;
    let mut i = 0;
    let value = |i: usize, name: &str| -> Result<f64, String> {
        args.get(i + 1)
            .and_then(|v| v.parse().ok())
            .ok_or(format!("{name} metres"))
    };
    while i < args.len() {
        match args[i].as_str() {
            "--wall-report" => {
                trace = Some(args.get(i + 1).ok_or("--wall-report TRACE")?.clone());
                i += 1;
            }
            "--half-m" => {
                half = Some(value(i, "--half-m")?);
                i += 1;
            }
            "--contact-m" => {
                contact = value(i, "--contact-m")?;
                i += 1;
            }
            "--clear-m" => {
                clear = value(i, "--clear-m")?;
                i += 1;
            }
            "--json" => json = true,
            other => return Err(format!("unknown wall-report option {other}")),
        }
        i += 1;
    }
    let trace = trace.ok_or("--wall-report TRACE")?;
    let half = half.ok_or("--half-m: inner half-size of the enclosure (m)")?;
    let file = std::fs::File::open(&trace).map_err(|e| format!("{trace}: {e}"))?;
    let mut lines = std::io::BufReader::new(file).lines();
    lines.next();
    // (t, wall distance, up z, releases so far)
    let mut rows: Vec<(f64, f64, f64, u64)> = Vec::new();
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            break;
        };
        let (Some(t), Some(q)) = (
            v["time_s"].as_f64(),
            v["qpos"].as_array().filter(|q| q.len() >= 7),
        ) else {
            continue;
        };
        let q: Vec<f64> = q[..7].iter().map(|x| x.as_f64().unwrap_or(0.)).collect();
        let distance = half - q[0].abs().max(q[1].abs());
        let up = 1. - 2. * (q[4] * q[4] + q[5] * q[5]);
        let releases = v["needs"]["releases"].as_u64().unwrap_or(0);
        rows.push((t, distance, up, releases));
    }
    let last = rows.last().ok_or("trace has no root poses")?;
    // Window starts: each increment of the release counter (or the start).
    let mut starts: Vec<f64> = rows
        .windows(2)
        .filter(|p| p[1].3 > p[0].3)
        .map(|p| p[1].0)
        .collect();
    if starts.is_empty() {
        starts.push(rows[0].0);
    }
    let dt = if rows.len() > 1 {
        (last.0 - rows[0].0) / (rows.len() - 1) as f64
    } else {
        0.
    };
    let mut windows = Vec::new();
    for (k, &start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(last.0 + dt);
        let span: Vec<_> = rows.iter().filter(|r| r.0 >= start && r.0 < end).collect();
        if span.is_empty() {
            continue;
        }
        let contact_at = span.iter().find(|r| r.1 <= contact).map(|r| r.0);
        let escaped_after = contact_at.and_then(|c| {
            span.iter()
                .find(|r| r.0 > c && r.1 >= clear)
                .map(|r| r.0 - c)
        });
        windows.push(Window {
            start,
            end,
            contact_at,
            contact_s: span.iter().filter(|r| r.1 <= contact).count() as f64 * dt,
            escaped_after,
            min_up: span.iter().map(|r| r.2).fold(f64::INFINITY, f64::min),
            end_distance: span.last().map(|r| r.1).unwrap_or(f64::NAN),
        });
    }
    let touched: Vec<_> = windows.iter().filter(|w| w.contact_at.is_some()).collect();
    let mut latencies: Vec<f64> = touched.iter().filter_map(|w| w.escaped_after).collect();
    latencies.sort_by(f64::total_cmp);
    let median = latencies
        .get(latencies.len() / 2)
        .copied()
        .unwrap_or(f64::NAN);
    let contact_total: f64 = windows.iter().map(|w| w.contact_s).sum();
    let span_total: f64 = windows.iter().map(|w| w.end - w.start).sum();
    if json {
        let per: Vec<_> = windows
            .iter()
            .map(|w| {
                serde_json::json!({
                    "start_s": w.start, "end_s": w.end,
                    "first_contact_s": w.contact_at.map(|c| c - w.start),
                    "contact_s": w.contact_s, "escaped_after_s": w.escaped_after,
                    "min_up_z": w.min_up, "end_wall_distance_m": w.end_distance,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "trace": trace, "half_m": half, "contact_m": contact, "clear_m": clear,
                "windows": per, "touched": touched.len(), "escaped": latencies.len(),
                "median_escape_s": median, "contact_fraction": contact_total / span_total.max(1e-9),
            })
        );
        return Ok(());
    }
    println!("{trace}");
    println!("  enclosure +-{half} m; contact <= {contact} m from a wall; clear >= {clear} m");
    for w in &windows {
        let first = w
            .contact_at
            .map(|c| format!("touched after {:.1} s", c - w.start))
            .unwrap_or_else(|| "no contact".into());
        let escape = match (w.contact_at, w.escaped_after) {
            (None, _) => String::new(),
            (Some(_), Some(e)) => format!("; clear {e:.1} s later"),
            (Some(_), None) => "; never cleared".into(),
        };
        println!(
            "  window {:.0}-{:.0} s: {first}{escape}; in contact {:.1} s; lowest up-z {:.2}; ends {:.2} m from a wall",
            w.start, w.end, w.contact_s, w.min_up, w.end_distance
        );
    }
    println!(
        "  windows {}: touched {}, cleared {}; median time to clear {:.1} s; in contact {:.0} % of the time",
        windows.len(),
        touched.len(),
        latencies.len(),
        median,
        100. * contact_total / span_total.max(1e-9)
    );
    Ok(())
}
