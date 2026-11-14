//! `sunlyt synth`: write a synthetic lunar-orbit trajectory CSV (no
//! mission data) in the `stk-body-vectors` layout, for examples and docs.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use glam::DVec3;
use sunlyt_core::{SynthAttitude, SynthOrbit};

#[derive(clap::Args)]
pub struct Args {
    /// Output CSV.
    #[arg(short, long)]
    output: PathBuf,
    /// Circular orbit altitude above the Moon, km.
    #[arg(long, default_value_t = 100.0)]
    altitude_km: f64,
    /// Angle between the orbit plane and the Sun line, degrees. 0 gives the
    /// longest eclipse each orbit; above about 71° (at 100 km) there is none.
    #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
    beta_deg: f64,
    /// Length, hours.
    #[arg(long, default_value_t = 24.0)]
    hours: f64,
    /// Row spacing, minutes.
    #[arg(long, default_value_t = 1)]
    step_min: u32,
    /// `inertial` (body axes = J2000) or `sun` (a body axis tracks the Sun).
    #[arg(long, default_value = "sun")]
    attitude: String,
    /// With `--attitude sun`: the body axis pointed at the Sun, e.g. `+Z`
    /// or `0,0.6,0.8`.
    #[arg(long, default_value = "+Z", allow_hyphen_values = true)]
    sun_axis: String,
    /// Start time, UTC `YYYY-MM-DDTHH:MM:SS`.
    #[arg(long, default_value = "2030-01-01T00:00:00")]
    start: String,
}

fn axis(s: &str) -> Result<DVec3> {
    let named = match s {
        "+X" | "X" => Some(DVec3::X),
        "-X" => Some(-DVec3::X),
        "+Y" | "Y" => Some(DVec3::Y),
        "-Y" => Some(-DVec3::Y),
        "+Z" | "Z" => Some(DVec3::Z),
        "-Z" => Some(-DVec3::Z),
        _ => None,
    };
    if let Some(a) = named {
        return Ok(a);
    }
    let v: Vec<f64> = s
        .split(',')
        .map(|x| x.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .with_context(|| format!("--sun-axis {s:?}: expected ±X/±Y/±Z or x,y,z"))?;
    match v.as_slice() {
        &[x, y, z] => Ok(DVec3::new(x, y, z)),
        _ => bail!("--sun-axis {s:?}: expected three numbers"),
    }
}

pub fn run(args: &Args) -> Result<()> {
    let attitude = match args.attitude.as_str() {
        "inertial" => SynthAttitude::Inertial,
        "sun" => SynthAttitude::SunPointing {
            axis: axis(&args.sun_axis)?,
        },
        a => bail!("--attitude {a:?}: expected `inertial` or `sun`"),
    };
    let start = chrono::NaiveDateTime::parse_from_str(&args.start, "%Y-%m-%dT%H:%M:%S")
        .with_context(|| format!("--start {:?}: expected YYYY-MM-DDTHH:MM:SS", args.start))?;
    let orbit = SynthOrbit {
        start,
        duration_s: args.hours * 3600.0,
        step_s: 60.0 * args.step_min as f64,
        altitude_km: args.altitude_km,
        beta_deg: args.beta_deg,
        attitude,
    };
    let f = std::fs::File::create(&args.output)
        .with_context(|| format!("creating {}", args.output.display()))?;
    let rows = orbit.write_csv(f)?;
    println!(
        "wrote       {} ({rows} rows, {:.1} min orbit, β {}°, synthetic)",
        args.output.display(),
        orbit.period_s() / 60.0,
        args.beta_deg
    );
    Ok(())
}
