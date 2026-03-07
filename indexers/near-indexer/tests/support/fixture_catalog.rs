use anyhow::{bail, Context, Result};
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use serde::Deserialize;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use super::fixture_ranges::RANGE_SCENARIOS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureKind {
    Single,
    Range,
}

impl FixtureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Range => "range",
        }
    }
}

impl Ord for FixtureKind {
    fn cmp(&self, other: &Self) -> Ordering {
        use FixtureKind::{Range, Single};

        match (self, other) {
            (Single, Single) | (Range, Range) => Ordering::Equal,
            (Single, Range) => Ordering::Less,
            (Range, Single) => Ordering::Greater,
        }
    }
}

impl PartialOrd for FixtureKind {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleExpectationKind {
    Raw,
    Venear,
}

impl ModuleExpectationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Venear => "venear",
        }
    }

    pub fn from_env(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "raw" => Ok(Self::Raw),
            "venear" => Ok(Self::Venear),
            other => bail!("unsupported MODULE_KIND '{other}', expected raw or venear"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureCase {
    pub id: String,
    pub kind: FixtureKind,
    pub source_paths: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FixtureFile {
    Single(StreamerMessage),
    Many(Vec<StreamerMessage>),
}

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/venear_governance")
}

fn ranges_root() -> PathBuf {
    fixtures_root().join("ranges")
}

pub fn expectation_root() -> PathBuf {
    fixtures_root().join("expectations")
}

fn load_messages_from_path(path: &Path) -> Result<Vec<StreamerMessage>> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read fixture file {}", path.display()))?;
    let parsed: FixtureFile = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to deserialize fixture file {}", path.display()))?;

    Ok(match parsed {
        FixtureFile::Single(message) => vec![message],
        FixtureFile::Many(messages) => messages,
    })
}

fn collect_range_paths(id: &str) -> Result<Vec<PathBuf>> {
    let single_path = ranges_root().join(format!("{id}.json"));
    if single_path.exists() {
        return Ok(vec![single_path]);
    }

    let part_prefix = format!("{id}.part");
    let mut part_paths: Vec<_> = std::fs::read_dir(ranges_root())
        .with_context(|| format!("failed to read fixture ranges dir {}", ranges_root().display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with(&part_prefix) && name.ends_with(".json"))
                .unwrap_or(false)
        })
        .collect();

    part_paths.sort();
    if part_paths.is_empty() {
        bail!(
            "failed to find fixture range file or parts for {id} in {}",
            ranges_root().display()
        );
    }

    Ok(part_paths)
}

pub fn single_fixture_cases() -> Result<Vec<FixtureCase>> {
    let mut cases = Vec::new();
    for entry in std::fs::read_dir(fixtures_root())
        .with_context(|| format!("failed to read fixtures dir {}", fixtures_root().display()))?
    {
        let path = entry
            .with_context(|| {
                format!(
                    "failed to read fixture directory entry under {}",
                    fixtures_root().display()
                )
            })?
            .path();

        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        cases.push(FixtureCase {
            id: stem.to_string(),
            kind: FixtureKind::Single,
            source_paths: vec![path],
        });
    }

    cases.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(cases)
}

pub fn range_fixture_cases() -> Result<Vec<FixtureCase>> {
    let mut cases = RANGE_SCENARIOS
        .iter()
        .map(|scenario| {
            Ok(FixtureCase {
                id: (*scenario).to_string(),
                kind: FixtureKind::Range,
                source_paths: collect_range_paths(scenario)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    cases.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(cases)
}

pub fn all_fixture_cases() -> Result<Vec<FixtureCase>> {
    let mut cases = single_fixture_cases()?;
    cases.extend(range_fixture_cases()?);
    cases.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(cases)
}

pub fn find_fixture_case(id: &str) -> Result<FixtureCase> {
    all_fixture_cases()?
        .into_iter()
        .find(|fixture_case| fixture_case.id == id)
        .with_context(|| format!("unknown fixture id '{id}'"))
}

pub fn load_fixture_messages(case: &FixtureCase) -> Result<Vec<StreamerMessage>> {
    let mut messages = Vec::new();
    for path in &case.source_paths {
        let mut loaded = load_messages_from_path(path)?;
        messages.append(&mut loaded);
    }

    if messages.is_empty() {
        bail!("fixture '{}' resolved to zero messages", case.id);
    }

    messages.sort_by_key(|message| message.block.header.height);
    Ok(messages)
}

pub fn load_all_range_fixture_messages() -> Result<Vec<StreamerMessage>> {
    let mut messages = Vec::new();
    for fixture_case in range_fixture_cases()? {
        let mut loaded = load_fixture_messages(&fixture_case)?;
        messages.append(&mut loaded);
    }
    messages.sort_by_key(|message| message.block.header.height);
    Ok(messages)
}
