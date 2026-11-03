use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;

use crate::{
    cache::{ArtifactCache, write_private},
    error::Result,
    forms::{self, FormPayload, FormProfile},
    model::{Event, OutputMode, RiskLayout},
    risk,
};

#[derive(Clone, Debug)]
pub struct Options {
    pub mode: OutputMode,
    pub layout: RiskLayout,
    pub destination: PathBuf,
    pub force: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Output {
    pub kind: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Generated {
    pub outputs: Vec<Output>,
    pub payload: Option<FormPayload>,
}

pub fn generate(
    source: &Path,
    options: &Options,
    cache: &ArtifactCache,
    profile: &FormProfile,
) -> Result<Generated> {
    let event = Event::load(source)?;
    let mut prepared = Vec::new();
    let mut payload = None;
    if options.mode.form() {
        let form = forms::payload(&event, profile, Utc::now())?;
        let bytes = forms::preview_json(&form)?;
        prepared.push((
            "form".into(),
            options
                .destination
                .join(format!("{}.form.json", event.output_name)),
            bytes,
        ));
        payload = Some(form);
    }
    if options.mode.risk() {
        let layouts = match options.layout {
            RiskLayout::Form => &[RiskLayout::Form][..],
            RiskLayout::Pack => &[RiskLayout::Pack][..],
            RiskLayout::Both => &[RiskLayout::Form, RiskLayout::Pack][..],
        };
        for layout in layouts {
            let key = cache.key("risk-docx", &(&event, layout))?;
            let bytes = cache.get_or_build(&key, "docx", || risk::document(&event, *layout))?;
            let (kind, suffix) = match layout {
                RiskLayout::Form => ("risk-form", ""),
                RiskLayout::Pack => ("risk-pack", "-pack"),
                RiskLayout::Both => unreachable!("both is expanded before generation"),
            };
            prepared.push((
                kind.into(),
                options
                    .destination
                    .join(format!("{}{suffix}.docx", event.output_name)),
                bytes,
            ));
        }
    }
    for (_, path, _) in &prepared {
        if path.exists() && !options.force {
            return Err(crate::error::invalid(format!(
                "Refusing to replace existing output {}",
                path.display()
            )));
        }
    }
    let outputs = prepared
        .into_iter()
        .map(|(kind, path, bytes)| {
            write_private(&path, &bytes, options.force)?;
            Ok(Output { kind, path })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Generated { outputs, payload })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::*;

    #[test]
    fn generates_form_and_both_risk_layouts() {
        let root = std::env::temp_dir().join(format!("soc-event-{}", Uuid::new_v4()));
        let output = root.join("output");
        let cache = ArtifactCache::new(root.join("cache"), 16 * 1024 * 1024).expect("cache");
        let result = generate(
            Path::new("tests/movie-night.toml"),
            &Options {
                mode: OutputMode::Both,
                layout: RiskLayout::Both,
                destination: output,
                force: false,
            },
            &cache,
            &FormProfile::bundled().expect("profile"),
        )
        .expect("event outputs");
        assert_eq!(result.outputs.len(), 3);
        assert!(result.outputs.iter().all(|output| output.path.is_file()));
        assert!(result.payload.is_some());
        fs::remove_dir_all(root).expect("clean output");
    }
}
