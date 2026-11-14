# sunlyt

Spacecraft solar-array power simulator, written in Rust.

You give it a trajectory table (Sun, Earth and Moon vectors in the spacecraft body frame, plus the attitude quaternion) and an OBJ model of the spacecraft. It works out how much sunlight reaches every solar cell at every step and how much power the arrays produce.

- **Light:** eclipses, penumbra, the horizon after landing, and the spacecraft shading itself.
- **Circuit:** cells are wired into series strings with bypass and blocking diodes. Each string is solved through a single-diode model fitted to the cell datasheet, at each panel's temperature.
- **Temperature:** integrated over the mission, including heating from the Moon's and Earth's reflected sunlight and infrared.
- **Viewer:** a real-time 3D viewer lets you pause, scrub and play the mission at the batch run's exact numbers.

Status: **slice 7.** See the [design](docs/specs/2026-11-14-solar-power-sim/design.md) and the [build log](docs/specs/2026-11-14-solar-power-sim/progress.md).

## Quick start

These four steps need no mission data. Install Rust (stable 1.95+) from <https://rustup.rs> first; on Windows, use MSVC with the Visual Studio Build Tools.

```bash
cargo build --release
```

```bash
target/release/sunlyt synth -o orbit.csv
```

```bash
target/release/sunlyt check examples/boxwing/scene.toml --svg layout/
```

```bash
target/release/sunlyt view orbit.csv --scene examples/boxwing/scene.toml
```

`synth` writes a synthetic 24 h lunar orbit. `check` shows what the scene file sets up and draws each panel's cells and strings to `layout/`. `view` runs the mission and opens the viewer. `sunlyt sandbox examples/sandbox/sandbox.toml` opens one panel face on, with its shadows and string I-V curves. For numbers instead of a window, run `sunlyt array orbit.csv --scene examples/boxwing/scene.toml -o power.csv`.

## Documentation

| Page | For |
|---|---|
| [Getting started](docs/guide/getting-started.md) | The quick start, step by step, and what each output means |
| [Scene file reference](docs/guide/scene-reference.md) | Every key: OBJ, panels, cell grids, strings, `[electrical]`, `[thermal]`, trajectory mappings |
| [Solar cells](docs/guide/cells.md) | Presets, and writing a cell file from a datasheet or ICD |
| [FAQ](docs/guide/faq.md) | Your own OBJ, picking the panel polygons, placing cells, wiring strings, changing the cell, 0 W panels, frames |
| [AI assistance](docs/guide/ai-assist.md) | Letting an AI assistant edit the configuration safely, with prompts and the check loop |
| [Panel sandbox](docs/guide/sandbox.md) | One panel face on: shadows (rays and exact), the Sun, eclipses, string I-V curves; checking a panel of a full run |

## Commands

| Command | What it does |
|---|---|
| `sunlyt synth -o orbit.csv` | Synthetic lunar-orbit trajectory (`--altitude-km`, `--beta-deg`, `--hours`, `--attitude`, `--sun-axis`) |
| `sunlyt check scene.toml [--svg dir]` | Validate a scene without a trajectory; draw panel layouts |
| `sunlyt inspect traj.csv` | Load a trajectory; row count, span, altitudes, attitude-convention residual |
| `sunlyt array traj.csv --scene scene.toml [-o out.csv]` | The full simulation, one CSV row per trajectory row |
| `sunlyt view traj.csv --scene scene.toml` | The same run in the 3D viewer |
| `sunlyt sandbox file.toml` | One panel face on: shadows, eclipse inset, string I-V curves (`--scene … --panel … --csv … --at …` for a panel of a scene; `--out dir` for CSV) |
| `sunlyt plate traj.csv --normal 0,0,1` | One flat plate, no self-shadowing (quick budget numbers) |
| `sunlyt cell <preset or file.toml>` | Fit a cell datasheet and print or write its I-V curve |

Shared trajectory options: `--mapping`, `--solar-constant`, and `--landed-on`/`--landed-at` (from touchdown on, the horizon is the local ground plane). `--help` on any command lists everything.

## Physics model

