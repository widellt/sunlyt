# Spacecraft solar-array power simulator — design discussion

Status: DRAFT for review · 2026-11-14 · will live at `docs/specs/2026-11-14-solar-power-sim/design.md`

## 1. Current state (research)

Greenfield. No existing code. The input sample is a user-supplied STK report (not in the repo; it may be export-controlled, so it is never committed and its details are kept out of these documents).

| Fact | Value |
|---|---|
| Cadence | Fixed 60 s rows; time format `M/D/YYYY H:MM` UTCG (no seconds) |
| Columns | time; spacecraft position and velocity in Earth-J2000 (km, km/s); quaternion `qx,qy,qz,qs`; geodetic altitude; Earth, Moon and Sun vectors **in the body frame** (km) |
| Scope | Lunar orbit and surface operations (eclipses by the Moon and Earth, a surface horizon) |
| Quaternion | **Verified:** scalar-last, J2000→body, `v_body = R(q)ᵀ · v_J2000` (Earth vector reproduced to a few km over all rows; the other convention is off by hundreds of thousands of km) |
| Quirks | The first row's attitude can differ from the rest; positions can freeze at the end of an ephemeris; `0` sentinels appear in some vector columns |

Notes for design: body-frame vectors alone are enough for geometry (no SPICE needed). Sun distance gives 1/r² irradiance. Low lunar altitude means lunar IR and albedo heat the panels noticeably.

## 2. Desired end state (v1)

1. `sunlit run mission.toml` → per-timestep CSV/Parquet: total power, per-panel and per-string I/V/P, per-cell illumination, panel temperature, eclipse fraction; plus a summary (orbit-average and minimum power, energy per orbit, eclipse durations).
2. `sunlit view mission.toml` → 3D window: spacecraft from OBJ, cells colour-mapped by illumination or current, live Sun/Earth/Moon vectors, timeline scrubber, play/pause, 1×…10⁵× speed, power plots synced to the cursor.
3. The whole sample mission (12k steps, ~10k cells) runs headless in **< 10 s** on a 12-core desktop.
4. Numbers in the viewer come from the same core code as batch output (one source of truth).
5. Validation suite of analytic cases passes in CI. MIT licence. Builds on Windows, Linux and macOS.

## 3. Constraints

- Not Python; performant; concurrent; visual; MIT; private GitHub repo `widellt/sunlit`.
- Trajectory data is user-supplied and possibly export-controlled → examples use a **synthetic** generated trajectory.
- All dependencies MIT/Apache/BSD/zlib-compatible.
- Units: SI internally (m, s, W, V, A, K); km only at the I/O boundary.

## 4. Options and trade-offs

**D1 Language.** (a) **Rust** ✅: speed comparable to C++, data-race-free threads (`rayon`), one-command cross-platform build (`cargo`), good native GPU/UI stack (`wgpu`, `egui`). (b) C++ with Embree and Qt: fastest ray tracer available, but CMake and dependency pain on Windows and fewer memory-safety guarantees for contributors. (c) Julia: fast, but JIT latency and a weak desktop-GUI story. → **Rust.** (Rust 1.8x is already in your WSL; Windows needs `rustup`.)

**D2 Shadow engine** (the core problem).
- (a) **CPU BVH ray casting**: sample points on each cell, cast rays toward the Sun disk. Exact, deterministic, headless, unit-testable.
- (b) GPU shadow maps: very fast, but resolution-limited, aliasing at grazing angles, GPU/driver-dependent numbers, awkward in CI.
- (c) **Sun-direction cache on top of (a)**: for a rigid spacecraft, cell shadowing depends **only on the Sun direction in the body frame**. Precompute or memoize illuminated fractions over a sphere grid (HEALPix, ~0.1°) and interpolate. The mission run then costs almost nothing, and this CSV keeps the Sun on one great circle, so the cache is tiny.
- → **(a) as the truth engine plus (c) as an opt-in accelerator**, validated against (a) in tests. The GPU is used only for display. Articulated arrays invalidate (c) unless the gimbal angle becomes a cache dimension, so tracking arrays fall back to (a).

**D3 Identifying cells in the OBJ.** (a) Every cell as its own OBJ group: tedious, and CAD exports are huge. (b) **The OBJ names panel substrates (`o PANEL_+Y`). A TOML layout generates the cell grid on that face** (rows × cols, pitch, gaps, origin corner, string map). (c) Texture-mask images. → **(b)**, with (a) supported as an override for irregular layouts. Everything else in the OBJ is an occluder.

