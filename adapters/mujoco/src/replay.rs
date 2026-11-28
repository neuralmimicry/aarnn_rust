// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! `aarnn-mujoco --replay TRACE --video OUT.mp4`: render a recorded lock-step
//! trace (exact `qpos` per row) into a video, in Rust.
//!
//! The body is posed at each frame's recorded generalised position, rendered
//! by the native viewer at the requested size (a GLFW window; on a headless
//! host run it on a virtual display such as Xvfb) and overlaid with MuJoCo's
//! own text and figures:
//! - the title and the simulated time;
//! - the energy needs (`needs` in the trace): reserve, refill, caretaker
//!   feeding, source reached;
//! - with `--neuromod FILE` (the runtime's `NM_NEUROMOD_TRACE` jsonl), a
//!   live figure of global dopamine, serotonin, noradrenaline and
//!   acetylcholine (percent above their baselines);
//! - with `--plot-sensor NAME,LABEL,A,B` (repeatable), a figure of
//!   `A + B x` the named sensor channel (e.g. a sonar reading as metres).
//!
//! Frames are piped as raw RGB straight into `ffmpeg` (H.264), with no
//! intermediate image files.
use aarnn_mujoco::native::{
    Physics,
    viewer::{FigureLine, FigureSpec, OverlayText, Viewer},
};
use std::{
    io::{BufRead, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

struct Row {
    t: f64,
    qpos: Vec<f64>,
    sensors: Vec<f64>,
    needs: Option<serde_json::Value>,
}
struct Plot {
    index: usize,
    label: String,
    offset: f64,
    scale: f64,
    /// `(threshold, window_s)`: plot the fraction of the last `window_s`
    /// seconds in which `A + B x` was below `threshold` instead of the value.
    near: Option<(f64, f64)>,
}
struct Args {
    trace: PathBuf,
    video: PathBuf,
    assets: PathBuf,
    width: i32,
    height: i32,
    fps: f64,
    speed: f64,
    start: f64,
    end: Option<f64>,
    distance: Option<f64>,
    azimuth: f64,
    elevation: f64,
    lookat: Option<[f64; 3]>,
    lookat_height: f64,
    title: String,
    subtitle: String,
    neuromod: Option<PathBuf>,
    window_s: f64,
    plots: Vec<(String, String, f64, f64, Option<(f64, f64)>)>,
}
fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        trace: PathBuf::new(),
        video: PathBuf::new(),
        assets: PathBuf::from("sim/content/embodiments/generated"),
        width: 1280,
        height: 720,
        fps: 25.,
        speed: 1.,
        start: 0.,
        end: None,
        distance: None,
        azimuth: 135.,
        elevation: -35.,
        lookat: None,
        lookat_height: 0.,
        title: String::new(),
        subtitle: String::new(),
        neuromod: None,
        window_s: 10.,
        plots: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let value = || args.get(i + 1).ok_or(format!("{} needs a value", args[i]));
        let number = |v: &String| {
            v.parse::<f64>()
                .map_err(|_| format!("{} expects a number", args[i]))
        };
        match args[i].as_str() {
            "--replay" => a.trace = PathBuf::from(value()?),
            "--video" => a.video = PathBuf::from(value()?),
            "--assets" => a.assets = PathBuf::from(value()?),
            "--width" => a.width = number(value()?)? as i32,
            "--height" => a.height = number(value()?)? as i32,
            "--fps" => a.fps = number(value()?)?,
            "--speed" => a.speed = number(value()?)?,
            "--start-s" => a.start = number(value()?)?,
            "--end-s" => a.end = Some(number(value()?)?),
            "--distance" => a.distance = Some(number(value()?)?),
            "--azimuth" => a.azimuth = number(value()?)?,
            "--elevation" => a.elevation = number(value()?)?,
            "--lookat-height" => a.lookat_height = number(value()?)?,
            "--lookat" => {
                let v: Vec<f64> = value()?
                    .split(',')
                    .map(|x| x.trim().parse::<f64>().map_err(|_| "--lookat X,Y,Z"))
                    .collect::<Result<_, _>>()?;
                a.lookat = Some(v.try_into().map_err(|_| "--lookat X,Y,Z")?);
            }
            "--title" => a.title = value()?.clone(),
            "--subtitle" => a.subtitle = value()?.clone(),
            "--neuromod" => a.neuromod = Some(PathBuf::from(value()?)),
            "--window-s" => a.window_s = number(value()?)?,
            "--plot-sensor" => {
                let v: Vec<&str> = value()?.split(',').collect();
                let [name, label, offset, scale] = v[..] else {
                    return Err("--plot-sensor NAME,LABEL,A,B".into());
                };
                a.plots.push((
                    name.to_owned(),
                    label.to_owned(),
                    offset.parse().map_err(|_| "--plot-sensor A")?,
                    scale.parse().map_err(|_| "--plot-sensor B")?,
                    None,
                ));
            }
            "--plot-near" => {
                let v: Vec<&str> = value()?.split(',').collect();
                let [name, label, offset, scale, threshold, window] = v[..] else {
                    return Err("--plot-near NAME,LABEL,A,B,THRESHOLD,WINDOW_S".into());
                };
                let f = |x: &str| x.parse::<f64>().map_err(|_| "--plot-near number");
                a.plots.push((
                    name.to_owned(),
                    label.to_owned(),
                    f(offset)?,
                    f(scale)?,
                    Some((f(threshold)?, f(window)?)),
                ));
            }
            other => return Err(format!("unknown replay option {other}")),
        }
        i += 2;
    }
    if a.trace.as_os_str().is_empty() || a.video.as_os_str().is_empty() {
        return Err("--replay TRACE --video OUT.mp4 required".into());
    }
    if !(a.fps > 0. && a.fps <= 120. && a.speed > 0. && a.window_s > 0.) {
        return Err("replay fps/speed/window".into());
    }
    Ok(a)
}

