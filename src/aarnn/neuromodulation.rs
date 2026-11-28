// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
//! Neuromodulation of synaptic transmission.
//!
//! Dopamine (DA), serotonin (5-HT), noradrenaline (NA; octopamine/tyramine in
//! invertebrates) and acetylcholine (ACh) set how well a presynaptic event
//! crosses the synapse:
//! - presynaptically, through the bouton's release probability (e.g. 5-HT1B
//!   autoreceptors depress release, D1-like and adrenergic receptors
//!   facilitate it);
//! - postsynaptically, through the receptor-mediated efficacy of the drive
//!   that reaches the dendrite (D1/D2, 5-HT1/5-HT2 and adrenergic receptors
//!   scale the response to the same transmitter release).
//!
//! The levels are global pools (volume transmission). The environment drives
//! them:
//! - food (taste, attractant odour) raises DA and 5-HT;
//! - a high energy reserve (satiety) and light raise 5-HT;
//! - heat, noxious input and sensory novelty raise NA;
//! - motor activity raises DA;
//! - explicit reward raises DA;
//! - the network's own modulatory cells (dopaminergic, serotonergic,
//!   noradrenergic/octopaminergic, cholinergic) release into their pool.
//!
//! What the levels mean for behaviour is not wired here: they change
//! transmission and plasticity, and experience decides the rest.
//!
//! Everything is neutral at baseline levels: with no drive, every gain is
//! exactly 1, so networks without modulatory input behave as before.

use serde::{Deserialize, Serialize};

/// Which environmental quantity a sensory channel reports, for neuromodulator drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensoryModality {
    Light,
    Heat,
    Food,
    Noxious,
    Energy,
    Other,
}

impl SensoryModality {
    /// The tag form (`light`, `heat`, `food`, `noxious`, `energy`, `other`).
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Heat => "heat",
            Self::Food => "food",
            Self::Noxious => "noxious",
            Self::Energy => "energy",
            Self::Other => "other",
        }
    }

    /// Parse an explicit tag written by a network builder (`light`, `heat`,
    /// `food`, `noxious`, `energy`, `other`). Returns `None` for any other text.
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag.trim().to_ascii_lowercase().as_str() {
            "light" => Some(Self::Light),
            "heat" => Some(Self::Heat),
            "food" => Some(Self::Food),
            "noxious" => Some(Self::Noxious),
            "energy" => Some(Self::Energy),
            "other" | "" => Some(Self::Other),
            _ => None,
        }
    }
}

/// Classify a sensory channel from its name, region or type label (fallback
/// when the builder did not tag modalities). Order matters: interoceptive and
/// aversive words win over the generic chemical ones.
pub fn classify_sensory_modality(label: &str) -> SensoryModality {
    if let Some(tag) = SensoryModality::from_tag(label) {
        if !label.trim().is_empty() {
            return tag;
        }
    }
    let s = label.to_ascii_lowercase();
    let has = |keys: &[&str]| keys.iter().any(|k| s.contains(k));
    if has(&[
        "energy", "hunger", "satiet", "battery", "fatigue", "charge", "thirst",
    ]) {
        SensoryModality::Energy
    } else if has(&["repel", "noxious", "nocicep", "aversive"]) {
        SensoryModality::Noxious
    } else if has(&["thermo", "heat", "temperat", "warm", "cold"]) {
        SensoryModality::Heat
    } else if has(&[
        "taste", "gustat", "chem", "odour", "odor", "olfact", "food", "nutri", "attract", "sugar",
    ]) {
        SensoryModality::Food
    } else if has(&[
        "light", "photo", "retina", "ocell", "_lum", "lumin", "camera", "irradian", "visual",
        "opsin", "eye",
    ]) {
        SensoryModality::Light
    } else {
        SensoryModality::Other
    }
}

/// The modulator pool a modulatory cell releases into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModulatorKind {
    Dopamine,
    Serotonin,
    Noradrenaline,
    Acetylcholine,
}

