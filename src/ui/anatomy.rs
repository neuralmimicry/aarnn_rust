//! Read-only anatomical projection and bounded mesh construction.
//! No PID state, simulation clock or mutable topology participates in a frame.
use crate::morphology_contract::{DisplaySnapshot, Vec3};
use eframe::egui::{self, Color32, Pos2, Vec2, epaint::Mesh};

pub(super) struct Frame {
    pivot: Vec3,
    origin: Pos2,
    x: [f64; 3],
    y: [f64; 3],
    pub boundary: Vec<Pos2>,
}

impl Frame {
    pub fn new(
        snapshot: &DisplaySnapshot,
        origin: Pos2,
        scale: Vec2,
        yaw: f32,
        pitch: f32,
    ) -> Self {
        let pivot = snapshot
            .coverage
            .membrane
            .map(|m| m.centre_mm)
            .or_else(|| {
                snapshot.coverage.region.map(|r| Vec3 {
                    x: (r.min.x + r.max.x) * 0.5,
                    y: (r.min.y + r.max.y) * 0.5,
                    z: (r.min.z + r.max.z) * 0.5,
                })
            })
            .unwrap_or(Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            });
        let (sy, cy) = (yaw as f64).sin_cos();
        let (sp, cp) = (pitch as f64).sin_cos();
        let mut frame = Self {
            pivot,
            origin,
            x: [cy * scale.x as f64, 0.0, sy * scale.x as f64],
            y: [
                -sy * sp * scale.y as f64,
                -cp * scale.y as f64,
                cy * sp * scale.y as f64,
            ],
            boundary: Vec::new(),
        };
        if let Some(m) = snapshot.coverage.membrane {
            // Project the ellipsoid analytically. The same inscribed polygon
            // is used for cloudy fill and clipping, including offset vertices.
            let r = [m.radii_mm.x, m.radii_mm.y, m.radii_mm.z];
            if r.iter().all(|r| r.is_finite() && *r > 0.0) {
                let a = std::array::from_fn::<_, 3, _>(|i| frame.x[i] * r[i]);
                let b = std::array::from_fn::<_, 3, _>(|i| frame.y[i] * r[i]);
                let xx = dot(a, a).sqrt();
                if xx > 1e-9 {
                    let yx = dot(a, b) / xx;
                    let yy = (dot(b, b) - yx * yx).max(0.0).sqrt();
                    frame.boundary = (0..96)
                        .map(|i| {
                            let t = std::f64::consts::TAU * i as f64 / 96.0;
                            origin
                                + egui::vec2(
                                    (xx * t.cos()) as f32,
                                    (yx * t.cos() + yy * t.sin()) as f32,
                                )
                        })
                        .collect();
                }
            }
        } else if let Some(r) = snapshot.coverage.region {
            let mut corners = Vec::new();
            for x in [r.min.x, r.max.x] {
                for y in [r.min.y, r.max.y] {
                    for z in [r.min.z, r.max.z] {
                        corners.push(frame.project(Vec3 { x, y, z }));
                    }
                }
            }
            frame.boundary = convex_hull(corners);
        }
        let area = frame
            .boundary
            .iter()
            .enumerate()
            .map(|(i, p)| {
                side(
                    frame.origin,
                    *p,
                    frame.boundary[(i + 1) % frame.boundary.len()],
                )
            })
            .sum::<f32>();
        if !area.is_finite() || area <= 1e-6 {
            // A collapsed/invalid frame cannot authorise unclipped drawing.
            frame.boundary.clear();
        }
        frame
    }

    pub fn project(&self, p: Vec3) -> Pos2 {
        let d = [p.x - self.pivot.x, p.y - self.pivot.y, p.z - self.pivot.z];
        self.origin + egui::vec2(dot(d, self.x) as f32, dot(d, self.y) as f32)
    }

    pub fn cloud(&self, painter: &egui::Painter) {
        if self.boundary.len() < 3 {
            return;
        }
        for (fraction, alpha) in [(1.0, 14), (0.985, 8), (0.96, 5)] {
            let outline = self
                .boundary
                .iter()
                .map(|p| self.origin + (*p - self.origin) * fraction)
                .collect();
            painter.add(egui::Shape::convex_polygon(
                outline,
                Color32::from_rgba_unmultiplied(180, 205, 240, alpha),
                egui::Stroke::NONE,
            ));
        }
    }

    /// Triangulate each local cylinder face, never a whole concave arbor.
    /// Work is linear in samples with eight radial faces per segment.
    pub fn tube(&self, points: &[Vec3], radius: f64, colour: Color32) -> Mesh {
        let mut mesh = Mesh::default();
        if !radius.is_finite() || radius <= 0.0 {
            return mesh;
        }
        for pair in points.windows(2) {
            if !pair.iter().all(|p| p.is_finite()) {
                continue;
            }
            let p = [pair[0].x, pair[0].y, pair[0].z];
            let q = [pair[1].x, pair[1].y, pair[1].z];
            let d = std::array::from_fn(|i| q[i] - p[i]);
            let length = dot(d, d).sqrt();
            if length <= 1e-12 {
                continue;
            }
            let tangent = d.map(|v| v / length);
            let axis = if tangent[0].abs() < 0.8 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            let n = cross(tangent, axis);
            let norm = dot(n, n).sqrt();
            let n = n.map(|v| v / norm);
            let b = cross(tangent, n);
            let ring = |point: [f64; 3], side: usize| {
                let angle = std::f64::consts::TAU * side as f64 / 8.0;
                let v: [f64; 3] = std::array::from_fn(|i| {
                    point[i] + radius * (n[i] * angle.cos() + b[i] * angle.sin())
                });
                self.project(Vec3 {
                    x: v[0],
                    y: v[1],
                    z: v[2],
                })
            };
            for side in 0..8 {
                let shade = 0.65 + 0.35 * (std::f32::consts::TAU * side as f32 / 8.0).sin().abs();
                self.polygon(
                    &mut mesh,
                    vec![
                        ring(p, side),
                        ring(q, side),
                        ring(q, side + 1),
                        ring(p, side + 1),
                    ],
                    colour.gamma_multiply(shade),
                );
            }
        }
        mesh
    }

    pub fn disc(&self, mesh: &mut Mesh, p: Pos2, radius: f32, colour: Color32) {
        self.polygon(
            mesh,
            (0..16)
                .map(|i| {
                    let t = std::f32::consts::TAU * i as f32 / 16.0;
                    p + egui::vec2(t.cos(), t.sin()) * radius
                })
                .collect(),
            colour,
        );
    }

    fn polygon(&self, mesh: &mut Mesh, mut polygon: Vec<Pos2>, colour: Color32) {
        if self.boundary.len() < 3 || !polygon.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
            return;
        }
        // Sutherland–Hodgman clips faces without dragging out-of-bounds
        // endpoints onto the boundary, which previously created long spikes.
        for i in 0..self.boundary.len() {
            if polygon.is_empty() {
                return;
            }
            let a = self.boundary[i];
            let b = self.boundary[(i + 1) % self.boundary.len()];
            if polygon.iter().all(|p| side(a, b, *p) >= 0.0) {
                continue;
            }
            if polygon.iter().all(|p| side(a, b, *p) < 0.0) {
                return;
            }
            let mut clipped = Vec::with_capacity(polygon.len() + 1);
            let mut previous = *polygon.last().unwrap();
            let mut prev_dist = side(a, b, previous);
            for current in polygon {
                let dist = side(a, b, current);
                if (dist >= 0.0) != (prev_dist >= 0.0) {
                    clipped
                        .push(previous + (current - previous) * (prev_dist / (prev_dist - dist)));
                }
                if dist >= 0.0 {
                    clipped.push(current);
                }
                previous = current;
                prev_dist = dist;
            }
            polygon = clipped;
        }
        if polygon.len() < 3 {
            return;
        }
        let base = mesh.vertices.len() as u32;
        for p in &polygon {
            mesh.colored_vertex(*p, colour);
        }
        for i in 1..polygon.len() as u32 - 1 {
            mesh.add_triangle(base, base + i, base + i + 1);
        }
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn side(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}
fn convex_hull(mut points: Vec<Pos2>) -> Vec<Pos2> {
    points.retain(|p| p.x.is_finite() && p.y.is_finite());
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    let mut lower = Vec::new();
    let mut upper = Vec::new();
    for p in &points {
        while lower.len() >= 2 && side(lower[lower.len() - 2], lower[lower.len() - 1], *p) <= 0.0 {
            lower.pop();
        }
        lower.push(*p);
    }
    for p in points.iter().rev() {
        while upper.len() >= 2 && side(upper[upper.len() - 2], upper[upper.len() - 1], *p) <= 0.0 {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::morphology_contract::*;

    fn fixture() -> DisplaySnapshot {
        let mut snapshot = DisplaySnapshot::bounded_with_paths(
            1,
            1,
            1,
            1,
            DisplayMode::Anatomical,
            DisplayProvenance::ProceduralAnatomy,
            Some(AxisAlignedBox {
                min: Vec3 {
                    x: -1.0,
                    y: -0.7,
                    z: -0.5,
                },
                max: Vec3 {
                    x: 1.0,
                    y: 0.7,
                    z: 0.5,
                },
            }),
            vec![DisplayNode {
                id: AnatomicalId::new(1, 1).unwrap(),
                role: DisplayRole::Hidden,
                layer: Some(0),
                position_mm: Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                kind: AnatomicalKind::Soma,
                colour_slot: 1,
            }],
            vec![],
            vec![],
            16,
            16,
            None,
        )
        .unwrap();
        snapshot.coverage.membrane = Some(DisplayMembrane {
            centre_mm: Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            radii_mm: Vec3 {
                x: 1.0,
                y: 0.7,
                z: 0.5,
            },
        });
        snapshot
    }

    #[test]
    fn anatomy_shared_fixture_has_repeatable_native_meshes() {
        let snapshot: DisplaySnapshot = serde_json::from_str(include_str!(
            "../../qa/fixtures/morphology/anatomical-stability.json"
        ))
        .unwrap();
        let frame = Frame::new(
            &snapshot,
            egui::pos2(400.0, 300.0),
            egui::vec2(300.0, 300.0),
            -0.77,
            -0.18,
        );
        let mut svg = String::from(
            "<svg xmlns='http://www.w3.org/2000/svg' width='800' height='600'><rect width='800' height='600' fill='#181818'/>",
        );
        let outline = frame
            .boundary
            .iter()
            .map(|p| format!("{},{}", p.x, p.y))
            .collect::<Vec<_>>()
            .join(" ");
        svg.push_str(&format!("<polygon points='{outline}' fill='#30353d'/>"));
        for path in &snapshot.paths {
            let slot = snapshot
                .nodes
                .iter()
                .find(|n| n.id == path.owner)
                .unwrap()
                .colour_slot;
            let variant = if path.kind == AnatomicalKind::Axon {
                0.055
            } else {
                -0.055
            };
            let colour = super::super::display_slot_colour(slot, variant);
            let mesh = frame.tube(&path.points_mm, path.radius_mm, colour);
            assert!(!mesh.indices.is_empty());
            assert_eq!(mesh, frame.tube(&path.points_mm, path.radius_mm, colour));
            for triangle in mesh.indices.chunks_exact(3) {
                let points = triangle
                    .iter()
                    .map(|i| {
                        let p = mesh.vertices[*i as usize].pos;
                        format!("{},{}", p.x, p.y)
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let c = mesh.vertices[triangle[0] as usize].color;
                svg.push_str(&format!(
                    "<polygon points='{points}' fill='rgb({},{},{})'/>",
                    c.r(),
                    c.g(),
                    c.b()
                ));
            }
        }
        for node in &snapshot.nodes {
            let p = frame.project(node.position_mm);
            let c = super::super::display_slot_colour(node.colour_slot, 0.0);
            svg.push_str(&format!(
                "<circle cx='{}' cy='{}' r='7' fill='rgb({},{},{})'/>",
                p.x,
                p.y,
                c.r(),
                c.g(),
                c.b()
            ));
        }
        svg.push_str("</svg>");
        if let Ok(directory) = std::env::var("ANATOMY_QA_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                std::path::Path::new(&directory).join("native-mesh.svg"),
                svg,
            )
            .unwrap();
        }
    }

    #[test]
    fn anatomy_collapsed_viewport_does_not_draw_unclipped_geometry() {
        let snapshot = fixture();
        let frame = Frame::new(
            &snapshot,
            egui::pos2(100.0, 100.0),
            egui::vec2(300.0, 0.0),
            0.0,
            0.0,
        );
        let points = [
            Vec3 {
                x: -20.0,
                y: 0.0,
                z: 0.0,
            },
            Vec3 {
                x: 20.0,
                y: 0.0,
                z: 0.0,
            },
        ];
        assert!(frame.tube(&points, 0.02, Color32::GREEN).indices.is_empty());
    }

    #[test]
    #[ignore = "MORPH-VIS-002: run with cargo xtask qa run --suite anatomical-growth"]
    fn anatomy_captured_growth_meshes() {
        let directory = std::env::var("ANATOMY_QA_DIR").expect("captured growth directory");
        for step in (0..=1200).step_by(100) {
            let snapshot: DisplaySnapshot = serde_json::from_slice(
                &std::fs::read(format!("{directory}/frame-{step:04}.json")).unwrap(),
            )
            .unwrap();
            snapshot.validate().unwrap();
            let frame = Frame::new(
                &snapshot,
                egui::pos2(400.0, 300.0),
                egui::vec2(190.0, 190.0),
                0.0,
                0.0,
            );
            let save = [400, 500, 1200].contains(&step);
            let mut svg = String::from(
                "<svg xmlns='http://www.w3.org/2000/svg' width='800' height='600'><rect width='800' height='600' fill='#181818'/>",
            );
            let mut triangles = 0;
            for path in &snapshot.paths {
                let node = snapshot.nodes.iter().find(|n| n.id == path.owner).unwrap();
                let colour = super::super::display_slot_colour(
                    node.colour_slot,
                    if path.kind == AnatomicalKind::Axon {
                        0.055
                    } else {
                        -0.055
                    },
                );
                let mesh = frame.tube(&path.points_mm, path.radius_mm, colour);
                assert_eq!(mesh, frame.tube(&path.points_mm, path.radius_mm, colour));
                triangles += mesh.indices.len() / 3;
                for vertex in &mesh.vertices {
                    assert!(vertex.pos.x.is_finite() && vertex.pos.y.is_finite());
                    for i in 0..frame.boundary.len() {
                        assert!(
                            side(
                                frame.boundary[i],
                                frame.boundary[(i + 1) % frame.boundary.len()],
                                vertex.pos
                            ) >= -0.05,
                            "{step}: mesh escaped membrane"
                        );
                    }
                }
                if save {
                    for triangle in mesh.indices.chunks_exact(3) {
                        let points = triangle
                            .iter()
                            .map(|i| {
                                let p = mesh.vertices[*i as usize].pos;
                                format!("{:.2},{:.2}", p.x, p.y)
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        let c = mesh.vertices[triangle[0] as usize].color;
                        svg.push_str(&format!(
                            "<polygon points='{points}' fill='rgb({},{},{})'/>",
                            c.r(),
                            c.g(),
                            c.b()
                        ));
                    }
                }
            }
            if step > 0 {
                assert!(triangles > 0, "growth disappeared at {step}");
            }
            if save {
                for node in &snapshot.nodes {
                    let p = frame.project(node.position_mm);
                    let c = super::super::display_slot_colour(node.colour_slot, 0.0);
                    svg.push_str(&format!(
                        "<circle cx='{}' cy='{}' r='4' fill='rgb({},{},{})'/>",
                        p.x,
                        p.y,
                        c.r(),
                        c.g(),
                        c.b()
                    ));
                }
                svg.push_str("</svg>");
                std::fs::write(format!("{directory}/native-growth-{step:04}.svg"), svg).unwrap();
            }
        }
    }

    #[test]
    fn anatomy_mesh_stays_inside_membrane_at_every_camera_angle() {
        let snapshot = fixture();
        let points = [
            Vec3 {
                x: -20.0,
                y: -8.0,
                z: 0.0,
            },
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            Vec3 {
                x: 20.0,
                y: 6.0,
                z: 0.0,
            },
        ];
        for yaw in [-3.0, -0.77, 0.0, 1.4] {
            for pitch in [-1.2, 0.0, 0.8] {
                let frame = Frame::new(
                    &snapshot,
                    egui::pos2(400.0, 300.0),
                    egui::vec2(310.0, 205.0),
                    yaw,
                    pitch,
                );
                let mut mesh = frame.tube(&points, 0.02, Color32::GREEN);
                frame.disc(&mut mesh, frame.boundary[0], 12.0, Color32::WHITE);
                assert!(!mesh.indices.is_empty());
                for v in &mesh.vertices {
                    assert!(v.pos.x.is_finite() && v.pos.y.is_finite());
                    for i in 0..frame.boundary.len() {
                        assert!(
                            side(
                                frame.boundary[i],
                                frame.boundary[(i + 1) % frame.boundary.len()],
                                v.pos
                            ) >= -0.05,
                            "escaped membrane: {:?}",
                            v.pos
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn anatomy_mesh_does_not_fill_concave_bends_or_bridge_invalid_samples() {
        let mut snapshot = fixture();
        snapshot.coverage.membrane = None;
        let frame = Frame::new(
            &snapshot,
            egui::pos2(0.0, 0.0),
            egui::vec2(100.0, 100.0),
            0.0,
            0.0,
        );
        let p = |x, y| Vec3 { x, y, z: 0.0 };
        let points = [p(-0.6, -0.4), p(-0.6, 0.4), p(0.6, 0.4), p(0.6, -0.4)];
        let mesh = frame.tube(&points, 0.02, Color32::GREEN);
        for triangle in mesh.indices.chunks_exact(3) {
            let centre = triangle
                .iter()
                .map(|i| mesh.vertices[*i as usize].pos.to_vec2())
                .fold(Vec2::ZERO, |sum, p| sum + p)
                / 3.0;
            assert!(
                centre.x.abs() >= 57.0 || centre.y < -37.0,
                "triangle filled empty centre: {centre:?}"
            );
        }
        let broken = frame.tube(
            &[p(-0.6, 0.0), p(f64::NAN, 0.0), p(0.6, 0.0)],
            0.02,
            Color32::GREEN,
        );
        assert!(broken.indices.is_empty());
        let reversal = frame.tube(
            &[p(-0.6, 0.0), p(0.6, 0.0), p(-0.6, 0.0)],
            0.02,
            Color32::GREEN,
        );
        assert!(reversal.vertices.iter().all(|v| v.pos.y.abs() <= 2.01));
    }

    #[test]
    fn anatomy_snapshot_sequence_and_truncation_do_not_move_geometry() {
        let snapshot = fixture();
        let frame = Frame::new(
            &snapshot,
            egui::pos2(400.0, 300.0),
            egui::vec2(300.0, 200.0),
            -0.77,
            -0.18,
        );
        let points = [
            Vec3 {
                x: -0.5,
                y: -0.1,
                z: 0.1,
            },
            snapshot.nodes[0].position_mm,
        ];
        let reference = frame.tube(&points, 0.01, Color32::GREEN);
        for sequence in 2..100 {
            let mut next = snapshot.clone();
            next.sequence = sequence;
            next.coverage.complete = sequence % 2 == 0;
            next.coverage.truncated = !next.coverage.complete;
            let current = Frame::new(
                &next,
                egui::pos2(400.0, 300.0),
                egui::vec2(300.0, 200.0),
                -0.77,
                -0.18,
            );
            assert_eq!(reference, current.tube(&points, 0.01, Color32::GREEN));
            assert_eq!(
                current.project(points[1]),
                current.project(next.nodes[0].position_mm)
            );
        }
    }
}