- **Irradiance.** S₀ · (1 AU / r)², with S₀ = 1361 W/m² (Kopp & Lean 2011).
- **Incidence.** Cosine law. A panel facing away from the Sun produces nothing.
- **Eclipses.** The fraction of the solar disk hidden by each body's limb, which gives umbra, penumbra and annular phases. The sky's curvature is matched at the limb, so the result is exact to second order across the solar disk; a brute-force spherical test checks this. When two bodies cover the Sun at once, any part of the disk they both cover is counted once.
- **Earth.** WGS-84 oblate spheroid, handled exactly by stretching space along the pole.
- **Moon.** A 1737.4 km sphere.
- **Surface.** After landing, the horizon is the local tangent plane, so terrain above the reference sphere doesn't cause false eclipses.
- **Self-shadowing.** Shadow rays go from n × n points on each cell toward the Sun and are tested against every triangle in the OBJ through a BVH. The Sun is a point for this purpose: its 0.27° radius blurs a shadow edge by about 9 mm per metre of distance, which is small next to a cell. The eclipse dims all cells equally and multiplies the self-shadow fraction.
- **Cells.** Single-diode model, fitted so it passes exactly through the datasheet's Isc, Voc and (Vmp, Imp), with the power maximum at Vmp.
  - Photocurrent scales with the effective irradiance (incidence, eclipse and lit fraction of the cell).
  - Isc and Voc follow the datasheet temperature coefficients.
  - Pmp lands within 1 % of the datasheet's coefficients from 28 to 140 °C.
  - A layout cell of another size is the same technology at its own area.
- **Strings.** Cells in series carry one current, and the string voltage is the sum of the cell voltages.
  - A cell that cannot carry the current (shaded) is held at −Vf by its bypass diode.
  - Each string feeds the bus through a blocking diode.
  - On a fixed bus each string's current is the exact root of V_string(I) = bus + Vf.
  - `mppt` finds each string's global maximum. It is an ideal tracker, an upper bound for any real MPPT.
- **Panel temperature.** One temperature per panel.
  - Heat in: sunlight absorbed on both faces, plus the Moon's and Earth's reflected sunlight and infrared, integrated over the visible disk and checked against the closed-form view factor.
  - Heat out: radiation from both faces, and the electrical power the panel exports.
  - The eclipse is sampled every 2 s inside each row interval.

## Repository layout

The core library has no GPU or UI code, so the CLI, the viewer and the tests all compute with exactly the same functions. Each folder covers one concern.

```
crates/
  core/                 sunlyt-core: headless library
    src/
      math.rs           constants, exact angle and slerp helpers
      trajectory/       CSV mapping, state table, interpolation, synth.rs (synthetic orbits)
      env/              irradiance, eclipses (limbs, overlap)  → Sunlight
      geometry/         OBJ mesh, scene TOML, panels and cell grid, check.rs (reports, SVG)
      shadow/           BVH ray caster (Illuminator trait), brute-force reference
      power/            sunlight → watts: plate, array, cell.rs (I-V fit), circuit.rs (strings)
      thermal/          panel temperature (lumped, RK4), planet.rs: albedo and IR from Moon/Earth
      sim/              whole-mission runs (parallel): plate.rs, array.rs, summaries
      output/           result writers (CSV; Parquet later)
    presets/            built-in CSV mappings and cell datasheets
    tests/              doc TOML check, real-data gates (need SUNLYT_SAMPLE_CSV), speed gate
  cli/                  sunlyt binary, one file per command in commands/
  viewer/               sunlyt-viewer: eframe + wgpu; scene/, timeline/, plots/, ui/, worker.rs
examples/               synthetic scenes (no mission data)
docs/guide/             user documentation
docs/specs/             design discussions and build logs
.claude/skills/         project skills: physics verification, viewer design, configuration help
```

## Development

```bash
cargo fmt --check
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

The real-data tests only run when `SUNLYT_SAMPLE_CSV` points to a trajectory file; otherwise they pass as no-ops. The speed gate (10,000 cells × 12,000 steps, synthetic) is opt-in:

```bash
cargo test --release -p sunlyt-core --test array_speed -- --ignored --nocapture
```

**Never commit mission trajectory files.** `data/private/` is git-ignored for local copies.

Work follows the research → design → build → test workflow in [`docs/specs/`](docs/specs/). Project skills in [`.claude/skills/`](.claude/skills/):

- `sunlyt-physics-verification`: how every physics change is tested.
- `sunlyt-viewer-design`: interaction and motion rules for the viewer.
- `sunlyt-config`: editing scene and cell files and checking them with `sunlyt check`.

## License

[MIT](LICENSE)