Slice 3 detail: when a panel object is a slab, its layout gives `normal` to choose the cell side. The grid is centred on that face and must fit inside it. Shadow rays start n×n stratified points per cell. For self-shadowing the Sun is a point; its finite disk is used for eclipses only.
**D4 Electrical model.**
- Cell: single-diode model (Iph, I0, n, Rs, Rsh) fitted to datasheet points (Isc, Voc, Imp, Vmp) at 28 °C AM0, scaled by temperature coefficients and BOL/EOL radiation factors. Two-diode is a later option.
- Series string with a bypass diode per cell (typical of space triple-junction cells) or per *k* cells. Strings are built in parallel into sections, each with a blocking diode.
- Solved by composing piecewise-linear I-V curves: series = add V at equal I; parallel = add I at equal V.
- Regulation per section: `mppt` | `fixed_bus(V)` (DET / sequential shunt) | `pwm`. This choice changes partial-shading results a lot, so it is first-class.
- A partially shaded cell gives photocurrent ∝ irradiance-weighted illuminated area (uniform within the cell).
- As built (slice 4): each string is solved exactly instead of composing piecewise-linear curves. On a fixed bus that's safeguarded Newton on V_string(I) = bus + Vf; under MPPT it's the global maximum per string. Strings on a fixed bus are independent. There is one bypass diode per cell, and `pwm` is not implemented. See progress.md.

**D5 Optical / environment.**
- Irradiance = S₀ (configurable, default 1361 W/m²) · (1 AU / r)².
- Incidence loss: cos θ × an angle-of-incidence curve (Kelly cosine / cover-glass AR table).
- Eclipse by Moon/Earth (and any listed body): fractional solar-disk overlap (circle–circle), giving correct penumbra. When several bodies cover the disk at once, any part they both cover counts once. The disk's visible fraction dims every cell equally and multiplies each cell's self-shadow lit fraction; the two are never subtracted from each other.
- Surface ops: the body you stand on clips the horizon (v1 is a sphere; DEM later).

**D6 Thermal.** (a) Fixed temperature. (b) **Per-panel lumped transient**: m·c·dT/dt = α·S_abs + planetary IR + albedo − ε_f·σT⁴ − ε_b·σT⁴ − P_elec. Moon IR from the surface temperature with a subsolar-cosine model. (c) Full thermal network: out of scope. → **(b)**, since lunar IR at 100 km altitude shifts Voc by several percent; (a) remains an option.
As built (slice 5):
- Inputs are per unit panel area, and the back radiates to space or to a set sink. Albedo and IR are a quadrature over the visible disk.
- Pass A runs in parallel over rows (rays, heat inputs, and the eclipse sampled every 2 s inside each interval). Pass B runs in parallel over panels and sequentially in time: RK4, with the circuit solved at the panel's temperature.
- Layout cells are scaled to their own area, which keeps electrical power below absorbed power. See progress.md.

**D7 Time and interpolation.** The CSV is at 60 s, but the viewer wants smooth sub-step playback. Rotate body vectors to J2000 with q, interpolate in inertial space (Sun/Moon/Earth move slowly there), slerp q, then rotate back. Naive linear interpolation of body vectors would be wrong at 6°/min roll rates.

**D8 Concurrency.**
- Pass A (geometry and illumination) is stateless per step → `rayon` parallel over timesteps, or over cells inside one step for the live viewer.
- Pass B (thermal + electrical) is sequential in time because temperature carries state, but costs O(panels + strings) per step, so it is negligible.
- The viewer runs the simulation on a worker thread that writes into a time-indexed result store. The UI thread reads it lock-free (`arc-swap`/channels). Scrubbing jumps to cached results, or recomputes one step on demand (milliseconds).

**D9 Viewer stack.** (a) **`eframe`/`egui` + a custom `wgpu` 3D pass**: plots via `egui_plot`, one binary, could later compile to WASM. (b) Bevy: a full engine, heavy, with a breaking API churn every release. (c) A Rerun.io log viewer: fastest to get running, but little control over interaction. → **(a).**

