//! Shared, presentation-only complexity policy for dashboard and FPV views.
//!
//! These stages select how an immutable display snapshot is drawn. They never
//! participate in neural event admission, traversal, timing or commitment.

use crate::morphology_contract::DisplaySnapshot;

pub const VISUALIZATION_POLICY_VERSION: u16 = 1;
pub const AUTO_UPSHIFT_DWELL_MS: u64 = 1_500;

/// Select a bounded p95 value from renderer-only latency samples.
/// Invalid and negative observations are discarded; callers retain the
/// samples in a separately bounded window.
pub fn latency_p95_ms(samples: &[f64]) -> Option<f64> {
    let mut finite = samples
        .iter()
        .copied()
        .filter(|sample| sample.is_finite() && *sample >= 0.0)
        .collect::<Vec<_>>();
    finite.sort_by(f64::total_cmp);
    let index = (((finite.len().checked_sub(1)? as u128) * 95) / 100) as usize;
    finite.get(index).copied()
}

/// Ordered display levels shared by every AARNN user interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum VisualizationStage {
    SyntheticPixels = 1,
    SyntheticConnections = 2,
    SyntheticNeurons = 3,
    AnatomicalPixels = 4,
    AnatomicalStraightEdges = 5,
    AnatomicalBranchingEdges = 6,
    AnatomicalCentreLines = 7,
    AnatomicalVolumes = 8,
    AnatomicalContacts = 9,
}

/// Return the highest presentation stage available to legacy projections.
///
/// Legacy topology can support a rendered anatomical arrangement and straight
/// graph links, but it has no committed neurite radii or volumetric-clearance
/// witness. It must therefore remain below the physical geometry stages.
pub const fn highest_legacy_stage(
    has_anatomical_arrangement: bool,
    has_graph_connections: bool,
) -> VisualizationStage {
    if has_anatomical_arrangement {
        if has_graph_connections {
            VisualizationStage::AnatomicalBranchingEdges
        } else {
            VisualizationStage::AnatomicalPixels
        }
    } else {
        VisualizationStage::SyntheticNeurons
    }
}

impl VisualizationStage {
    pub const ALL: [Self; 9] = [
        Self::SyntheticPixels,
        Self::SyntheticConnections,
        Self::SyntheticNeurons,
        Self::AnatomicalPixels,
        Self::AnatomicalStraightEdges,
        Self::AnatomicalBranchingEdges,
        Self::AnatomicalCentreLines,
        Self::AnatomicalVolumes,
        Self::AnatomicalContacts,
    ];

    pub const fn number(self) -> u8 {
        self as u8
    }

    pub fn from_number(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|stage| stage.number() == value)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::SyntheticPixels => "Synthetic pixels",
            Self::SyntheticConnections => "Synthetic connections",
            Self::SyntheticNeurons => "Synthetic neurons",
            Self::AnatomicalPixels => "Anatomical pixels",
            Self::AnatomicalStraightEdges => "Straight connections",
            Self::AnatomicalBranchingEdges => "Branching connections",
            Self::AnatomicalCentreLines => "Anatomical centre-lines",
            Self::AnatomicalVolumes => "Anatomical volumes",
            Self::AnatomicalContacts => "Synapses and boutons",
        }
    }

    pub const fn uses_anatomical_arrangement(self) -> bool {
        self.number() >= Self::AnatomicalPixels.number()
    }

    pub const fn requires_volumetric_clearance(self) -> bool {
        self.number() >= Self::AnatomicalCentreLines.number()
    }
}