/// Classify a hidden cell as modulatory from its type and region names.
/// Dopaminergic labels win (an insect "LC"-named teaching cell in the
/// mushroom-body dopaminergic cluster is dopaminergic); the vertebrate locus
/// coeruleus and invertebrate octopamine/tyramine cells are noradrenergic.
pub fn classify_modulatory_cell(type_name: &str, region: &str) -> Option<ModulatorKind> {
    let s = format!("{type_name} {region}").to_ascii_lowercase();
    let has = |keys: &[&str]| keys.iter().any(|k| s.contains(k));
    if has(&["dopamin", "modulator:da"]) {
        Some(ModulatorKind::Dopamine)
    } else if has(&[
        "serotonin",
        "serotonergic",
        "5ht",
        "5-ht",
        "raphe",
        "modulator:5ht",
    ]) {
        Some(ModulatorKind::Serotonin)
    } else if has(&[
        "noradren",
        "norepin",
        "adrenergic",
        "octopamin",
        "tyramin",
        "locus_coeruleus",
        "modulator:na",
        "modulator:lc",
    ]) {
        Some(ModulatorKind::Noradrenaline)
    } else if has(&[
        "neuromodulatory:acetylcholine",
        "basal_forebrain",
        "modulator:ach",
    ]) {
        Some(ModulatorKind::Acetylcholine)
    } else {
        None
    }
}

/// Global modulator levels (1.0 = default baseline).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModulatorLevels {
    pub dopamine: f64,
    pub serotonin: f64,
    pub noradrenaline: f64,
    pub acetylcholine: f64,
}

impl Default for ModulatorLevels {
    fn default() -> Self {
        Self::splat(1.0)
    }
}

impl ModulatorLevels {
    pub const fn splat(v: f64) -> Self {
        Self {
            dopamine: v,
            serotonin: v,
            noradrenaline: v,
            acetylcholine: v,
        }
    }
    pub const fn zero() -> Self {
        Self::splat(0.0)
    }
}

/// Receptor sensitivity of a cell (or of boutons, for release): the change in
/// efficacy per unit change of each level from its baseline. Positive values
/// facilitate (D1-like, 5-HT2-like, beta-adrenergic), negative depress
/// (D2-like, 5-HT1-like, alpha2-like).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReceptorProfile {
    pub dopamine: f64,
    pub serotonin: f64,
    pub noradrenaline: f64,
}

/// Multiplicative efficacy for one cell or bouton: exactly 1 at baseline,
/// clamped to `[min, max]` (non-finite input gives 1).
pub fn transmission_gain(
    levels: &ModulatorLevels,
    baselines: &ModulatorLevels,
    profile: &ReceptorProfile,
    min: f64,
    max: f64,
) -> f64 {
    let g = 1.0
        + profile.dopamine * (levels.dopamine - baselines.dopamine)
        + profile.serotonin * (levels.serotonin - baselines.serotonin)
        + profile.noradrenaline * (levels.noradrenaline - baselines.noradrenaline);
    if !g.is_finite() {
        return 1.0;
    }
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    g.clamp(lo.max(0.0), hi.max(0.0))
}

/// What drives the levels on one step; each term is 0..1 (a fraction of
/// active cells or a normalised signal).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModulatorDrives {
    pub reward: f64,
    pub food: f64,
    pub light: f64,
    pub heat: f64,
    pub noxious: f64,
    pub energy: f64,
    pub activity: f64,
    pub novelty: f64,
    /// Fraction of each pool's modulatory cells active this step.
    pub cells: ModulatorLevels,
}

/// Gains from drives to levels (level units per unit drive).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DriveGains {
    pub reward_dopamine: f64,
    pub food_dopamine: f64,
    pub food_serotonin: f64,
    pub energy_serotonin: f64,
    pub light_serotonin: f64,
    pub heat_noradrenaline: f64,
    pub noxious_noradrenaline: f64,
    pub activity_dopamine: f64,
    pub novelty_noradrenaline: f64,
    pub cell_release: f64,
}

