//! `sunlyt cell`: fit a cell datasheet and show (or write) its I-V curve.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sunlyt_core::power::cell::ZERO_C_K;
use sunlyt_core::{CellDatasheet, CellModel};

#[derive(clap::Args)]
pub struct Args {
    /// Built-in preset name, or a cell TOML path.
    #[arg(default_value = "azur-3g30a-advanced-4x8")]
    cell: String,
    /// Datasheet condition (BOL or a listed fluence).
    #[arg(long, default_value = "BOL")]
    condition: String,
    /// Cell temperature, °C (default: the datasheet's reference).
    #[arg(long)]
    temperature_c: Option<f64>,
    /// Effective irradiance, W/m² (default: the datasheet's reference).
    #[arg(long)]
    irradiance: Option<f64>,
    /// Write the I-V curve (V, I, P) as CSV.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

pub fn run(args: &Args) -> Result<()> {
    let sheet = CellDatasheet::load(&args.cell, Path::new("."))?;
    let model = CellModel::fit(&sheet, &args.condition)?;
    let t_c = args
        .temperature_c
        .unwrap_or(model.reference_temperature_k - ZERO_C_K);
    let g = args.irradiance.unwrap_or(model.reference_irradiance_w_m2);
    let d = model.at_temperature(t_c + ZERO_C_K)?.at(g);
    let r = &model.reference;
    let p = &model.condition;

    println!("cell        {} ({})", model.name, p.name);
    println!("source      {}", sheet.source);
    println!(
        "fit         a = {:.5} V, Rs = {:.4} Ω, I0 = {:.4e} A, Iph = {:.6} A, Rsh = {}",
        r.a_v,
        r.rs_ohm,
        r.i0_a,
        r.iph_a,
        if r.gsh_s == 0.0 {
            "∞".into()
        } else {
            format!("{:.1} Ω", 1.0 / r.gsh_s)
        }
    );
    let (vmp_r, imp_r) = r.mpp();
    println!("            datasheet     model");
    println!("  Isc  A    {:<12.4}  {:.6}", p.isc_a, r.i_at(0.0));
    println!("  Voc  V    {:<12.4}  {:.6}", p.voc_v, r.voc());
    println!("  Vmp  V    {:<12.4}  {:.6}", p.vmp_v, vmp_r);
    println!("  Imp  A    {:<12.4}  {:.6}", p.imp_a, imp_r);
    println!(
        "  Pmp  W    {:<12.4}  {:.6}",
        p.vmp_v * p.imp_a,
        vmp_r * imp_r
    );

    let (vmp, imp) = d.mpp();
    println!(
        "at {t_c:.1} °C, {g:.1} W/m²: Isc {:.4} A, Voc {:.4} V, Pmp {:.4} W at {:.4} V, \
         efficiency {:.2} %",
        d.i_at(0.0),
        d.voc(),
        vmp * imp,
        vmp,
        100.0 * vmp * imp / (g * model.area_m2)
    );

    if let Some(path) = &args.output {
        let f =
            std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
        let mut w = std::io::BufWriter::new(f);
        writeln!(w, "v_v,i_a,p_w")?;
        let voc = d.voc();
        for k in 0..=400 {
            let v = voc * k as f64 / 400.0;
            let i = d.i_at(v);
            writeln!(w, "{v:.6},{i:.6},{:.6}", v * i)?;
        }
        w.flush()?;
        println!("wrote       {}", path.display());
    }
    Ok(())
}
