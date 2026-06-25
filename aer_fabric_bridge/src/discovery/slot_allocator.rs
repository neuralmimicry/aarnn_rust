use anyhow::Context;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Default, Serialize, Deserialize)]
struct SlotRegistryFile {
    #[serde(default)]
    node_slots: BTreeMap<String, u16>,
}

pub struct SlotAllocator {
    path: PathBuf,
    slots: RwLock<BTreeMap<Uuid, u16>>,
}

impl SlotAllocator {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Ok(Self {
                path,
                slots: RwLock::new(BTreeMap::new()),
            });
        }

        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read slot registry '{}'", path.display()))?;
        let parsed: SlotRegistryFile = toml::from_str(&raw)
            .with_context(|| format!("failed to parse slot registry '{}'", path.display()))?;
        let mut slots = BTreeMap::new();
        for (uuid_raw, slot) in parsed.node_slots {
            let uuid = Uuid::parse_str(&uuid_raw)
                .with_context(|| format!("invalid UUID '{}' in slot registry", uuid_raw))?;
            slots.insert(uuid, slot);
        }
        Ok(Self {
            path,
            slots: RwLock::new(slots),
        })
    }

    pub fn slot_for(&self, node_uuid: Uuid) -> Option<u16> {
        self.slots.read().get(&node_uuid).copied()
    }

    pub fn assign_slot(&self, node_uuid: Uuid) -> anyhow::Result<u16> {
        if let Some(slot) = self.slot_for(node_uuid) {
            return Ok(slot);
        }
        let next = {
            let slots = self.slots.read();
            let mut candidate = 0u16;
            while slots.values().any(|existing| *existing == candidate) {
                candidate = candidate.saturating_add(1);
            }
            candidate
        };
        self.slots.write().insert(node_uuid, next);
        self.persist()?;
        Ok(next)
    }

    pub fn assign_deterministic_if_empty(&self, node_uuids: &[Uuid]) -> anyhow::Result<()> {
        if !self.slots.read().is_empty() {
            return Ok(());
        }
        let mut ordered = node_uuids.to_vec();
        ordered.sort();
        let mut slots = self.slots.write();
        for (idx, uuid) in ordered.iter().enumerate() {
            slots.insert(*uuid, idx as u16);
        }
        drop(slots);
        self.persist()
    }

    fn persist(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create parent directory for '{}'",
                    self.path.display()
                )
            })?;
        }
        let mut node_slots = BTreeMap::new();
        for (uuid, slot) in self.slots.read().iter() {
            node_slots.insert(uuid.to_string(), *slot);
        }
        let rendered = toml::to_string_pretty(&SlotRegistryFile { node_slots })
            .context("failed to render slot registry TOML")?;
        std::fs::write(&self.path, rendered)
            .with_context(|| format!("failed to write '{}'", self.path.display()))?;
        Ok(())
    }
}