/// Increments over baseline that the drives ask for (added to the existing
/// signal-driven targets before the levels relax toward them).
pub fn drive_increments(d: &ModulatorDrives, g: &DriveGains) -> ModulatorLevels {
    let c = |x: f64| {
        if x.is_finite() {
            x.clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    ModulatorLevels {
        dopamine: g.reward_dopamine * c(d.reward)
            + g.food_dopamine * c(d.food)
            + g.activity_dopamine * c(d.activity)
            + g.cell_release * c(d.cells.dopamine),
        serotonin: g.food_serotonin * c(d.food)
            + g.energy_serotonin * c(d.energy)
            + g.light_serotonin * c(d.light)
            + g.cell_release * c(d.cells.serotonin),
        noradrenaline: g.heat_noradrenaline * c(d.heat)
            + g.noxious_noradrenaline * c(d.noxious)
            + g.novelty_noradrenaline * c(d.novelty)
            + g.cell_release * c(d.cells.noradrenaline),
        acetylcholine: g.cell_release * c(d.cells.acetylcholine),
    }
}

/// One telemetry sample of the network's neuromodulatory state: global
/// levels against external stimuli and synaptic energy (for the UI graph,
/// the engine activity stream and the `NM_NEUROMOD_TRACE` file).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NeuromodSample {
    pub t_ms: f64,
    pub depth: usize,
    pub dopamine: f64,
    pub serotonin: f64,
    pub noradrenaline: f64,
    pub acetylcholine: f64,
    /// Fraction of sensory cells active (external stimuli reaching the network).
    pub stimuli: f64,
    /// Mean magnitude of chemical synaptic drive delivered per cell (mV/ms)
    /// after modulation: the energy carried across synapses.
    pub synapse_drive: f64,
    /// Mean metabolic energy (ATP) of axons and dendrites, when morphology is present.
    pub synapse_energy: Option<f64>,
    /// Mean synapse activity trace (`Synapse.stimuli`), when morphology is present.
    pub synapse_stimuli: Option<f64>,
    /// Postsynaptic efficacy of a default cell at these levels.
    pub efficacy: f64,
    /// Presynaptic release-probability factor at these levels (1 below depth 2).
    pub release_factor: f64,
    pub drives: ModulatorDrives,
    /// Smoothed population rates (Hz per cell) behind the drives: food, light,
    /// heat, noxious, energy, all sensory, output, DA / 5-HT / NA / ACh cells.
    #[serde(default)]
    pub rates_hz: [f64; 11],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_at_baseline() {
        let p = ReceptorProfile {
            dopamine: 0.4,
            serotonin: -0.25,
            noradrenaline: 0.3,
        };
        let b = ModulatorLevels::default();
        assert_eq!(transmission_gain(&b, &b, &p, 0.25, 3.0), 1.0);
        let none = drive_increments(
            &ModulatorDrives {
                cells: ModulatorLevels::zero(),
                ..Default::default()
            },
            &gains(),
        );
        assert_eq!(none, ModulatorLevels::zero());
    }

    #[test]
    fn dopamine_facilitates_serotonin_depresses_and_clamps() {
        let p = ReceptorProfile {
            dopamine: 0.4,
            serotonin: -0.25,
            noradrenaline: 0.3,
        };
        let b = ModulatorLevels::default();
        let da = ModulatorLevels { dopamine: 1.5, ..b };
        assert!((transmission_gain(&da, &b, &p, 0.25, 3.0) - 1.2).abs() < 1e-12);
        let ht = ModulatorLevels {
            serotonin: 2.0,
            ..b
        };
        assert!((transmission_gain(&ht, &b, &p, 0.25, 3.0) - 0.75).abs() < 1e-12);
        let huge = ModulatorLevels {
            dopamine: 100.0,
            ..b
        };
        assert_eq!(transmission_gain(&huge, &b, &p, 0.25, 3.0), 3.0);
        let nan = ModulatorLevels {
            dopamine: f64::NAN,
            ..b
        };
        assert_eq!(transmission_gain(&nan, &b, &p, 0.25, 3.0), 1.0);
    }

    fn gains() -> DriveGains {
        DriveGains {
            reward_dopamine: 1.0,
            food_dopamine: 0.5,
            food_serotonin: 0.5,
            energy_serotonin: 0.5,
            light_serotonin: 0.5,
            heat_noradrenaline: 0.5,
            noxious_noradrenaline: 0.5,
            activity_dopamine: 0.3,
            novelty_noradrenaline: 0.5,
            cell_release: 1.0,
        }
    }

    #[test]
    fn environment_drives_the_right_pools() {
        let g = gains();
        let food = drive_increments(
            &ModulatorDrives {
                food: 1.0,
                cells: ModulatorLevels::zero(),
                ..Default::default()
            },
            &g,
        );
        assert_eq!(
            (food.dopamine, food.serotonin, food.noradrenaline),
            (0.5, 0.5, 0.0)
        );
        let light = drive_increments(
            &ModulatorDrives {
                light: 1.0,
                cells: ModulatorLevels::zero(),
                ..Default::default()
            },
            &g,
        );
        assert_eq!((light.dopamine, light.serotonin), (0.0, 0.5));
        let heat = drive_increments(
            &ModulatorDrives {
                heat: 1.0,
                cells: ModulatorLevels::zero(),
                ..Default::default()
            },
            &g,
        );
        assert_eq!((heat.noradrenaline, heat.serotonin), (0.5, 0.0));
        let act = drive_increments(
            &ModulatorDrives {
                activity: 1.0,
                cells: ModulatorLevels::zero(),
                ..Default::default()
            },
            &g,
        );
        assert!((act.dopamine - 0.3).abs() < 1e-12);
        let cells = drive_increments(
            &ModulatorDrives {
                cells: ModulatorLevels {
                    dopamine: 0.5,
                    serotonin: 0.0,
                    noradrenaline: 1.0,
                    acetylcholine: 0.2,
                },
                ..Default::default()
            },
            &g,
        );
        assert_eq!(
            (cells.dopamine, cells.noradrenaline, cells.acetylcholine),
            (0.5, 1.0, 0.2)
        );
        // Drives are bounded to 0..1 and non-finite input is ignored.
        let wild = drive_increments(
            &ModulatorDrives {
                reward: 7.0,
                food: f64::NAN,
                cells: ModulatorLevels::zero(),
                ..Default::default()
            },
            &g,
        );
        assert_eq!(wild.dopamine, 1.0);
    }

    #[test]
    fn classification() {
        use SensoryModality::*;
        assert_eq!(
            classify_sensory_modality("hex_s_26_head_camera.on.r00c00"),
            Light
        );
        assert_eq!(classify_sensory_modality("eye_left_lum"), Light);
        assert_eq!(classify_sensory_modality("s_15_chem_left"), Food);
        assert_eq!(
            classify_sensory_modality("s_13_chem_repellent_left"),
            Noxious
        );
        assert_eq!(classify_sensory_modality("s_10_thermo_left"), Heat);
        assert_eq!(classify_sensory_modality("energy_reserve"), Energy);
        assert_eq!(classify_sensory_modality("hex_s_27_ultrasonic_left"), Other);
        assert_eq!(classify_sensory_modality("lumbar_spinal_cord"), Other);
        assert_eq!(classify_sensory_modality("food"), Food);
        use ModulatorKind::*;
        assert_eq!(
            classify_modulatory_cell("Modulator:LC", "mushroom_body_dopaminergic_neurons"),
            Some(Dopamine)
        );
        assert_eq!(
            classify_modulatory_cell("Modulator:LC", "locus_coeruleus"),
            Some(Noradrenaline)
        );
        assert_eq!(
            classify_modulatory_cell("Neuromodulatory:serotonin", "pharynx"),
            Some(Serotonin)
        );
        assert_eq!(
            classify_modulatory_cell("Neuromodulatory:octopamine", "head"),
            Some(Noradrenaline)
        );
        assert_eq!(
            classify_modulatory_cell("Leg:E", "mesothoracic_ganglion_CPG"),
            None
        );
    }
}
