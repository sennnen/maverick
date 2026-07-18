//! The registry: model string in, device family out. The one resolution rule from the ledger is
//! encoded here rather than scattered through callers: a model string nothing claims resolves to
//! the family marked as the fallback (gen5), because the surveyed lineage stored legacy and
//! wizard-paired models under several spellings and a miss must not strand a real strap.

use crate::codec::{DeviceCodec, ManifestCodec};
use crate::manifest::Manifest;
use mav_model::error::{codes, MavError, Result};
use std::sync::Arc;

type CodecFactory = Box<dyn Fn() -> Box<dyn DeviceCodec> + Send + Sync>;

pub struct RegistryEntry {
    manifest: Arc<Manifest>,
    make_codec: CodecFactory,
}

impl RegistryEntry {
    pub fn manifest(&self) -> &Arc<Manifest> {
        &self.manifest
    }

    pub fn new_codec(&self) -> Box<dyn DeviceCodec> {
        (self.make_codec)()
    }
}

#[derive(Default)]
pub struct Registry {
    entries: Vec<RegistryEntry>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a family whose decoding is fully manifest-driven.
    pub fn register(&mut self, manifest: Manifest) -> Result<()> {
        self.register_with_codec(manifest, || Box::new(ManifestCodec::new()))
    }

    /// Register a family that needs its own codec for stateful or learned behaviour. The decoder
    /// ids the manifest names are checked against what a probe instance of the codec admits, so a
    /// name/module mismatch fails here rather than decoding nothing later.
    pub fn register_with_codec(
        &mut self,
        manifest: Manifest,
        make_codec: impl Fn() -> Box<dyn DeviceCodec> + Send + Sync + 'static,
    ) -> Result<()> {
        manifest.validate()?;
        manifest.validate_against_codec(make_codec().as_ref())?;
        self.entries.push(RegistryEntry {
            manifest: Arc::new(manifest),
            make_codec: Box::new(make_codec),
        });
        Ok(())
    }

    /// Resolve a model string: an exact (trimmed, case-insensitive) match on any family's model
    /// list wins; otherwise the family marked `fallback_for_unknown_models` takes it; otherwise
    /// the model is genuinely unknown and that is a typed error the caller logs.
    pub fn resolve(&self, model: &str) -> Result<&RegistryEntry> {
        let wanted = model.trim().to_lowercase();
        let exact = self.entries.iter().find(|entry| {
            entry
                .manifest
                .identity
                .models
                .iter()
                .any(|m| m.trim().to_lowercase() == wanted)
        });
        if let Some(entry) = exact {
            return Ok(entry);
        }
        self.entries
            .iter()
            .find(|entry| entry.manifest.identity.fallback_for_unknown_models)
            .ok_or_else(|| {
                MavError::new(
                    codes::DECODE_NO_MANIFEST_FOR_MODEL,
                    "no registered manifest matches the model string",
                )
                .context(format!("model {model:?}"))
            })
    }

    pub fn families(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .map(|entry| entry.manifest.identity.family.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(family: &str, models: &[&str], fallback: bool) -> Manifest {
        let models_json = models
            .iter()
            .map(|m| format!("{m:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        Manifest::from_json(&format!(
            r#"{{
                "schema": "connector-manifest/v1",
                "identity": {{
                    "family": "{family}",
                    "display_name": "{family}",
                    "models": [{models_json}],
                    "fallback_for_unknown_models": {fallback}
                }},
                "gatt": {{ "service": "s", "command": "c", "notify": ["n"] }},
                "frame": {{ "wire_format": "gen5", "max_frame_bytes": 8192 }},
                "packets": {{ "35": "command" }},
                "capabilities": ["heart_rate"]
            }}"#
        ))
        .unwrap()
    }

    fn registry() -> Registry {
        let mut r = Registry::new();
        r.register(manifest("whoop4", &["WHOOP 4.0", "4.0"], false))
            .unwrap();
        r.register(manifest(
            "whoop5",
            &["WHOOP 5.0", "5.0", "WHOOP MG", "MG"],
            true,
        ))
        .unwrap();
        r
    }

    #[test]
    fn exact_model_match_wins() {
        let r = registry();
        assert_eq!(
            r.resolve("WHOOP 4.0").unwrap().manifest().identity.family,
            "whoop4"
        );
        assert_eq!(
            r.resolve("WHOOP MG").unwrap().manifest().identity.family,
            "whoop5"
        );
    }

    #[test]
    fn matching_is_trimmed_and_case_insensitive() {
        let r = registry();
        assert_eq!(
            r.resolve("  whoop 4.0 ")
                .unwrap()
                .manifest()
                .identity
                .family,
            "whoop4"
        );
    }

    #[test]
    fn unknown_and_legacy_models_default_to_the_fallback_family() {
        let r = registry();
        // The ledger rule: the legacy seeded row stored just "WHOOP", and wizard spellings vary.
        assert_eq!(
            r.resolve("WHOOP").unwrap().manifest().identity.family,
            "whoop5"
        );
        assert_eq!(r.resolve("").unwrap().manifest().identity.family, "whoop5");
    }

    #[test]
    fn no_fallback_registered_is_a_typed_error() {
        let mut r = Registry::new();
        r.register(manifest("whoop4", &["WHOOP 4.0"], false))
            .unwrap();
        let err = r.resolve("mystery strap").err().unwrap();
        assert_eq!(err.code, codes::DECODE_NO_MANIFEST_FOR_MODEL);
    }

    #[test]
    fn resolved_entry_hands_out_working_codecs() {
        let r = registry();
        let entry = r.resolve("WHOOP 5.0").unwrap();
        let _codec = entry.new_codec();
        assert_eq!(entry.manifest().identity.family, "whoop5");
    }
}
