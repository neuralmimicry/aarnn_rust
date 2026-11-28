// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! `aarnn-mujoco --needs-report TRACE [--from-s T] [--json]`: energy-needs
//! summary of a recorded trace (`needs` per row, `aarnn-mujoco-trace-v2`).
//!
//! Over the whole run:
//! - caretaker feeds;
//! - arrivals at a food source / dock;
//! - energy taken in.
//!
//! From `--from-s` (e.g. the end of the caretaker period):
//! - the reserve at that time and its minimum;
//! - each hunger episode: the reserve falling below 0.5, timed to the next
//!   arrival at a source (or the caretaker's feed, exhaustion, or the end
//!   of the run);
//! - the share of time at a source;
//! - exhaustion.
//!
//! Foraging trials (`NM_MUJOCO_RELEASE_*`): each release timed to the first
//! arrival, and how the body moved before it (from the root pose and the
//! source positions in the trace header):
//! - approach speed: the fall in distance to the nearest source per second;
//! - heading error: the angle between the body's heading and the bearing to
//!   that source (90 degrees = no preference);
//! - closed-loop klinokinesis: the turn rate while the body got nearer to /
//!   farther from the nearest source over the last second (by position: the
//!   cue sensors depend on bearing, so turning alone makes them rise).
use std::io::BufRead;

/// Yaw (rad) of a MuJoCo quaternion `[w, x, y, z]`.
fn yaw_of(q: &[f64]) -> f64 {
    let [w, x, y, z] = [q[0], q[1], q[2], q[3]];
    (2. * (w * z + x * y)).atan2(1. - 2. * (y * y + z * z))
}