/// Neuromodulator samples (simulated seconds, DA, 5-HT, NA, ACh).
fn neuromod(path: &PathBuf) -> Result<Vec<[f64; 5]>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for line in std::io::BufReader::new(file).lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            break;
        };
        let f = |k: &str| v[k].as_f64().unwrap_or(f64::NAN);
        out.push([
            f("t_ms") / 1000.,
            f("dopamine"),
            f("serotonin"),
            f("noradrenaline"),
            f("acetylcholine"),
        ]);
    }
    Ok(out)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let a = parse(args)?;
    let file = std::fs::File::open(&a.trace).map_err(|e| format!("{}: {e}", a.trace.display()))?;
    let mut lines = std::io::BufReader::new(file).lines();
    let header: serde_json::Value = serde_json::from_str(
        &lines
            .next()
            .ok_or("empty trace")?
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("trace header: {e}"))?;
    let profile = header["profile"]
        .as_str()
        .ok_or("trace header lacks profile")?;
    let sensor_names: Vec<&str> = header["sensor_names"]
        .as_array()
        .map(|v| v.iter().filter_map(|s| s.as_str()).collect())
        .unwrap_or_default();
    let plots = a
        .plots
        .iter()
        .map(|(name, label, offset, scale, near)| {
            Ok(Plot {
                near: *near,
                index: sensor_names
                    .iter()
                    .position(|n| n == name)
                    .ok_or(format!("trace has no sensor {name}"))?,
                label: label.clone(),
                offset: *offset,
                scale: *scale,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut rows = Vec::new();
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        // A run cut off mid-write ends with a partial line.
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            break;
        };
        let (Some(t), Some(q)) = (v["time_s"].as_f64(), v["qpos"].as_array()) else {
            continue;
        };
        rows.push(Row {
            t,
            qpos: q.iter().filter_map(|x| x.as_f64()).collect(),
            sensors: v["sensors"]
                .as_array()
                .map(|s| s.iter().map(|x| x.as_f64().unwrap_or(0.)).collect())
                .unwrap_or_default(),
            needs: v.get("needs").filter(|n| !n.is_null()).cloned(),
        });
    }
    if rows.is_empty() {
        return Err("trace has no qpos rows".into());
    }
    let model = a.assets.join(format!("{profile}.xml"));
    let mut physics = Physics::load(&model)?;
    let samples = match &a.neuromod {
        Some(p) => neuromod(p)?,
        None => Vec::new(),
    };
    // The brain's clock runs at its own step rate in lock-step; place its
    // samples on the body's time axis proportionally.
    let body_end = rows.last().map_or(0., |r| r.t);
    let brain_end = samples.last().map_or(0., |s| s[0]);
    let brain_scale = if brain_end > 0. {
        body_end / brain_end
    } else {
        1.
    };
    let end = a.end.unwrap_or(body_end).min(body_end);
    let mut viewer = Viewer::new_sized(&physics, "aarnn replay", a.width, a.height)?;
    let distance = a.distance.unwrap_or(1.5);
    let mut ffmpeg = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-s",
        ])
        .arg(format!("{}x{}", a.width, a.height))
        .args([
            "-r",
            &a.fps.to_string(),
            "-i",
            "-",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-crf",
            "20",
        ])
        .arg(&a.video)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ffmpeg: {e}"))?;
    let mut pipe = ffmpeg.stdin.take().ok_or("ffmpeg stdin")?;
    let mut frames = 0u64;
    let mut t = a.start;
    let mut k = 0usize;
    let mut history: Vec<(f64, Vec<f64>, Option<f64>)> = Vec::new();
    // Untransformed-by-window plot values (A + B x) per frame, for `--plot-near`.
    let mut raw: Vec<(f64, Vec<f64>)> = Vec::new();
    while t <= end + 1e-9 {
        while k + 1 < rows.len() && rows[k + 1].t <= t {
            k += 1;
        }
        let row = &rows[k];
        physics.set_qpos(&row.qpos)?;
        let (root, _, _) = physics.body_state(1)?;
        let lookat = a
            .lookat
            .unwrap_or([root[0], root[1], root[2] + a.lookat_height]);
        viewer.set_camera(lookat, distance, a.azimuth, a.elevation);
        let energy = row.needs.as_ref().and_then(|n| n["fraction"].as_f64());
        let values: Vec<f64> = plots
            .iter()
            .map(|p| p.offset + p.scale * row.sensors.get(p.index).copied().unwrap_or(0.))
            .collect();
        raw.push((row.t, values.clone()));
        let shown = plots
            .iter()
            .enumerate()
            .map(|(j, p)| match p.near {
                None => values[j],
                Some((threshold, window)) => {
                    let recent: Vec<f64> = raw
                        .iter()
                        .rev()
                        .take_while(|r| r.0 > row.t - window)
                        .map(|r| r.1[j])
                        .collect();
                    recent.iter().filter(|v| **v < threshold).count() as f64
                        / recent.len().max(1) as f64
                }
            })
            .collect();
        history.push((row.t, shown, energy));
        let from = row.t - a.window_s;
        let mut figures = Vec::new();
        let mut slot = 0.;
        if !samples.is_empty() {
            let colours = [
                [0.95, 0.76, 0.31],
                [0.71, 0.56, 0.68],
                [0.88, 0.42, 0.35],
                [0.37, 0.70, 0.70],
            ];
            let names = ["dopamine", "serotonin", "noradrenaline", "acetylcholine"];
            let lines = (0..4)
                .map(|m| FigureLine {
                    name: names[m].into(),
                    rgb: colours[m],
                    points: samples
                        .iter()
                        // Percent above baseline (MuJoCo figures round the
                        // y-range to whole numbers; levels change by tenths).
                        .map(|s| (s[0] * brain_scale, 100. * (s[m + 1] - 1.)))
                        .filter(|(x, y)| *x >= from && *x <= row.t && y.is_finite())
                        .map(|(x, y)| [x as f32, y as f32])
                        .collect(),
                })
                .collect();
            figures.push(FigureSpec {
                title: "Neuromodulators (% above baseline)".into(),
                rect: [0.60, 0.52, 0.39, 0.46],
                x_range: [from as f32, row.t.max(from + 1e-3) as f32],
                y_range: [-10., 80.],
                lines,
            });
            slot += 1.;
        }
        let mut lines = Vec::new();
        for (j, p) in plots.iter().enumerate() {
            let colours = [[0.61, 0.80, 0.40], [0.40, 0.65, 0.95], [0.95, 0.55, 0.85]];
            lines.push(FigureLine {
                name: p.label.clone(),
                rgb: colours[j % colours.len()],
                points: history
                    .iter()
                    .filter(|h| h.0 >= from)
                    .map(|h| [h.0 as f32, h.1[j] as f32])
                    .collect(),
            });
        }
        if energy.is_some() {
            lines.push(FigureLine {
                name: "energy reserve".into(),
                rgb: [0.85, 0.85, 0.85],
                points: history
                    .iter()
                    .filter(|h| h.0 >= from)
                    .filter_map(|h| h.2.map(|e| [h.0 as f32, e as f32]))
                    .collect(),
            });
        }
        if !lines.is_empty() {
            let y_max = history
                .iter()
                .filter(|h| h.0 >= from)
                .flat_map(|h| h.1.iter().copied().chain(h.2))
                .fold(1.0f64, f64::max);
            figures.push(FigureSpec {
                title: if plots.is_empty() {
                    "Energy".into()
                } else {
                    "Senses and energy".into()
                },
                rect: [0.60, 0.52 - 0.48 * slot, 0.39, 0.46],
                x_range: [from as f32, row.t.max(from + 1e-3) as f32],
                // MuJoCo rounds the figure range: 0-1.05 drew as 0-2. Data
                // within 0-1 (sensor codes, reserve) gets exactly 0-1.
                y_range: [
                    0.,
                    if y_max <= 1.0 {
                        1.0
                    } else {
                        (y_max * 1.05) as f32
                    },
                ],
                lines,
            });
        }
        let mut status_left = String::from("time");
        let mut status_right = format!("{:.1} s", row.t);
        if let Some(n) = &row.needs {
            status_left.push_str("\nenergy reserve\nrefill");
            status_right.push_str(&format!(
                "\n{:.0} %\n{}",
                100. * n["fraction"].as_f64().unwrap_or(0.),
                if n["caretaker_feeding"].as_bool() == Some(true) {
                    "caretaker feeding".to_owned()
                } else if let Some(s) = n["at_source"].as_str() {
                    format!("at {s}")
                } else {
                    "-".to_owned()
                }
            ));
            if n["exhausted"].as_bool() == Some(true) {
                status_left.push_str("\nstate");
                status_right.push_str("\nexhausted");
            }
        }
        let texts = [
            OverlayText {
                gridpos: 0,
                left: a.title.clone(),
                right: a.subtitle.clone(),
            },
            OverlayText {
                gridpos: 2,
                left: status_left,
                right: status_right,
            },
        ];
        let (w, h, rgb) = viewer.capture(&mut physics, &texts, &figures)?;
        if w != a.width as usize || h != a.height as usize {
            return Err(format!(
                "framebuffer {w}x{h} differs from the requested {}x{} (window manager resize?)",
                a.width, a.height
            ));
        }
        pipe.write_all(&rgb)
            .map_err(|e| format!("ffmpeg pipe: {e}"))?;
        frames += 1;
        t = a.start + frames as f64 * a.speed / a.fps;
    }
    drop(pipe);
    let status = ffmpeg.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("ffmpeg exited with {status}"));
    }
    eprintln!(
        "replay: {frames} frames ({:.1}-{:.1} s at {}x speed) -> {}",
        a.start,
        end,
        a.speed,
        a.video.display()
    );
    Ok(())
}