Viewer look and motion follow the project skill `.claude/skills/sunlit-viewer-design` (Emil Kowalski's design-engineering principles adapted to egui): no animation on scrubbing or keyboard actions, data never eased, chrome motion under 300 ms with strong ease-out, perceptually uniform colour maps, tabular numbers.

**D10 Input generality.** Canonical internal `StateTable { t, sun_body, bodies: [{name, vec_body, equatorial/polar radius, pole}], q_bi }`. The attitude quaternion is **required**. Mapping a CSV to it is configured in TOML (column names, units, quaternion convention, time format), with an STK preset matching this file. SPICE/ANISE input comes later.

## 5. Resolved decisions

- Rust workspace, MIT, edition 2024 — speed + safe concurrency + easy builds.
- The core library has no GPU or UI dependency — the CLI, CI and future bindings reuse it.
- CPU ray casting is the truth engine; the GPU is for display only — reproducible numbers.
- Config is TOML; outputs are CSV (default) and Parquet (feature flag) — human-diffable and analysis-friendly.
- Cell presets ship as data files citing public datasheets (parameters only).
- The sample CSV stays outside the repo; `.gitignore` blocks `data/private/`.

- Name `sunlit`, private repo `widellt/sunlit` (user, 2026-11-14).
- v1 arrays are **fixed, body-mounted**. There are no gimbals, so the Sun-direction cache (D2c) is always valid. The scene graph stays one rigid body, but `Panel` keeps a `transform` field so joints can be added later.
- v1 regulation default is **`FixedBus(28.0)`**. The real vehicle uses MPPTs whose properties are unknown; `Regulation::Mppt` stays in the enum and becomes the v2 target once MPPT characteristics are available.
- Battery and load energy balance are **v2**, planned as an integration with the user's Kairos tool. v1 exposes power vs. time output for it to consume.
- Development and the viewer run on **native Windows** (rustup installed 2026-11-14). CI also covers Linux.
- **Attitude is required** (user, 2026-11-14). Without a quaternion the rotation between rows is unknown, so a mapping with no `[attitude]` is rejected when it is parsed.
- **Earth is the WGS-84 oblate spheroid** (a = 6378.137 km, b = 6356.752 km, pole = J2000 +Z). **The Moon is a sphere** (1737.4 km): lunar flattening of ~0.0012 changes the occulting limb by about 2 km at most. Each body carries `pole_b` so eclipse code (slice 2) can treat the spheroid exactly.
- Times are UTC with no leap-second handling (user, 2026-11-14).

## 6. Open questions for you

None blocking v1. Later: MPPT characteristics (efficiency curve, tracking algorithm, per-section or per-array); the Kairos interface format.

## 7. Structure outline

```
sunlit/                        (cargo workspace, MIT)
  crates/core/src/             no GPU/UI deps; one folder per concern
    math.rs           AU_M, AngleExt::angle_exact, slerp
    trajectory/       mapping.rs  CsvMapping (TOML, presets)
                      table.rs    StateTable::from_csv_path / row(i) / sample(t) -> Epoch { t, sun_b, bodies, q_bi }
    env/              irradiance.rs  irradiance(sun_dist_m, s0)
                      occultation.rs Limb::of(body, landed); Limb::occultation; visible_fraction (union, overlap counted once)
                      mod.rs      sunlight(epoch, &EnvConfig) -> Sunlight { irradiance, sun_dir_b, occulted[], visible }
    power/            plate.rs    FlatPlate::sample (slice 2)
                      cell.rs     CellModel::iv(g, t) -> IvCurve (slice 4)
                      circuit.rs  Regulation, Network::solve (slice 4)
    geometry/         Scene::load(obj, layout) -> panels, cells (slice 3)
    shadow/           trait Illuminator; RayCaster { bvh }; SunDirCache (slice 3)
    thermal/          PanelThermal::step (slice 5)
    sim/              PlateRun::compute (rayon over rows), summary(), plate_at(t) for the viewer
    output/           csv.rs (now), parquet.rs (feature flag, slice 7)
  crates/cli/src/              main.rs + commands/{inspect, plate, run, validate, bake_cache}.rs
  crates/viewer/src/           (slice 6) eframe + wgpu, depends on sunlit-core only
    scene/            mesh upload, cell colouring, Sun/Earth/Moon markers
    timeline/         scrubber, play/pause/speed, eclipse bands
    plots/            power and I-V plots (dataviz skill rules)
    ui/               theme tokens, motion.rs easing curves (viewer-design skill)
    worker.rs         SimWorker: time-indexed result store, lock-free reads
  crates/core/presets/         mappings/*.toml, cells/*.toml (datasheet-cited)
  examples/cubesat_lunar/      synthetic trajectory generator + OBJ + mission.toml
  docs/specs/2026-11-14-solar-power-sim/
  .claude/skills/              sunlit-physics-verification, sunlit-viewer-design
```

### Vertical slices

| # | Scope | Test gate |
|---|---|---|
| 1 | Repo, CI (fmt, clippy, test on Win/Linux), `StateTable` CSV + STK preset, interpolation (D7) | Load your CSV locally; Earth-vector reconstruction ≤5 km; interpolated midpoint within 0.1° of the slerp reference |
| 2 | Flat-plate power, no shadows: one panel, cos law, 1/r², eclipse fraction | Analytic plate test exact to 1e-9; rows over 50 % hidden equal rows with the Sun centre behind the limb; plot of power vs. time on the sample mission |
| 3 | OBJ + layout → cells, BVH ray caster, self-shadowing | Cube shadowing a plate: illuminated fraction matches the analytic value to <0.5%; 10k cells × 12k steps < 10 s |
| 4 | Cell single-diode model + strings, bypass diodes, MPPT/fixed-bus | Fitted I-V passes through datasheet Isc/Voc/Pmp within 1%; half-shaded string behaviour matches a hand-computed case |
| 5 | Thermal lumped model + lunar IR | Steady-state panel temperature matches the closed-form radiative balance within 0.5 K |
| 6 | Viewer: 3D scene, cell colouring, timeline, pause/scrub, speed, plots | Manual: pause in eclipse ingress and see the penumbra dim; scrub = numbers equal batch output |
| 7 | Sun-direction cache, Parquet, summary report, synthetic example, README | Cache vs. ray caster max error <0.5% per cell; example runs from a clean clone |

Slices 1–2 are mechanical (medium effort). Slices 3–4 and 6 need judgment (high effort).