fn wrap(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(2. * std::f64::consts::PI) - std::f64::consts::PI
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut trace = None;
    let mut from = 0.0f64;
    let mut json = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--needs-report" => {
                trace = Some(args.get(i + 1).ok_or("--needs-report TRACE")?.clone());
                i += 1;
            }
            "--from-s" => {
                from = args
                    .get(i + 1)
                    .and_then(|v| v.parse().ok())
                    .ok_or("--from-s seconds")?;
                i += 1;
            }
            "--json" => json = true,
            other => return Err(format!("unknown needs-report option {other}")),
        }
        i += 1;
    }
    let trace = trace.ok_or("--needs-report TRACE")?;
    let file = std::fs::File::open(&trace).map_err(|e| format!("{trace}: {e}"))?;
    let mut lines = std::io::BufReader::new(file).lines();
    let head: serde_json::Value = lines
        .next()
        .and_then(|l| l.ok())
        .and_then(|l| serde_json::from_str(&l).ok())
        .unwrap_or_default();
    let sources: Vec<[f64; 2]> = head["needs"]["sources"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    let p = s["position_m"].as_array()?;
                    Some([p.first()?.as_f64()?, p.get(1)?.as_f64()?])
                })
                .collect()
        })
        .unwrap_or_default();
    let cue_ix: Vec<usize> = head["sensor_names"]
        .as_array()
        .map(|a| {
            a.iter()
                .enumerate()
                .filter(|(_, n)| n.as_str().is_some_and(|n| n.starts_with("needs_cue")))
                .map(|(i, _)| i)
                .collect()
        })
        .unwrap_or_default();
    // (t, x, y, yaw, cue) per row with a free root.
    let mut poses: Vec<(f64, f64, f64, f64, f64)> = Vec::new();
    // (t, fraction, at_source, caretaker_feeding, consume, exhausted, feeds, visits, consumed_j)
    let mut rows: Vec<(f64, f64, bool, bool, f64, bool, u64, u64, f64)> = Vec::new();
    // (t, releases so far) for foraging trials.
    let mut trial_marks: Vec<(f64, u64)> = Vec::new();
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            break;
        };
        let (Some(t), Some(n)) = (
            v["time_s"].as_f64(),
            v.get("needs").filter(|n| !n.is_null()),
        ) else {
            continue;
        };
        rows.push((
            t,
            n["fraction"].as_f64().unwrap_or(f64::NAN),
            !n["at_source"].is_null(),
            n["caretaker_feeding"].as_bool().unwrap_or(false),
            n["consume"].as_f64().unwrap_or(0.),
            n["exhausted"].as_bool().unwrap_or(false),
            n["caretaker_feeds"].as_u64().unwrap_or(0),
            n["source_visits"].as_u64().unwrap_or(0),
            n["consumed_j"].as_f64().unwrap_or(0.),
        ));
        trial_marks.push((t, n["releases"].as_u64().unwrap_or(0)));
        if let Some(q) = v["qpos"].as_array().filter(|q| q.len() >= 7) {
            let q: Vec<f64> = q[..7].iter().map(|x| x.as_f64().unwrap_or(0.)).collect();
            let cue = if cue_ix.is_empty() {
                f64::NAN
            } else {
                cue_ix
                    .iter()
                    .map(|&i| v["sensors"][i].as_f64().unwrap_or(0.))
                    .sum::<f64>()
                    / cue_ix.len() as f64
            };
            poses.push((t, q[0], q[1], yaw_of(&q[3..7]), cue));
        }
    }
    let last = *rows.last().ok_or("trace has no needs rows")?;
    let at_from = rows.iter().find(|r| r.0 >= from).copied().unwrap_or(last);
    let after: Vec<_> = rows.iter().filter(|r| r.0 >= from).collect();
    let dt = if rows.len() > 1 {
        (last.0 - rows[0].0) / (rows.len() - 1) as f64
    } else {
        0.
    };
    let min_after = after.iter().map(|r| r.1).fold(f64::INFINITY, f64::min);
    let at_source_s = after.iter().filter(|r| r.2).count() as f64 * dt;
    let charging_s = after.iter().filter(|r| r.2 && r.4 > 0.).count() as f64 * dt;
    let exhausted_at = rows.iter().find(|r| r.5).map(|r| r.0);
    let first_arrival = rows.windows(2).find(|w| w[1].2 && !w[0].2).map(|w| w[1].0);
    let arrivals_after = after.windows(2).filter(|w| w[1].2 && !w[0].2).count();
    // Hunger episodes after `from`: the reserve falls below 0.5 -> next source arrival.
    let mut episodes = Vec::new();
    let mut k = 1;
    while k < after.len() {
        if after[k].1 < 0.5 && after[k - 1].1 >= 0.5 {
            let onset = after[k].0;
            let end = after[k..].iter().find(|r| r.2 || r.3 || r.5);
            episodes.push(match end {
                Some(r) if r.2 => (onset, Some(r.0 - onset), "reached source"),
                Some(r) if r.3 => (onset, Some(r.0 - onset), "fed by the caretaker"),
                Some(r) => (onset, Some(r.0 - onset), "exhausted"),
                None => (onset, None, "run ended"),
            });
        }
        k += 1;
    }
    // Foraging trials (NM_MUJOCO_RELEASE_*): from each release to the first
    // arrival at a source, exhaustion or the next release.
    let releases: Vec<(f64, u64)> = {
        let mut out = Vec::new();
        let mut last_count = 0u64;
        for (t, n) in &trial_marks {
            if *n > last_count {
                out.push((*t, *n));
                last_count = *n;
            }
        }
        out
    };
    // The release period: the most common gap between releases.
    let trial_window = releases
        .windows(2)
        .map(|w| w[1].0 - w[0].0)
        .fold(f64::NAN, |a: f64, b| if a.is_nan() { b } else { a.min(b) });
    let nearest = |x: f64, y: f64| -> Option<(f64, f64)> {
        sources
            .iter()
            .map(|s| ((s[0] - x).hypot(s[1] - y), (s[1] - y).atan2(s[0] - x)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
    };
    let mut trials = Vec::new();
    // Pooled over trials, before arrival: |turn rate| (deg/s) while the body
    // approached / receded from the nearest source by > 5 cm/s over 1 s.
    let (mut rising, mut falling) = (Vec::new(), Vec::new());
    let lag = 20usize; // rows per second at the default trace_every 25
    for (k, (t0, _)) in releases.iter().enumerate() {
        let t1 = releases.get(k + 1).map_or(last.0, |r| r.0);
        let seg: Vec<_> = rows.iter().filter(|r| r.0 >= *t0 && r.0 < t1).collect();
        let reached = seg.iter().skip(1).find(|r| r.2).map(|r| r.0 - t0);
        let exhausted = seg.iter().find(|r| r.5).map(|r| r.0 - t0);
        let end = t0 + reached.unwrap_or(t1 - t0);
        // Skip the first 0.5 s: the release itself.
        let path: Vec<_> = poses
            .iter()
            .filter(|p| p.0 >= t0 + 0.5 && p.0 <= end)
            .collect();
        let (mut approach, mut heading_error, mut d_start, mut d_min) =
            (f64::NAN, f64::NAN, f64::NAN, f64::NAN);
        if let (Some(a), Some(b)) = (path.first(), path.last()) {
            if let (Some((da, _)), Some((db, _))) = (nearest(a.1, a.2), nearest(b.1, b.2)) {
                if b.0 > a.0 {
                    approach = (da - db) / (b.0 - a.0);
                }
                d_start = da;
            }
            let errs: Vec<f64> = path
                .iter()
                .filter_map(|p| nearest(p.1, p.2).map(|(_, bearing)| wrap(bearing - p.3).abs()))
                .collect();
            heading_error = errs.iter().sum::<f64>() / errs.len().max(1) as f64;
            d_min = path
                .iter()
                .filter_map(|p| nearest(p.1, p.2).map(|n| n.0))
                .fold(f64::INFINITY, f64::min);
            for w in path.windows(lag + 1) {
                let (a, b) = (w[0], w[lag]);
                let dt = b.0 - a.0;
                let (Some((da, _)), Some((db, _))) = (nearest(a.1, a.2), nearest(b.1, b.2)) else {
                    continue;
                };
                if dt <= 0. {
                    continue;
                }
                let turn = wrap(b.3 - a.3).abs().to_degrees() / dt;
                let closing = (da - db) / dt;
                if closing > 0.05 {
                    rising.push(turn);
                } else if closing < -0.05 {
                    falling.push(turn);
                }
            }
        }
        trials.push(serde_json::json!({"release_s": t0, "window_s": t1 - t0, "reached_s": reached, "exhausted_s": exhausted,
            "start_distance_m": d_start, "min_distance_m": d_min, "approach_mps": approach,
            "heading_error_deg": heading_error.to_degrees()}));
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    let klinokinesis = serde_json::json!({
        "turn_deg_s_approaching": mean(&rising), "approaching_s": rising.len() as f64 * dt,
        "turn_deg_s_receding": mean(&falling), "receding_s": falling.len() as f64 * dt,
    });
    let report = serde_json::json!({
        "trace": trace,
        "duration_s": last.0,
        "caretaker_feeds": last.6,
        "source_visits": last.7,
        "consumed_j": last.8,
        "first_source_arrival_s": first_arrival,
        "from_s": from,
        "reserve_at_from": at_from.1,
        "min_reserve_after": min_after,
        "arrivals_after": arrivals_after,
        "at_source_after_s": at_source_s,
        "charging_after_s": charging_s,
        "exhausted_at_s": exhausted_at,
        "hunger_episodes": episodes.iter().map(|(t, d, how)| serde_json::json!({"onset_s": t, "to_end_s": d, "end": how})).collect::<Vec<_>>(),
        "reserve_at_end": last.1,
        "trials": trials,
        "klinokinesis": klinokinesis,
    });
    if json {
        println!("{report}");
    } else {
        println!(
            "{trace}\n  {:.0} s; caretaker feeds {}; source arrivals {} (first at {}); energy taken in {:.0} J",
            last.0,
            last.6,
            last.7,
            first_arrival.map_or("-".into(), |t| format!("{t:.1} s")),
            last.8
        );
        println!(
            "  from {from:.0} s: reserve {:.2} -> min {:.2}, end {:.2}; arrivals {arrivals_after}; at source {at_source_s:.0} s (charging {charging_s:.0} s); exhausted {}",
            at_from.1,
            min_after,
            last.1,
            exhausted_at.map_or("no".into(), |t| format!("at {t:.1} s"))
        );
        for tr in &trials {
            println!(
                "  trial released at {:.0} s ({:.2} m): {}; approach {:+.3} m/s, heading error {:.0} deg, nearest {:.2} m",
                tr["release_s"].as_f64().unwrap_or(0.),
                tr["start_distance_m"].as_f64().unwrap_or(f64::NAN),
                match tr["reached_s"].as_f64() {
                    Some(d) => format!("reached the source after {d:.1} s"),
                    None => match tr["exhausted_s"].as_f64() {
                        Some(d) => format!("exhausted after {d:.1} s"),
                        None => format!(
                            "not reached in {:.0} s",
                            tr["window_s"].as_f64().unwrap_or(0.)
                        ),
                    },
                },
                tr["approach_mps"].as_f64().unwrap_or(f64::NAN),
                tr["heading_error_deg"].as_f64().unwrap_or(f64::NAN),
                tr["min_distance_m"].as_f64().unwrap_or(f64::NAN),
            );
        }
        if !trials.is_empty() {
            let full: Vec<_> = trials
                .iter()
                .filter(|tr| tr["window_s"].as_f64().unwrap_or(0.) >= trial_window - 0.5)
                .collect();
            let mut times: Vec<f64> = full
                .iter()
                .filter_map(|tr| tr["reached_s"].as_f64())
                .collect();
            times.sort_by(f64::total_cmp);
            let median = match times.len() {
                0 => f64::NAN,
                n if n % 2 == 1 => times[n / 2],
                n => 0.5 * (times[n / 2 - 1] + times[n / 2]),
            };
            // Unreached trials count as the full window (a censored time).
            let censored = (times.iter().sum::<f64>()
                + (full.len() - times.len()) as f64 * trial_window)
                / full.len().max(1) as f64;
            let approach: Vec<f64> = full
                .iter()
                .filter_map(|tr| tr["approach_mps"].as_f64())
                .collect();
            let heading: Vec<f64> = full
                .iter()
                .filter_map(|tr| tr["heading_error_deg"].as_f64())
                .collect();
            println!(
                "  trials (full {trial_window:.0} s windows): {} of {} reached; median {median:.1} s; mean with misses at {trial_window:.0} s: {censored:.1} s; approach {:+.3} m/s; heading error {:.0} deg",
                times.len(),
                full.len(),
                mean(&approach),
                mean(&heading)
            );
            println!(
                "  before arrival, turn rate while approaching the source {:.1} deg/s ({:.0} s), receding {:.1} deg/s ({:.0} s)",
                mean(&rising),
                rising.len() as f64 * dt,
                mean(&falling),
                falling.len() as f64 * dt
            );
        }
        for (t, d, how) in &episodes {
            println!(
                "  hunger at {t:.1} s -> {how}{}",
                d.map_or(String::new(), |d| format!(" after {d:.1} s"))
            );
        }
    }
    Ok(())
}
