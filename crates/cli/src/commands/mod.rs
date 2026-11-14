//! One file per subcommand. Shared arguments and printing live here.

pub mod array;
pub mod cell;
pub mod check;
pub mod inspect;
pub mod plate;
#[cfg(feature = "viewer")]
pub mod sandbox;
pub mod synth;
#[cfg(feature = "viewer")]
pub mod view;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use sunlyt_core::{CsvMapping, EnvConfig, RunSummary, StateTable, Surface};

/// A built-in preset name or a path to a mapping TOML file.
pub fn load_mapping(spec: &str) -> Result<CsvMapping> {
    if let Some(m) = CsvMapping::preset(spec) {
        return Ok(m);
    }
    let path = PathBuf::from(spec);
    if !path.exists() {
        bail!(
            "mapping {spec:?} is neither a preset ({}) nor a file",
            CsvMapping::PRESETS.join(", ")
        );
    }
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {spec}"))?;
    toml::from_str(&text).with_context(|| format!("parsing {spec}"))
}

/// Trajectory and environment options shared by every power command.
#[derive(clap::Args)]
pub struct TrajectoryArgs {
    /// Trajectory CSV.
    pub csv: PathBuf,
    /// Column mapping: a built-in preset name or a path to a TOML file.
    #[arg(long, default_value = "stk-body-vectors")]
    pub mapping: String,
    /// Irradiance at 1 AU, W/m².
    #[arg(long, default_value_t = sunlyt_core::env::SOLAR_CONSTANT_W_M2)]
    pub solar_constant: f64,
    /// Body the spacecraft lands on (needs --landed-at).
    #[arg(long, requires = "landed_at")]
    pub landed_on: Option<String>,
    /// UTC touchdown time, `YYYY-MM-DDTHH:MM:SS`. From then on the horizon
    /// is the local tangent plane.
    #[arg(long, requires = "landed_on")]
    pub landed_at: Option<String>,
}

impl TrajectoryArgs {
    pub fn load(&self) -> Result<StateTable> {
        let map = load_mapping(&self.mapping)?;
        Ok(StateTable::from_csv_path(&self.csv, &map)?)
    }

    pub fn env(&self, table: &StateTable) -> Result<EnvConfig> {
        let mut env = EnvConfig {
            solar_constant_w_m2: self.solar_constant,
            ..Default::default()
        };
        if let (Some(body), Some(at)) = (&self.landed_on, &self.landed_at) {
            let index = table
                .body_index(body)
                .with_context(|| format!("no body {body:?} in the table"))?;
            let at = chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%dT%H:%M:%S")
                .with_context(|| format!("--landed-at {at:?}: expected YYYY-MM-DDTHH:MM:SS"))?;
            let from_t = (at - table.epoch()).num_milliseconds() as f64 / 1000.0;
            env.surface = Some(Surface {
                body: index,
                from_t,
            });
        }
        Ok(env)
    }
}

/// Eclipse counts and the power summary, shared by every power command.
pub fn print_summary(table: &StateTable, s: &RunSummary) {
    println!("rows        {}", s.rows);
    for (k, name) in table.body_names().enumerate() {
        println!(
            "{name:<11} umbra {} rows, penumbra {} rows",
            s.umbra[k], s.penumbra[k]
        );
    }
    if s.rows > 0 {
        println!(
            "power       mean {:.3} W, min {:.3} W, max {:.3} W",
            s.mean_w, s.min_w, s.max_w
        );
    }
    println!("energy      {:.3} Wh", s.energy_wh);
}

/// Wall-clock phases for `--timings`.
pub fn print_timings(phases: &[(&str, Duration)]) {
    let parts: Vec<String> = phases
        .iter()
        .map(|(n, d)| format!("{n} {:.1} ms", d.as_secs_f64() * 1e3))
        .collect();
    println!("timings     {}", parts.join(", "));
}
