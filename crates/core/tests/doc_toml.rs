//! The docs can't drift from the code: every ```toml block in docs/guide
//! and every .toml under examples/ must parse through the real config
//! types (which reject unknown keys).

use std::path::{Path, PathBuf};

use sunlyt_core::geometry::SceneConfig;
use sunlyt_core::thermal::ThermalConfig;
use sunlyt_core::{CellDatasheet, CsvMapping, ElectricalConfig};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn toml_blocks(md: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut cur: Option<(usize, String)> = None;
    for (n, line) in md.lines().enumerate() {
        let t = line.trim_start();
        if let Some((start, body)) = &mut cur {
            if t.starts_with("```") {
                out.push((*start, std::mem::take(body)));
                cur = None;
            } else {
                // Strip the list indentation of blocks nested in lists.
                body.push_str(line.strip_prefix("   ").unwrap_or(line));
                body.push('\n');
            }
        } else if t.starts_with("```toml") {
            cur = Some((n + 1, String::new()));
        }
    }
    out
}

/// Checks one TOML document by its shape; returns what it was checked as.
fn check(text: &str) -> Result<&'static str, String> {
    let table: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut kinds = Vec::new();
    if table.contains_key("size") {
        // A sandbox file: the viewer crate's `sandbox` test parses these.
        if let Some(e) = table.get("electrical") {
            let mut t = toml::Table::new();
            t.insert("electrical".into(), e.clone());
            ElectricalConfig::from_scene_str(&toml::to_string(&t).unwrap())
                .map_err(|e| format!("[electrical]: {e}"))?;
        }
        return Ok("sandbox");
    }
    if table.contains_key("sun_body") {
        toml::from_str::<CsvMapping>(text).map_err(|e| format!("mapping: {e}"))?;
        return Ok("mapping");
    }
    if table.contains_key("condition") {
        CellDatasheet::from_toml_str(text).map_err(|e| format!("cell: {e}"))?;
        return Ok("cell");
    }
    if table.contains_key("electrical") {
        let cfg = ElectricalConfig::from_scene_str(text)
            .map_err(|e| format!("[electrical]: {e}"))?
            .unwrap();
        // Built-in preset names must exist; file paths are the reader's.
        if !cfg.cell.ends_with(".toml") {
            CellDatasheet::load(&cfg.cell, Path::new(".")).map_err(|e| e.to_string())?;
        }
        kinds.push("electrical");
    }
    if table.contains_key("thermal") {
        ThermalConfig::from_scene_str(text).map_err(|e| format!("[thermal]: {e}"))?;
        kinds.push("thermal");
    }
    if table.contains_key("obj") || table.contains_key("panel") {
        let mut t = table.clone();
        t.remove("electrical");
        t.remove("thermal");
        // Fragments: a panel-only block has no `obj`, a top-level block
        // no `[[panel]]`.
        t.entry("obj").or_insert("x.obj".into());
        t.entry("panel").or_insert(toml::Value::Array(Vec::new()));
        let s = toml::to_string(&t).unwrap();
        toml::from_str::<SceneConfig>(&s).map_err(|e| format!("scene: {e}"))?;
        kinds.push("scene");
    }
    match kinds.first() {
        Some(k) => Ok(k),
        None => Err("not a scene, cell, mapping, [electrical] or [thermal] block".into()),
    }
}

#[test]
fn every_doc_toml_block_parses() {
    let dir = root().join("docs/guide");
    let mut n = 0;
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_some_and(|e| e == "md") {
            let md = std::fs::read_to_string(&p).unwrap();
            for (line, block) in toml_blocks(&md) {
                n += 1;
                if let Err(e) = check(&block) {
                    failures.push(format!("{}:{line}: {e}", p.display()));
                }
            }
        }
    }
    assert!(n >= 10, "found only {n} toml blocks in {}", dir.display());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_example_toml_parses() {
    let mut n = 0;
    for dir in std::fs::read_dir(root().join("examples")).unwrap() {
        for f in std::fs::read_dir(dir.unwrap().path()).unwrap() {
            let p = f.unwrap().path();
            if p.extension().is_some_and(|e| e == "toml") {
                let text = std::fs::read_to_string(&p).unwrap();
                check(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                n += 1;
            }
        }
    }
    assert!(n >= 1);
}
