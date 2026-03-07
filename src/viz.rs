//! # Simulation Result Visualization (Static PNG)
//!
//! This module provides utility functions for generating static graphical reports
//! from simulation results. These are used in batch mode to provide visual
//! feedback without requiring a GUI.
//!
//! ## Visualizations:
//! - **Network Diagram (`draw_network_diagram`)**: A high-level schematic showing
//!   the network's layers and neurons.
//! - **Spike Raster (`draw_spike_raster`)**: A temporal plot showing when each
//!   neuron in the network fired.
//! - **Weight Histograms (`draw_weight_histograms`)**: Distribution of synaptic
//!   weights across the network.
//!
//! These functions use the `plotters` crate for rendering and produce standard
//! PNG files.
use anyhow::Result;
use ndarray::Array2;
use plotters::prelude::full_palette::ORANGE;
use plotters::prelude::*;

use crate::config::NetworkConfig;
use crate::sim::WeightsOut;

const WIDTH: u32 = 1200;
const HEIGHT: u32 = 800;

/// Draw a simple schematic of the network layout to `path`.
///
/// The diagram shows sensory, hidden, and output columns with capped counts to
/// keep the rendering readable for large networks. It is intended as a quick
/// overview rather than precise geometry.
pub fn draw_network_diagram(path: &str, cfg: &NetworkConfig, _w: &WeightsOut) -> Result<()> {
    let root = BitMapBackend::new(path, (WIDTH, HEIGHT)).into_drawing_area();
    root.fill(&WHITE)?;
    let mut chart = ChartBuilder::on(&root)
        .caption("Neuromorphic Network Diagram", ("sans-serif", 28))
        .margin(20)
        .build_cartesian_2d(0f32..1f32, 0f32..1f32)?;
    chart
        .configure_mesh()
        .disable_mesh()
        .x_labels(0)
        .y_labels(0)
        .draw()?;
    // Simple schematic: columns for S, H1..HL, O
    let cols = cfg.num_hidden_layers + 2;
    let x_step = 1.0 / (cols as f32 + 1.0);
    // Sensory
    let x_sensory = x_step;
    for i in 0..cfg.num_sensory_neurons.min(80) {
        let y = 0.1 + 0.8 * (i as f32) / (cfg.num_sensory_neurons.min(80) as f32 + 1.0);
        chart.draw_series(std::iter::once(Circle::new(
            (x_sensory, y),
            3,
            BLUE.filled(),
        )))?;
    }
    // Hidden layers
    for l in 0..cfg.num_hidden_layers {
        let x = x_step * (2.0 + l as f32);
        for j in 0..cfg.num_hidden_per_layer_initial.min(40) {
            let y =
                0.1 + 0.8 * (j as f32) / (cfg.num_hidden_per_layer_initial.min(40) as f32 + 1.0);
            chart.draw_series(std::iter::once(Circle::new((x, y), 4, ORANGE.filled())))?;
        }
    }
    // Output
    let x_output = x_step * (2.0 + cfg.num_hidden_layers as f32);
    for k in 0..cfg.num_output_neurons.min(20) {
        let y = 0.1 + 0.8 * (k as f32) / (cfg.num_output_neurons.min(20) as f32 + 1.0);
        chart.draw_series(std::iter::once(Circle::new(
            (x_output, y),
            5,
            GREEN.filled(),
        )))?;
    }
    root.present()?;
    Ok(())
}