/// Return the highest stage supported by the published snapshots.
///
/// Legacy display snapshots default to no clearance witness, so they cannot
/// accidentally enable a volumetric stage merely because they contain paths.
pub fn highest_supported_stage(
    synthetic: Option<&DisplaySnapshot>,
    anatomical: Option<&DisplaySnapshot>,
) -> Option<VisualizationStage> {
    let has_synthetic = synthetic.is_some_and(|snapshot| !snapshot.nodes.is_empty());
    let Some(anatomical) = anatomical else {
        return has_synthetic.then_some(VisualizationStage::SyntheticNeurons);
    };

    // A verified empty brain has no soma, path or contact to render, but its
    // contact inspection stage is still a valid, explicitly empty view.
    if anatomical.nodes.is_empty() {
        return if anatomical.coverage.complete
            && anatomical.coverage.volumetric_clearance_verified
            && anatomical.coverage.contact_set_verified
        {
            Some(VisualizationStage::AnatomicalContacts)
        } else {
            has_synthetic.then_some(VisualizationStage::SyntheticNeurons)
        };
    }

    let mut highest = VisualizationStage::AnatomicalPixels;
    if !anatomical.edges.is_empty() {
        highest = VisualizationStage::AnatomicalBranchingEdges;
    }
    if anatomical.coverage.volumetric_clearance_verified
        && (!anatomical.paths.is_empty() || anatomical.coverage.contact_set_verified)
    {
        let physical_somas = anatomical.nodes.iter().all(|node| {
            node.soma_radius_mm
                .is_some_and(|radius| radius.is_finite() && radius > 0.0)
        });
        let physical_neurites = anatomical
            .paths
            .iter()
            .all(|path| path.radius_mm.is_finite() && path.radius_mm > 0.0);
        // A centreline alone cannot establish physical clearance. Require the
        // published soma and neurite radii alongside the producer's clearance
        // witness before exposing any volumetric stage.
        if physical_somas && physical_neurites {
            highest = VisualizationStage::AnatomicalVolumes;
            if anatomical.coverage.complete && anatomical.coverage.contact_set_verified {
                highest = VisualizationStage::AnatomicalContacts;
            }
        }
    }
    let _ = has_synthetic;
    Some(highest)
}

/// Pick an automatic stage from zoom and the measured visualisation latency.
///
/// Degrading detail is immediate. Increasing detail is limited to one stage
/// after a sustained dwell, which prevents frame-to-frame oscillation.
pub fn update_auto_stage(
    current: VisualizationStage,
    zoom: f64,
    latency_ms: f64,
    highest_available: VisualizationStage,
    dwell_ms: u64,
) -> VisualizationStage {
    let target = automatic_target(zoom, latency_ms, highest_available);
    if target < current {
        target
    } else if target > current && dwell_ms >= AUTO_UPSHIFT_DWELL_MS {
        VisualizationStage::from_number(current.number() + 1).unwrap_or(current)
    } else {
        current.min(highest_available)
    }
}

/// Resolve one immutable FPV frame without consulting worker scheduling.
pub fn automatic_target(
    zoom: f64,
    latency_ms: f64,
    highest_available: VisualizationStage,
) -> VisualizationStage {
    zoom_stage(zoom)
        .min(latency_stage(latency_ms))
        .min(highest_available)
}

pub fn zoom_stage(zoom: f64) -> VisualizationStage {
    let stage = if !zoom.is_finite() || zoom < 0.45 {
        1
    } else if zoom < 0.65 {
        2
    } else if zoom < 0.90 {
        3
    } else if zoom < 1.20 {
        4
    } else if zoom < 1.55 {
        5
    } else if zoom < 1.95 {
        6
    } else if zoom < 2.50 {
        7
    } else if zoom < 3.20 {
        8
    } else {
        9
    };
    VisualizationStage::from_number(stage).expect("zoom policy stage is bounded")
}

pub fn latency_stage(latency_ms: f64) -> VisualizationStage {
    let stage = if !latency_ms.is_finite() || latency_ms > 80.0 {
        1
    } else if latency_ms > 50.0 {
        2
    } else if latency_ms > 33.0 {
        3
    } else if latency_ms > 24.0 {
        4
    } else if latency_ms > 16.0 {
        5
    } else if latency_ms > 12.0 {
        6
    } else if latency_ms > 8.0 {
        7
    } else if latency_ms > 5.0 {
        8
    } else {
        9
    };
    VisualizationStage::from_number(stage).expect("latency policy stage is bounded")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct PolicyFixture {
        version: u16,
        zoom_upper_bounds: Vec<f64>,
        latency_upper_bounds_ms: Vec<f64>,
        upshift_dwell_ms: u64,
        p95_sample_ms: Vec<f64>,
        expected_p95_ms: f64,
        labels: Vec<String>,
    }

    #[test]
    fn shared_policy_fixture_has_nine_stable_zoom_and_latency_bands() {
        let fixture: PolicyFixture = serde_json::from_str(include_str!(
            "../qa/fixtures/visualization/complexity-policy-v1.json"
        ))
        .unwrap();
        assert_eq!(fixture.version, VISUALIZATION_POLICY_VERSION);
        assert_eq!(fixture.zoom_upper_bounds.len(), 8);
        assert_eq!(fixture.latency_upper_bounds_ms.len(), 8);
        assert_eq!(fixture.upshift_dwell_ms, AUTO_UPSHIFT_DWELL_MS);
        assert_eq!(
            fixture.labels,
            VisualizationStage::ALL.map(|stage| stage.label().to_owned())
        );
        assert_eq!(
            latency_p95_ms(&fixture.p95_sample_ms),
            Some(fixture.expected_p95_ms)
        );
        assert_eq!(
            latency_p95_ms(&[1.0, f64::NAN, -1.0, f64::INFINITY]),
            Some(1.0)
        );
        for (index, boundary) in fixture.zoom_upper_bounds.iter().enumerate() {
            assert_eq!(zoom_stage(*boundary - 1e-9).number(), index as u8 + 1);
        }
        for (index, boundary) in fixture.latency_upper_bounds_ms.iter().enumerate() {
            assert_eq!(latency_stage(*boundary).number(), 9 - index as u8);
        }
    }

    #[test]
    fn auto_policy_degrades_immediately_and_upshifts_one_stage_after_dwell() {
        let current = VisualizationStage::AnatomicalContacts;
        assert_eq!(
            update_auto_stage(current, 1.0, 90.0, current, 10_000),
            VisualizationStage::SyntheticPixels
        );
        assert_eq!(
            update_auto_stage(
                VisualizationStage::AnatomicalPixels,
                4.0,
                4.0,
                current,
                1_499
            ),
            VisualizationStage::AnatomicalPixels
        );
        assert_eq!(
            update_auto_stage(
                VisualizationStage::AnatomicalPixels,
                4.0,
                4.0,
                current,
                1_500
            ),
            VisualizationStage::AnatomicalStraightEdges
        );
    }

    #[test]
    fn legacy_projection_stages_stop_before_unverified_physical_geometry() {
        assert_eq!(
            highest_legacy_stage(false, true),
            VisualizationStage::SyntheticNeurons
        );
        assert_eq!(
            highest_legacy_stage(true, false),
            VisualizationStage::AnatomicalPixels
        );
        assert_eq!(
            highest_legacy_stage(true, true),
            VisualizationStage::AnatomicalBranchingEdges
        );
    }

    #[test]
    fn missing_volumetric_clearance_clamps_available_detail() {
        let stage = update_auto_stage(
            VisualizationStage::AnatomicalBranchingEdges,
            4.0,
            4.0,
            VisualizationStage::AnatomicalBranchingEdges,
            10_000,
        );
        assert_eq!(stage, VisualizationStage::AnatomicalBranchingEdges);
    }

    #[test]
    fn clearance_without_physical_radii_does_not_enable_physical_stages() {
        let first = crate::morphology_contract::AnatomicalId::new(1, 1).unwrap();
        let second = crate::morphology_contract::AnatomicalId::new(2, 1).unwrap();
        let mut anatomical = DisplaySnapshot::bounded_with_paths(
            1,
            1,
            1,
            1,
            crate::morphology_contract::DisplayMode::Anatomical,
            crate::morphology_contract::DisplayProvenance::ProceduralAnatomy,
            None,
            vec![
                crate::morphology_contract::DisplayNode {
                    id: first,
                    role: crate::morphology_contract::DisplayRole::Hidden,
                    layer: Some(0),
                    position_mm: crate::morphology_contract::Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    kind: crate::morphology_contract::AnatomicalKind::Soma,
                    soma_radius_mm: None,
                    colour_slot: 0,
                },
                crate::morphology_contract::DisplayNode {
                    id: second,
                    role: crate::morphology_contract::DisplayRole::Hidden,
                    layer: Some(0),
                    position_mm: crate::morphology_contract::Vec3 {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    kind: crate::morphology_contract::AnatomicalKind::Soma,
                    soma_radius_mm: None,
                    colour_slot: 0,
                },
            ],
            vec![crate::morphology_contract::DisplayEdge {
                source: first,
                target: second,
                points_mm: Vec::new(),
                multiplicity: 1,
                kind: "axon".to_owned(),
            }],
            vec![crate::morphology_contract::DisplayPath {
                id: crate::morphology_contract::AnatomicalId::new(3, 1).unwrap(),
                owner: first,
                kind: crate::morphology_contract::AnatomicalKind::Axon,
                points_mm: vec![
                    crate::morphology_contract::Vec3 {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    crate::morphology_contract::Vec3 {
                        x: 0.5,
                        y: 0.0,
                        z: 0.0,
                    },
                ],
                radius_mm: 0.02,
            }],
            8,
            8,
            None,
        )
        .unwrap();
        anatomical.coverage.volumetric_clearance_verified = true;
        assert_eq!(
            highest_supported_stage(None, Some(&anatomical)),
            Some(VisualizationStage::AnatomicalBranchingEdges),
            "a clearance flag without physical soma radii is not sufficient evidence",
        );
    }

    #[test]
    fn verified_empty_contact_set_supports_stage_nine_without_invented_markers() {
        use crate::morphology_contract::{DisplayMode, DisplayProvenance};

        let mut empty = DisplaySnapshot::bounded_with_paths(
            1,
            1,
            1,
            1,
            DisplayMode::Anatomical,
            DisplayProvenance::ProceduralAnatomy,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            8,
            8,
            None,
        )
        .unwrap();
        assert!(empty.coverage.complete);
        assert_eq!(highest_supported_stage(None, Some(&empty)), None);
        empty.coverage.volumetric_clearance_verified = true;
        assert_eq!(highest_supported_stage(None, Some(&empty)), None);
        empty.coverage.contact_set_verified = true;
        assert_eq!(
            highest_supported_stage(None, Some(&empty)),
            Some(VisualizationStage::AnatomicalContacts)
        );
        assert!(empty.nodes.is_empty());
        assert!(empty.paths.is_empty());
        assert!(empty.markers.is_empty());
        empty.coverage.complete = false;
        assert_eq!(highest_supported_stage(None, Some(&empty)), None);

        let clipped = DisplaySnapshot::bounded_with_paths(
            1,
            1,
            1,
            2,
            DisplayMode::Anatomical,
            DisplayProvenance::ProceduralAnatomy,
            None,
            Vec::new(),
            vec![crate::morphology_contract::DisplayEdge {
                source: crate::morphology_contract::AnatomicalId::new(1, 1).unwrap(),
                target: crate::morphology_contract::AnatomicalId::new(2, 1).unwrap(),
                points_mm: Vec::new(),
                multiplicity: 1,
                kind: "unresolved".to_owned(),
            }],
            Vec::new(),
            8,
            8,
            None,
        )
        .unwrap();
        assert!(!clipped.coverage.complete);
        assert!(clipped.coverage.truncated);
    }

    #[test]
    fn physical_scene_needs_complete_contact_witness_for_stage_nine() {
        use crate::morphology_contract::{
            AnatomicalId, AnatomicalKind, DisplayMode, DisplayNode, DisplayProvenance, DisplayRole,
            Vec3,
        };

        let mut scene = DisplaySnapshot::bounded_with_paths(
            1,
            1,
            1,
            1,
            DisplayMode::Anatomical,
            DisplayProvenance::ProceduralAnatomy,
            None,
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
                soma_radius_mm: Some(0.04),
                colour_slot: 0,
            }],
            Vec::new(),
            Vec::new(),
            8,
            8,
            None,
        )
        .unwrap();
        scene.coverage.volumetric_clearance_verified = true;
        assert_eq!(
            highest_supported_stage(None, Some(&scene)),
            Some(VisualizationStage::AnatomicalPixels)
        );
        scene.coverage.contact_set_verified = true;
        assert_eq!(
            highest_supported_stage(None, Some(&scene)),
            Some(VisualizationStage::AnatomicalContacts)
        );
    }
}