/// Draw a multi‑panel spike raster for hidden layers and the output layer.
///
/// - `spikes_h`: per‑layer matrices of shape (steps × H(l)).
/// - `spikes_o`: matrix of shape (steps × O).
/// - `dt`: step time in ms (informational only; not drawn on axes to avoid
///   plotters tick overflows for large step ranges).
pub fn draw_spike_raster(
    path: &str,
    spikes_h: &Vec<Array2<i8>>,
    spikes_o: &Array2<i8>,
    _dt: f64,
) -> Result<()> {
    let h = 800u32;
    let root = BitMapBackend::new(path, (1200, h)).into_drawing_area();
    root.fill(&WHITE)?;
    let total_layers = spikes_h.len() + 1;
    let areas = root.split_evenly((total_layers as usize, 1));
    // Hidden layers
    for (l, sp) in spikes_h.iter().enumerate() {
        let area = &areas[l];
        let steps = sp.nrows();
        let n = sp.ncols();
        if steps == 0 || n == 0 {
            continue;
        }
        let mut chart = ChartBuilder::on(area)
            .margin(10)
            .caption(format!("Hidden L{}", l + 1), ("sans-serif", 16))
            .build_cartesian_2d(0..steps, 0..n)?;
        // Avoid drawing mesh/ticks to prevent plotters usize key_points overflow on large ranges
        chart.draw_series(
            (0..steps)
                .flat_map(|t| {
                    (0..n).filter_map(move |j| if sp[(t, j)] != 0 { Some((t, j)) } else { None })
                })
                .map(|(t, j)| Circle::new((t, j), 1, BLACK.filled())),
        )?;
    }
    // Output layer at bottom
    let area = &areas[total_layers - 1];
    let steps = spikes_o.nrows();
    let n = spikes_o.ncols();
    if steps == 0 || n == 0 {
        root.present()?;
        return Ok(());
    }
    let mut chart = ChartBuilder::on(area)
        .margin(10)
        .caption("Output", ("sans-serif", 16))
        .build_cartesian_2d(0..steps, 0..n)?;
    // Avoid drawing mesh/ticks to prevent plotters usize key_points overflow on large ranges
    chart.draw_series(
        (0..steps)
            .flat_map(|t| {
                (0..n).filter_map(move |j| {
                    if spikes_o[(t, j)] != 0 {
                        Some((t, j))
                    } else {
                        None
                    }
                })
            })
            .map(|(t, j)| Circle::new((t, j), 1, BLACK.filled())),
    )?;

    root.present()?;
    Ok(())
}

/// Draw histograms of weights. If `outputs_only` is true, only `w_out` is used.
pub fn draw_weight_histograms(path: &str, w: &WeightsOut, outputs_only: bool) -> Result<()> {
    let root = BitMapBackend::new(path, (1200, 800)).into_drawing_area();
    root.fill(&WHITE)?;
    let mut chart = ChartBuilder::on(&root)
        .caption(
            if outputs_only {
                "Weight Histogram (Output)"
            } else {
                "Weight Histograms"
            },
            ("sans-serif", 24),
        )
        .margin(20)
        .build_cartesian_2d(0f32..1f32, 0u32..100u32)?;
    chart.configure_mesh().draw()?;

    // Collect weights
    let collect = |arr: &Array2<f64>| -> Vec<f64> { arr.iter().copied().collect() };
    let mut bins = vec![0u32; 20];
    let weights: Vec<f64> = if outputs_only {
        collect(&w.w_out)
    } else {
        let mut v = collect(&w.w_in);
        for m in &w.w_hh_fwd {
            v.extend(m.iter().copied());
        }
        for m in &w.w_hh_bwd {
            v.extend(m.iter().copied());
        }
        v
    };
    if !weights.is_empty() {
        let w_min = 0.0f64;
        let w_max = 1.0f64;
        for &val in &weights {
            let x = val.clamp(w_min, w_max);
            let b = ((x - w_min) / (w_max - w_min) * (bins.len() as f64)) as usize;
            let idx = b.min(bins.len() - 1);
            bins[idx] += 1;
        }
    }
    let _max_bin_count = bins.iter().copied().max().unwrap_or(1);
    chart.configure_mesh().disable_mesh().draw()?;
    chart.draw_series(bins.iter().enumerate().map(|(i, &c)| {
        let x0 = i as f32 / bins.len() as f32;
        let x1 = (i + 1) as f32 / bins.len() as f32;
        Rectangle::new([(x0, 0u32), (x1, c)], RED.mix(0.5).filled())
    }))?;
    root.present()?;
    Ok(())
}

/// Draw a final weighted network visualization. Currently a placeholder that
/// reuses the basic diagram; kept as a separate function to allow future
/// enhancement (e.g., weighting edge opacity by magnitude).
pub fn draw_final_weighted_network(path: &str, cfg: &NetworkConfig, _w: &WeightsOut) -> Result<()> {
    // For now, draw a simple placeholder similar to the diagram
    draw_network_diagram(path, cfg, _w)
}
