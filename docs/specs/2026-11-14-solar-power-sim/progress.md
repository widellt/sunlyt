# Build log

## Slice 1: workspace, CSV input, interpolation (2026-11-14)

**Changed**
- Cargo workspace with `crates/core` (`sunlit-core`) and `crates/cli` (`sunlit`). The CLI has one command: `sunlit inspect <csv> [--mapping preset|file.toml]`.
- `mapping.rs`: TOML-driven `CsvMapping` (time column and format, length unit, Sun and body columns, quaternion order and direction, optional inertial position for cross-checks). Built-in preset `stk-body-vectors`.
- `input.rs`: `StateTable` (SI units), `sample(t)` following D7 (rotate to inertial, lerp, slerp the attitude, rotate back), `attitude_residual()`, `subset()`.
- CI: fmt, clippy `-D warnings` and test, on Ubuntu and Windows.

**Gate** (WSL, real CSV via `SUNLIT_SAMPLE_CSV`; the file is not in the repo)
- Loads a 60 s-cadence STK report. ✅
- Attitude residual: max a few km against the 5 km gate (relative 1e-5). The STK convention is confirmed. ✅
- Leave-one-out (predict odd rows from even rows): Sun, Earth and Moon direction errors far below the 0.1° median gate. The single worst point falls in an attitude slew that the 60 s cadence cannot resolve, a data limit rather than a model error.
- Unit tests: 10/10, including exact spin interpolation (<1e-9 rad), a quaternion sign flip, and detection of a wrong convention.

**Deviations**
- glam's `DQuat::slerp` and `DVec3::angle_between` use an approximate `acos` (~2e-8 rad error). Added an exact `slerp` and `AngleExt::angle_exact`; shadow code must use these.
- Presets live in `crates/core/presets/` rather than at the repo root, so `include_str!` stays inside the crate.
- On the surface, the landed altitude sits well above the reference sphere (terrain). Surface-horizon modelling (slice 2) should use the local radius, not the mean radius.
- Windows toolchain: the GNU target needs a MinGW assembler that it doesn't ship. Switched to MSVC with VS 2022 Build Tools. The local gate ran in WSL in the meantime.

### Slice 1 review changes (2026-11-14)
- `attitude` in `CsvMapping` is now mandatory. Removed the linear fallback for body-frame vectors, and `Epoch::q_bi` is no longer an `Option`.
- Bodies are oblate spheroids: `radius_km` (equatorial), optional `polar_radius_km`, `pole_inertial` (default +Z). `BodyState` exposes `pole_b`, `surface_radius_m()` and `altitude_m()`. Earth uses WGS-84; the Moon stays a sphere.
- Check: the computed Earth altitude matches STK's geodetic altitude to within 0.1 km at its maximum. The unit tests and the real-data gate pass natively on Windows (MSVC).

## Slice 2: flat-plate power and eclipses (2026-11-14)

**Changed**
- `env.rs`: `irradiance` (S₀ · (1 AU/r)², S₀ = 1361 W/m² default), `sunlight(epoch, cfg)` giving the per-body occulted fraction and the visible fraction of the solar disk. `disk_occultation(a, b, c)` uses circle–circle overlap with the limb replaced by a plane circle of radius tan b, which matches the spherical cap's edge position and curvature (exact to second order across the solar disk). For b → 90° it uses a straight-edge segment.
- The oblate Earth is exact: space is stretched by a/b along `pole_b`, so the spheroid becomes a sphere of radius a. The solar disk keeps its true angular radius, an error under 0.34 %.
- Surface ops: `EnvConfig::surface = Some(Surface { body, from_t })` makes the horizon the local tangent plane from touchdown on (radius = |r|, applied in stretched space, so it is also the spheroid's tangent plane). The same horizon is used automatically below the reference surface.
- `plate.rs`: `FlatPlate { normal_b, area_m2, efficiency }` and `sample()`. P = irradiance · A · η · max(0, cos θ) · visible.
- CLI: `sunlit plate <csv> [--normal --area --efficiency --solar-constant --landed-on --landed-at -o]` writes a per-row CSV and prints a summary.
- README rewritten. Added project skills `.claude/skills/sunlit-physics-verification` (verification rules and reference cases for every slice) and `.claude/skills/sunlit-viewer-design` (Emil Kowalski's principles adapted to egui, for slice 6).

**Gate** (native Windows, real CSV via `SUNLIT_SAMPLE_CSV`)
- Analytic plate exact to 1e-9 relative at 0°, 45°, 60° and 89°, at 0.983–1.017 AU. ✅
- Disk overlap: closed-form cases to 1e-12. The curved-limb approximation agrees with brute-force spherical sampling (600×600) to < 2e-3 at b = 0.3, 1.2 and 1.5 rad. The oblate limb grazing the pole hides 0.5 ± 0.005, against > 0.6 for an equatorial sphere. ✅
- Real data: the rows with more than 50 % of the disk hidden are exactly the rows with the Sun centre inside the limb; no Earth eclipses. ✅
- Power vs. time: `target/sunlyt-gate/plate_power.svg`, written by the gate (not committed).
- Unit tests: 22/22 core; clippy `-D warnings` clean.

**Deviations**
- Bodies are assumed not to overlap each other on the solar disk (visible = 1 − Σ occulted, clamped). A simultaneous Earth and Moon eclipse never occurs in scope.
- Landing is not detected from the data: terrain above the reference sphere makes an altitude threshold unreliable, and the table has no Moon-fixed velocity. Touchdown is given explicitly with `--landed-at`.

### Slice 2 review changes (2026-11-14)

**Modular layout** (ahead of the viewer). The core is now one folder per concern:
- `math`, `trajectory/{mapping,table}`, `env/{irradiance,occultation}`, `power/plate`, `sim`, `output/csv`.
- The CLI has one file per command in `commands/`.
- Root re-exports keep the public paths unchanged.
- Run statistics and CSV writing moved from the CLI into `sim` and `output`, so the viewer reuses them.
- The README and design §7 show the target tree, including `crates/viewer/{scene,timeline,plots,ui}`.

**Several bodies on the disk.** The user asked that bodies be assumed to cover different parts of the disk, with tests for the case where they cover the same part twice.
- `visible_fraction` = 1 − (Σ fᵢ − excess). The excess is the part of the disk hidden more than once, weighted by the extra count, integrated over 4,096 sunflower points. It is computed only when two or more bodies touch the disk; when they cover separate parts it is exactly 0.
- New tests:
  - separate parts add exactly;
  - an identical body twice counts once;
  - a body inside another's shadow adds nothing;
  - a double umbra gives 0;
  - a horizon plus a body matches brute force;
  - partial overlap matches brute-force union sampling (< 3e-3);
  - max fᵢ ≤ hidden ≤ Σ fᵢ over 40 configurations.
- `Sunlight` now carries `sun_dist_m`.
- Design D5 states that the disk fraction multiplies the per-cell self-shadow fraction (slice 3); the two are never subtracted from each other.

**Performance** (a 12k-row trajectory, release, native Windows):

| Phase | Time |
|---|---|
| CSV load | ≈ 30 ms |
| Physics, serial | 3.5 ms (290 ns/row) |
| Physics, parallel | **0.9 ms** (rayon over rows) |
| CSV write | ≈ 16 ms |

- Parallel and serial results are identical bit for bit (gate test).
- Physics is now under 5 % of a run, so the remaining time is I/O. Parquet output (slice 7) and a faster parser are the levers if that ever matters.
- Recursion was considered and rejected: Rust has no guaranteed tail calls, so a loop is at least as fast and cannot overflow the stack.
- Sorting rows only pays when it enables reuse. That is the Sun-direction cache in slice 3, where rows are binned by Sun direction and results are returned in time order.
- New `sunlit plate --timings` flag.

**Skills.**
- Viewer: a latency budget (visible response ≤ 200 ms, target next frame), plus plot rules from the `dataviz` skill (one axis, validated palette, crosshair, legend, CSV export).
- Physics: reference cases for several bodies on the disk and for eclipse × self-shadow, plus rules for performance changes.

- Tests: 28/28 core unit tests; real-data gate unchanged.

## Slice 3: OBJ, cells, BVH self-shadowing (2026-11-14)

**Changed**
- `geometry/`:
  - `Mesh` loads OBJ with `tobj` (MIT, `use_f64`); every triangle remembers its `o`/`g` object.
  - `SceneConfig` is the TOML layout (D3b): `obj`, `scale`, `samples`, and `[[panel]]` entries with `object`, optional `normal`, `rows`, `cols`, `cell`, `gap`, optional `u_axis` and `offset`.
  - `Scene::build` finds each panel's cell face. With `normal` given it takes only the triangles facing that way, so a slab works; without it the object must be planar and facing one way. It centres the grid on the face, rejects any cell that hangs off the edge (naming the row and column), and places n×n stratified sample points lifted by 1e-7 × scene size.
  - `shapes::ObjWriter` writes quads, boxes and spheres for tests, benchmarks and examples.
- `shadow/`:
  - The `Illuminator` trait.
  - `RayCaster`: a binned-SAH BVH in f64, built and traversed with explicit stacks (no recursion), any-hit Möller–Trumbore, double-sided triangles.
  - `BruteForce`: the reference caster that the BVH must match bit for bit.
- `power/array.rs`: `ArrayModel` with constant efficiency. Per-panel power and sunlit fraction. Rays are skipped when the Sun is fully eclipsed.
- `sim/` split into `plate.rs` and `array.rs` with a shared `RunSummary`. `ArrayRun::compute` runs rows in parallel, reusing one scratch buffer per thread. `array_at(t)` returns the per-cell lit fractions for the viewer.
- `output::write_array_csv`. CLI `sunlit array --scene`. Trajectory and environment options are now shared by all commands (`TrajectoryArgs`).
- `examples/boxwing/`: synthetic bus, two wings and a mast (960 cells).

**Gate**
- Cube over plate (0.4 m cube above a 1 m plate, 100×100 cells, 2×2 rays) against the exact projected-shadow area (convex hull clipped to the plate), at five Sun angles: max relative error **0.21 %** (gate < 0.5 %). ✅
- BVH = brute force bit for bit over 60 Sun directions in a cluttered scene (bus, wings, sphere, mast). ✅
- Unshaded cell array = flat plate to 1e-12. Eclipse × self-shadow multiply. Umbra gives exactly 0. ✅
- **Speed** (synthetic, no mission data): 10,000 cells × 4 rays, 2,240 triangles, 12,000 steps in **0.90 s** (75 µs/step; 39 % of steps are in umbra and skipped). Gate < 10 s. ✅ Parallel = serial bit for bit.
- Real trajectory with `examples/boxwing`: compute well under a second; the bus and mast cost about a tenth of the energy.
- Tests: 41 core unit tests, 3 real-data gate tests, 1 opt-in speed gate. Clippy clean.

**Decisions and deviations**
- The Sun is a point for self-shadowing (blur ≈ 9 mm per metre from the occluder). The finite disk stays modelled for eclipses only. Revisit if a validation case needs soft shadow edges.
- Lit fraction counts sample points, so a cell's partial shadow is quantised to 1/n². The panel-level error averages out (the cube gate uses n = 2).
- Per-cell lit fractions are not stored for every row, since rows × cells is too large. The viewer recomputes one step on demand (`array_at`). For 10k cells that takes an estimated 1–2 ms on one core, inside one 16 ms frame and well within the 200 ms feedback budget.
- A `cell_area_m2` mismatch between a layout and its substrate is not an error, only cells off the edge are. Overlapping panels are not detected.

## Slice 4: cells, strings, regulation (2026-11-14)

**Changed**
- `power/cell.rs`:
  - `CellDatasheet` (TOML, built-in presets) and `CellModel::fit`. The fit gives the single-diode (Iph, I0, a, Rs) for a given Rsh (default ∞), passing exactly through Isc, Voc and (Vmp, Imp) with dP/dV = 0 at Vmp. It uses nested bisection, which cannot diverge and runs once per run.
  - `DiodeParams`: `i_at` (Lambert W, Jain & Kapoor 2004), `v_at` (explicit in I), `i_at_newton` (reference), `mpp`.
  - `lambert_w_exp(l)` = W(eˡ) without forming eˡ.
  - `at_temperature`: Isc and Voc follow the datasheet coefficients, a ∝ T, I0 is chosen to hit Voc(T). `at(G)`: Iph and Gsh scale with irradiance.
- Preset `presets/cells/azur-3g30a-advanced-4x8.toml`: AZUR SPACE DB 00010891-01 (11-Apr-2025), CIC columns. BOL, 2.5E14, 5E14 and 1E15 (1 MeV e⁻/cm²), with temperature coefficients and the bypass diode Vf ≤ 0.8 V.
- `power/circuit.rs`:
  - `Circuit` (cell at temperature, bypass Vf, blocking Vf, `Regulation::FixedBus { bus_v } | Mppt`).
  - `string_v(groups, I)` = Σ max(V_cell, −Vf_bypass).
  - Fixed bus: safeguarded Newton for V_string(I) = bus + Vf_blocking. The blocking diode makes a string that cannot reach the bus give 0.
  - MPPT: golden-section search in each interval between photocurrent levels, which finds the global maximum under partial shading.
  - Cells under equal light are grouped (`group_irradiance`) so a string costs a few evaluations per distinct irradiance.
  - `ElectricalConfig`: the `[electrical]` table of the scene TOML.
- `geometry`: `PanelLayout.strings`, either `{ series = N, order = "rows" | "columns" }` (serpentine walk, cut into strings) or an explicit `[row, col]` list. The default is one string per row. `Scene.strings`, `Panel.strings`. Bad wiring names the cell. `Scene::load` ignores `[electrical]`.
- `ArrayModel` is now `Constant { efficiency } | Circuit(Circuit)`. `ArraySample` gains per-string current and voltage. The CSV gains `current_a_<panel>` with the circuit.
- CLI:
  - `sunlit array` uses the circuit when the scene has `[electrical]`, unless `--efficiency` is given. It prints the cell, wiring and regulation, and warns when layout and datasheet cell areas differ by more than 10 %.
  - New `sunlit cell` prints the fit and the datasheet-vs-model points, and writes an I-V CSV.
- `examples/boxwing`: 4 × 8 cm cells, 15-cell strings (32 per wing), `[electrical]` at 60 °C on a 28 V bus.

**Gate**
- The fit passes through the datasheet Isc, Voc, Vmp, Imp and Pmp for all four conditions. Worst error 2.2e-9, against a 1 % gate. ✅ The MPP is checked with golden-section search, which never sees the slope condition used by the fit.
  - BOL: a = 0.07656 V, Rs = 0.0465 Ω, I0 = 2.49e-16 A.
- Lambert W against Newton: < 1e-12 A from −1 V to Voc + 0.1 V, with Rsh = ∞, 500 Ω and 20 Ω. ✅ V(I(V)) inverts to within the curve's conditioning (ulp(I)·|dV/dI|).
- Golden values:
  - W(1) = Ω, W(e) = 1 and W(e^(e+1)) = e, to 4e-15.
  - Ideal-diode closed forms: Voc = a·ln(Iph/I0 + 1), and Vmp = a·(W(e(Iph+I0)/I0) − 1) agrees with direct bisection of dP/dV to 1e-12.
  - pvlib could not be run (no Python on this machine), so closed forms stand in for its golden values.
- Temperature, 28–140 °C, all conditions:
  - Voc follows the coefficient exactly (1e-12), and Isc to 1e-6.
  - **Pmp is within 0.8 %** of the datasheet's linear coefficients.
  - Vmp and Imp separately drift up to +2.3 % / −1.7 % at 140 °C after 1E15: Rs is held constant, which moves the MPP along the curve while keeping its power. Bounds: Pmp < 1 %, Vmp and Imp < 2.5 %.
  - `sunlit cell --condition 1E15 --temperature-c 80 --irradiance 1361` gives Pmp 0.9149 W, against 0.9137 W from the datasheet coefficients (0.13 %).
- **Half-shaded string = hand calculation:** 12 ideal cells, 6 at half light, 0.8 V bypass, 0.7 V blocking, to 1e-12 A. ✅
  - 8 V bus: the shaded cells are bypassed. Closed form I = Iph − I0(exp((8.7/6 + 0.8)/a) − 1) = 0.408911 A.
  - 26 V bus: all cells conduct. Quadratic in I: 0.234690 A.
- Strings:
  - Series adds V at equal I. Bypass clamps at exactly −Vf. Blocking gives exactly 0, never negative.
  - Over a 0–45 V bus sweep: current never rises, power ≥ 0, and the fixed bus never beats MPPT.
  - MPPT equals a brute-force scan of 200,001 currents to 1e-9, including double-peak cases.
  - Unshaded array = strings × one string, bit for bit. Umbra gives 0 A.
  - A post shading 15 % of a panel costs 27.5 % of its power under MPPT, because whole cells are bypassed.
- **Speed** (synthetic, 10,000 cells, 200 strings, 12,000 steps, release):

  | Model | Time |
  |---|---|
  | Constant | 0.92 s |
  | Fixed bus | 1.16 s |
  | Ideal MPPT | 1.27 s |

  Gate < 10 s. ✅ Parallel = serial bit for bit for all three.
- Real trajectory, `examples/boxwing` (960 cells, 60 °C, BOL): fixed 28 V bus, ideal MPPT and constant 30 % runs.
  - The fixed bus gives about 89 % of MPPT, as expected for strings sized for hot operation (they sit near Isc at 60 °C).
  - MPPT vs constant: the 60 °C efficiency, datasheet vs layout cell area, the blocking diode, and bypass losses under partial shade.
  The real-trajectory numbers are kept in the private development repo.
- Tests: 64 core unit tests, 3 real-data gates and 1 opt-in speed gate. Clippy clean.

**Decisions and deviations**
- **Exact string solve instead of piecewise-linear I-V composition (design D4).** Strings in parallel on a fixed bus are independent, so each is a monotone 1-D root and needs no network solve. This has no discretisation error. MPPT is ideal and per string. A per-section tracker, and piecewise-linear curves for viewer plots, can come with v2's MPPT.
- **One bypass diode per cell**, as on the assembly datasheet. Groups of k cells per diode are not implemented. Each diode is a constant-Vf clamp, so its own I-V curve is neglected.
- **Rsh = ∞ by default**: the datasheet gives no shunt data. A finite `rsh_ohm` in a preset is supported and tested.
- **Temperature is one fixed value** (`temperature_c`, default 28 °C) until the thermal model (slice 5) supplies it per panel and step.
- **Default blocking diode Vf of 0.7 V** is an assumption (typical Si PN near 1 A) and is documented as such.
- No BOL → EOL interpolation between fluences: pick a listed condition.
- **Cell current comes from the datasheet, not the layout rectangle.** The CLI warns when the areas differ by more than 10 %. The speed gate's 1.9 cm cells therefore read high, which is expected.
- **The default wiring walks serpentine.** "One string per row" runs every other row backwards. Order within a series string doesn't change the result.

## Slice 5: panel temperature, lunar and Earth albedo and IR (2026-11-14)

**Changed**
- `thermal/mod.rs`:
  - `ThermalConfig` (`[thermal]` in the scene TOML) and `PanelThermal`. The model is C dT/dt = q_in − ε_f σT⁴ − ε_b σ(T⁴ − T_sink⁴) − p_elec, per unit area.
  - The integrator is RK4. Substeps are at most a quarter of the radiative time constant and are aligned with the input samples (`step`, `step_with`, `step_n`).
  - `steady_k` gives the closed-form steady state. Temperatures are floored at 2.7 K.
  - `ThermalModel::absorbed` returns `PanelHeat { direct (per unit visible disk), planet }`. The front absorbs α_f on the lit cell fraction. An exposed back gets α_b direct sunlight. IR is absorbed with ε (Kirchhoff).
- `thermal/planet.rs`:
  - `BodyRadiation`:
    - Moon: Bond albedo 0.12, ε 0.95 (Vasavada 2012), night side 95 K (Williams 2017). The day side is in subsolar radiative equilibrium.
    - Earth: albedo 0.29, OLR 240 W/m² (CERES, Loeb 2018).
  - `add_irradiance`: a 32 × 64 equal-solid-angle quadrature over the visible disk. Radiance is computed once per direction and shared by all plates.
  - `view_factor_sphere`: Howell's closed form, used as the check.
- `sim/array.rs`: `ArrayRun::compute_thermal` (and `_serial`).
  - Pass A, parallel over rows: rays, string irradiance groups, heat per panel, and the visible disk every ≤ 2 s inside the interval to the next row (`ECLIPSE_SAMPLE_S`).
  - Pass B, parallel over panels and sequential in time. At each row the circuit is solved at the panel temperature (reported). The next row's power is predicted at the same temperature. The interval is then integrated with direct sun and P_elec per unit visible disk, both linear between rows and scaled by the sampled visible fraction. An umbra row, where rays were skipped, borrows its neighbour's value. Planet heat is linear between rows.
  - The default start is the steady state of the first row, iterated with P_elec(T).
- `power/array.rs`: `ArrayModel::illuminate` (rays, groups) and `panel_output` (circuit at a temperature) split the old `sample`, which is unchanged in behaviour. `ArraySample.panel_temperature_k` is new.
- `power/cell.rs` and `circuit.rs`:
  - `CellTempRef` (Copy) and `Circuit::with_temperature` re-evaluate the cell at any temperature.
  - The cell model is never evaluated below `MIN_CELL_TEMPERATURE_K` (70 K).
  - **Layout cells are scaled to their own area** (`CellAtTemp::scaled`, `Circuit::for_cell_area`): Iph, I0 and Gsh ∝ area, Rs ∝ 1/area. See the decisions below.
- `geometry`: `Panel.area_m2` (face area). `Scene::load` ignores `[thermal]`.
- CLI: `sunlit array` reads `[thermal]`. It prints the thermal properties, names bodies with no albedo/IR model, and prints each panel's min/max/mean temperature. The CSV gains `temp_c_<panel>`.
- `examples/boxwing`: `[thermal]` with free-standing wings.
- `array_at(…, panel_temp_k)` lets the viewer evaluate the circuit at a run's temperatures.

**Gate**
- **Steady state = closed form:** 336.6661 K against 336.6661 K at normal incidence (gate 0.5 K), integrated from −100 °C in 60 s rows. ✅ The `PanelThermal` unit test agrees to 1e-6 K at 0°, 45° and 75°.
- **Energy balance with P_elec:** residual 2.5e-11 W/m². Loaded panel 44.2 °C against 63.5 °C open-circuit. Starting from steady state is already settled. ✅
- **Eclipse cooling** against the exact solution T = (T0⁻³ + 3kt)^(−1/3): worst 4.5e-5 K over 2 h. ✅
- **RK4 order:** in the asymptotic range (256 → 512 substeps) the error ratio is 19.7 (2⁴ = 16). At coarse steps the T⁴ stiffness inflates the ratio (41.6 → 22.1 from 4 to 256 substeps); measured and explained in the test.
- **Planet flux:**
  - Quadrature against Howell's view factor: worst 3.8e-4 of the nadir value over tilt 0–180° and d/R from 1.02 to 220. ✅
  - Albedo and IR through an enclosing sphere equal A·S·πR² and (1 − A)·S·πR² to 1e-5. ✅
  - At 100 km over the Moon, a plate facing the surface gets 1038 W/m² at local noon and 3.3 W/m² at midnight.
- **Lunar heating:** in a synthetic 100 km orbit the Moon raises the mean panel temperature by 19.6 K.
- **Cadence:**
  - Before sub-row eclipse sampling, 120 s rows against 60 s rows differed by **16 K** at eclipse ingress. The panel time constant (~100 s) is shorter than the rows, and it cools at ~1 K/s.
  - With 2 s samples, the 2-minute difference is 2.8 K and the 4-minute difference 11.1 K: ratio 3.96, second order.
  - What remains is linear trajectory interpolation between rows. The chord sags 2.7 km off a 100 km orbit at 120 s, which moves ingress by a second or two.
  - Extrapolated, 60 s data carries about ±1 K at ingress and egress. A follow-up could interpolate the trajectory with Hermite splines.
- **Area scaling:** I(V) scales exactly by r (1e-12) at r = 0.12, 1.06 and 2.65. Voc and Vmp are unchanged.
- **Parallel = serial**, bit for bit, for the thermal run.
- **Speed** (10,000 cells, 200 strings, 12,000 steps, release, idle machine):

  | Model | Time |
  |---|---|
  | Constant | 1.02 s |
  | Fixed bus | 1.32 s |
  | MPPT | 1.40 s |
  | Fixed bus + thermal | 2.63 s |
  | MPPT + thermal | 3.52 s |

  Gate < 10 s. ✅ Earlier the same build measured MPPT at 3.5–4.9 s while the CPU was 57 % busy with other programs (`main` measured the same then). Timings on this machine vary about 3× with background load.
- **Real trajectory, `examples/boxwing`** (fixed 28 V bus, BOL): thermal with and without planets, and a fixed 60 °C run.
  - The Moon's albedo and IR raise the peak wing temperature by tens of kelvin.
  - Energy barely changes: strings sized for hot operation sit near Isc on a fixed bus, which hardly depends on temperature.
  The real-trajectory numbers are kept in the private development repo.
- Tests: 76 core unit tests, 5 thermal run gates, 3 real-data gates and 1 opt-in speed gate. Clippy clean.

**Decisions and deviations**
- **Cell area scaling (reverses a slice-4 choice).** Slice 4 gave every layout cell the datasheet cell's full current and only warned on a mismatch. With temperature coupled, that broke energy conservation: the speed gate's 3.6 cm² cells exported more power than their panels absorbed and froze to 3 K, where the cell model overflowed to infinite power. A layout cell is now treated as the same technology at its own area. This moves the boxwing example by +6 % (32 cm² against the datasheet's 30.18 cm²).
- **One temperature per panel.** No gradients and no conduction or radiation between panels or to the bus. All panels share one set of thermal properties for now.
- **The back face and the planets are not shadowed by the spacecraft.** The front's non-cell substrate is assumed lit like the cells.
- **The planet model** uses the equatorial radius for Earth, no surface thermal inertia (the night floor stands in for it), and the solar flux at the spacecraft for the planet.
- **Electrical power between rows** is per unit visible disk and linear, then scaled by the sampled eclipse. It is a first-order stand-in for the circuit's nonlinear response during the ~10 s of penumbra.
- **Below 70 K** the cells are evaluated at 70 K.
- **Default thermal properties** (C = 1800 J/(m²·K), ε 0.85/0.85, back α 0.90) are typical values, documented as assumptions. Front α 0.91 is the cell datasheet's bound.

## Slice 6: viewer (2026-11-14)

Design: [slice-6-viewer.md](slice-6-viewer.md), approved with the I-V plot deferred and CLI flags instead of `mission.toml`.

**Changed**
- `crates/viewer` (`sunlit-viewer`, a library): eframe/egui 0.37 with its own wgpu 30 pass, laid out as `worker.rs`, `clock.rs`, `scene/`, `timeline/`, `plots/` and `ui/`.
  - **Workers:** the run worker computes the whole run once with a progress bar and publishes it through `ArcSwapOption`. The instant worker evaluates only the newest requested time; older requests are dropped. At a row time the viewer shows the batch `ArraySample` itself. Between rows it uses `array_at` with panel temperatures linear between rows.
  - **3D view:** offscreen 4× MSAA colour + depth, shown as an egui image. Cells are unlit flat colour (the colour is data) in viridis for effective illumination or cividis for string current, with a legend and units. The structure is lit by a headlight plus the real Sun dimmed by the eclipse. Sun, Earth and Moon direction lines are labelled, and body axes are drawn. The camera runs on critically damped springs, with presets Sun and ±X/Y/Z, double-click to reset, and a reduced-motion setting.
  - **Timeline:** painted transport icons (the bundled fonts have no media glyphs), speeds 1×…10⁵× with 60× default, and keys Space, ←/→, Shift+←/→, Home/End and [ ]. The scrubber has umbra and penumbra bands, resampled every 0.5 s around eclipse edges because 60 s rows miss ~10 s penumbrae. The hover readout shows UTC and shade.
  - **Plots:** stacked power, temperature and visible charts sharing the time axis, linked cursors, a snapping tooltip, click to seek, the dataviz reference palette in fixed order, and CSV export using the batch CSV's columns and number formats.
  - **Capture mode** for checks and docs (env vars, see `ui/capture.rs`): screenshot to PNG, jump to a time or the n-th penumbra, scrub sweep with frame timing.
- CLI: `sunlit view <csv> --scene … [--efficiency]`, behind the default-on `viewer` feature. `--no-default-features` builds a headless CLI, and CI checks it.
- Core:
  - `ArrayRun::compute_progress` / `compute_thermal_progress`, where the existing functions use a throwaway counter.
  - `Cell.size_m`.
  - `output::csv::column_safe` is now public.
  - `geometry/mesh.rs` uses `as_chunks` (a new clippy lint in Rust 1.100).
- Workspace `rust-version` 1.85 → 1.95 (egui_plot 0.38's minimum).
- CI installs the X11/Wayland dev packages on Ubuntu.

**Gate**
- **Penumbra dims (manual, screenshot):** real trajectory, boxwing, mid-ingress between two rows: every cell dims uniformly with the visible disk, and the structure shading dims too. ✅
- **Scrub = batch:**
  - `array_at_rows_equal_batch` (core): at every row of a thermal run with eclipses, `array_at(t_i, T_i)` equals the batch sample bit for bit, and the same holds without thermal. ✅
  - The viewer shows the batch sample at rows, and the readout matches the batch CSV there. ✅
- **Export = batch:** `export_matches_batch` checks that `t_s`, `utc`, `visible`, `power_w`, `power_w_<panel>` and `temp_c_<panel>` are identical strings to `write_array_csv`'s columns. ✅
- **Feedback budget:** a 600-frame scrub across the whole mission (release build, 3D view on, 960 cells).

  | Measure | Median | p95 | Max |
  |---|---|---|---|
  | UI CPU per frame | 6.1 ms | 11.6 ms | 17.4 ms |
  | Instant worker | 0.05 ms | 0.73 ms | 1.55 ms |

  No frame showed a stale instant. The 200 ms budget holds with a wide margin. The ≤ 16 ms next-frame aim holds at p95, and the worst frame is 17.4 ms.
- Run in the viewer: 1.2–1.7 s for a 12k-row trajectory.
- Tests: core 76 unit + 6 thermal-run + 3 real-data + 1 opt-in speed; viewer 7 unit + 1 integration. Clippy `-D warnings` and fmt are clean.

**Decisions and deviations**
- **Cell colour is unlit:** cells are flat data colour so they match the legend. Shape comes from the lit structure around them.
- **Penumbra bands come from resampling:** 0.5 s sampling inside intervals whose ends differ. This is sunlight only, with no rays, so it is cheap.
- **The plots draw batch rows only:** the cursor and readout show sub-row values, but the plotted lines are the batch run.
- **Not built:** the I-V plot (deferred by the user); cell picking or hover in 3D; reading the OS reduced-motion setting (egui doesn't expose it, so it is a toggle that defaults to off); a file dialog for export (it writes to the working directory and shows the path).
- **Palette:** the dataviz reference palette is used as published (validated there against #fcfcfb / #1a1a19 surfaces). Its validator was not rerun.

## Slice 7: rename to sunlyt, synth, check, user guide (2026-11-14)

Design: [slice-7-docs-sandbox.md](slice-7-docs-sandbox.md). Approved: the repo and folder are renamed too; the sandbox moves to slice 8 (with box occluders plus a from-scene mode); the cache, Parquet and summary move to slice 9; one cell type per array stays. Open for slice 8: which cell/string fields the user's ICD specifies.

**Changed**
- **Rename:** `sunlit` → `sunlyt` everywhere outside `docs/specs/` (crates `sunlyt-core` and `sunlyt-viewer`, binary `sunlyt`, env vars `SUNLYT_*`, skills `sunlyt-*`). The GitHub repo is now `widellt/sunlyt` (GitHub redirects the old URL), cloned at `dev/sunlyt`. The specs keep the old name as history.
- `trajectory/synth.rs` + `sunlyt synth`: a circular lunar orbit written in the `stk-body-vectors` layout. Options: altitude, β, duration, step in whole minutes, and an inertial or Sun-pointing attitude (a chosen body axis toward the Sun, with roll fixed by the orbit normal). The Sun is 1 AU along +X and the Earth at quarter phase along +Y, so Earth never eclipses.
- `geometry/check.rs` + `sunlyt check scene.toml [--svg DIR]`:
  - the OBJ group list with roles (panel or occluder);
  - per panel: normal, u/v, face size, grid size and offset, margin, and strings;
  - `[electrical]` with one full-sun string per length against the bus, with a warning when Vmp < bus + blocking Vf;
  - `[thermal]` presence;
  - a layout SVG per panel: the face, cells coloured per string, wiring paths from a numbered first cell, and the u/v axes.
- `docs/guide/`: getting-started, scene-reference (every key with its unit and default, plus trajectory mappings), cells (template, where the datasheet numbers are, unit conversions), faq (the user's six questions plus 0 W panels, frames, irregular layouts, speed and mission data), and ai-assist (what to give an assistant, four prompts, the check loop).
- `README.md` is now a quick start, a docs table, a command table, the physics summary and the layout.
- `.claude/skills/sunlyt-config`: the edit → `sunlyt check --svg` → report loop.
- `crates/core/tests/doc_toml.rs`: every TOML block in `docs/guide/` and every `.toml` in `examples/` parses through the real config types.
- Viewer: the plot bands are unnamed (egui_plot printed a truncated "u…" in every band). The timeline legend still labels umbra and penumbra.
- `.gitignore`: the quick-start outputs.

**Gate**
- **Rename:** the test count is unchanged after the rename (93 passed, 1 ignored). `git grep -i sunlit` outside `docs/specs/` finds nothing.
- **Clean clone** of the branch: `cargo build --release`, then `synth -o orbit.csv` (1,441 rows, 117.8 min orbit), then `check … --svg layout/` (two panels, 480 cells, 32 strings of 15 each, two SVGs), then `array` (555 umbra rows and 5 penumbra rows, mean 602.8 W, 14.46 kWh). `view` opens on the synthetic orbit; screenshot taken mid-penumbra at 51.09 % visible, 501.43 W. ✅
- **Synth:**
  - the attitude residual is < 1 m and the Sun lies exactly on the chosen body axis at every row;
  - umbra rows match the shadow-cylinder estimate asin(R/r)/π within 6 rows of 241;
  - β = 80° gives no eclipse;
  - steps that are not whole minutes are rejected. ✅
- **Check:** a unit test on a known layout gives face 0.6 × 0.4 m, grid 0.53 × 0.19 m, margin 0.035 m, strings [8, 8, 8], and an SVG with 24 cells and 3 paths. On boxwing it reports a 15-cell string at 60 °C delivering 15.54 W at 28.70 V against its 17.28 W maximum at 33.02 V. Two grids on one face work and are reported. ✅
- **Docs:** `doc_toml` passes for 14+ blocks. Planting `colz = 6` in the FAQ makes it fail with "unknown field `colz`". ✅
- Tests: 99 passed, 1 ignored (opt-in speed). Clippy `-D warnings` and fmt are clean, and the headless build works.

**Decisions and deviations**
- `check`'s margin uses the face's bounding box. For non-rectangular faces it is an approximation; the hard fit rule (every corner on a face triangle) is unchanged.
- `synth` writes whole-minute steps because the STK time format has no seconds.
- FAQ "non-rectangular layouts": several grids per face is the documented workaround. Free-form cell positions are not supported.

### Slice 7 addition: SolAero IMM-α preset and the shunt fit (2026-11-14)

The user pointed to the SolAero IMM-α listing on satsearch.co. The linked preliminary datasheet (April 2018) was read for its parameters.

**Changed**
- `presets/cells/solaero-imm-alpha.toml`, defined **per cm²** because the sheet gives current densities and no cell size; the layout's cell size scales it.
  - BOL is as printed: Voc 4.78 V, Jsc 10.66 mA/cm², Vmp 4.28 V, Jmp 10.12 mA/cm², AM0 135.3 mW/cm².
  - 5E14, 1E15 and 5E15 are BOL × the sheet's remaining factors. The products reproduce its Pmp factors (0.911/0.872/0.714 against 0.91/0.87/0.71).
  - Temperature coefficients are as printed (marked "projected" on the sheet).
  - Assumed and flagged in the file: the 28 °C test temperature (not printed) and the 0.8 V bypass default (no Vf given).
- **Cell fit fallback** (`power/cell.rs::fit_point_shunt`). The typical points are squarer than a single diode with Rs ≥ 0 and an ideal shunt allows: with Rs = 0 and no shunt, the curve through Isc, Voc and (Vmp, Imp) peaks at ~4.23 V, below the printed 4.28 V. When the series fit is infeasible and the file sets no `rsh_ohm`, the fit now sets Rs = 0 and solves for Rsh. This is the same five-parameter model (design D4), with four conditions pinned exactly. The AZUR fits are unchanged: the fallback never runs for them, and a test asserts that.
- `sunlyt check`: when a fixed-bus string's Vmp sits far above the bus (it delivers < 80 % of its maximum), it prints the share and the string length that would put Vmp just above the bus.
- Docs: the preset table, per-cm² datasheets, EOL from remaining factors, the shunt fit, and "a new cell usually needs a new string length".

**Gate**
- **The fit passes through all four datasheet points** for every IMM-α condition, checked by the independent golden-section MPP search. Worst error is 3e-9.
  - Fitted: a = 0.116–0.151 V (n·Nj 4.5–5.8), Rsh = 9.1e3–1.2e5 Ω for 1 cm².
  - BOL efficiency 32.01 % against the sheet's 32.0 %.
- **The claim behind the fallback** is checked by bisection on the ideal-diode dP/dV: the MPP lands > 30 mV below Vmp. The series fit errors for every IMM-α condition and works for every AZUR one.
- **Temperature against the projected coefficients**, 28–140 °C:
  - Voc follows its coefficient exactly. Isc does too (exact because Rs = 0).
  - Pmp: BOL within +0.97 %, 1E15 within 1.3 %, 5E15 within 0.7 %. 5E14 reaches +2.9 % at 100 °C and +4.7 % at 140 °C.
  - Reason: with Rs and Rsh fixed, the Voc and Isc coefficients determine the curve's temperature behaviour, so the Vmp and Imp coefficients can only be predicted. The projected 5E14 values ask the fill factor to fall faster than the curve does, and the projected Jmp coefficients aren't monotonic in fluence (6.7, 5.2, 3.3, 7.6 µA/cm²/°C).
  - Bounds: BOL < 1 %, all conditions < 5 %. Revisit with measured coefficients.
- **Boxwing with IMM-α** (synthetic orbit, thermal), with the AZUR baseline at 602.8 W:
  - 15-cell strings: 388.8 W mean. `check` reports that the bus takes 51 % of a hot string's maximum and suggests 8 cells.
  - With 8-cell strings: 691.3 W mean, 16.58 kWh.
- Tests: 101 passed, 1 ignored. Clippy and fmt are clean.

### Slice 7 addition: boxwing switched to IMM-α (2026-11-14)

The user chose to make `examples/boxwing` use `solaero-imm-alpha` with 8-cell strings (60 per wing).
- `check` reports: 120 strings of 8, and at 60 °C a full-sun string gives 9.69 W at 28.70 V, against a 10.24 W maximum at 31.40 V. The per-cm² preset now prints "cell size: per-cm² cell file, scaled to the layout's 32.00 cm² cells" instead of the mismatch note.
- Synthetic orbit (24 h, β = 0, +Z to the Sun, thermal): mean 691.3 W, 16.58 kWh, wings −143 … 107 °C. The AZUR version with 15-cell strings gave 602.8 W.
- Trade-off recorded in the scene file: at the 107 °C peak, Vmp is 8 × 3.40 = 27.2 V, below the 28.7 V bus + diode, so the hottest minutes run between Vmp and Voc (31.6 V). On a fixed bus no single string length suits every temperature; MPPT (v2) removes that.
- The guide text that described boxwing is updated. Unit tests still use the AZUR preset, whose fits are measured against its datasheet.

## Slice 8: panel sandbox, exact shadows, I-V plot (2026-11-14)

Built as the slice-7 design (S1–S3) says; no addendum was needed. The user chose to use the existing cell-file fields (no ICD fields).

**Added**
- `shadow/exact.rs`: every mesh triangle is cut to the half-space above the panel plane (above `ray_offset_m`, where the rays start), projected along the Sun direction, and clipped to each cell. The covered area is the union of convex polygons, computed exactly by a strip sweep: between consecutive breakpoints (vertices and edge crossings) every cross-section is linear, so the midpoint length times the width is exact. `ExactClipper` implements `Illuminator`, so the power path can run on exact shadows.
- `Circuit::string_curve`: points exactly on `string_v`, packed towards each photocurrent knee.
- `Scene::load_config`, `ObjWriter::oriented_box`.
- `crates/viewer/src/sandbox/`: `config` (`sandbox.toml`), `model` (one-panel scene, evaluation), `view2d`, `iv`, `eclipse`. Window with egui's painter only (no wgpu pass).
- `sunlyt sandbox` (box mode, or `--scene --panel --csv --at`), `--out dir` (cells, strings, I-V CSV), `--save-to`. Capture via `SUNLYT_SCREENSHOT` / `SUNLYT_AT` (UTC, seconds or `penumbra:<n>`) / `SUNLYT_SWEEP`.
- `examples/sandbox/{sandbox,penumbra}.toml`, `docs/guide/sandbox.md`. `doc_toml` routes sandbox files to the viewer's `sandbox_docs_parse` test.

**Gate**
- **Exact against analytic:** cube over plate, five Sun directions: shadow area equals the projected convex hull clipped to the plate to < 1e-12 m².
- **Rays against exact, per cell** (cube over plate, 2500 cells, five Suns): cells no edge crosses agree exactly; edge cells are within 1/n. Worst edge-cell error 0.173 / 0.162 / 0.048 for n = 2 / 4 / 8 (bounds 0.5 / 0.25 / 0.125); panel-mean difference ≤ 1.9e-3. The cluttered scene (bus, wings, dish, mast; 20 Suns) also passes the no-edge rule.
- **string_curve** against the ideal-cell closed form to 1e-9 (two knees, open circuit, all-bypassed end). Operating points read off the curve match `solve` (fixed bus, interpolated) and `mpp` (best sample) within 2e-3 / 1e-3.
- **From-scene power equals the full simulation** bit for bit (`assert_eq!`): boxwing, synthetic β = 0 orbit with the Sun 35° off the wings' normal, both wings, 42 row instants (21 with partly shaded cells) and 4 penumbra instants found by bisection between rows (visible = 0.5, 233.089 / 237.003 W). Exact-shadow power within 0.33 % of the rays there (samples = 2).
- **Box examples:** sandbox.toml 35.347 W (rays) vs 35.348 W (exact), 3 partly shaded strings, 4 distinct curves; penumbra.toml visible 0.4766.
- **Timing** (release, `SUNLYT_SWEEP=200`, 480-cell boxwing wing): evaluation median 2.8 ms, p95 5.1 ms, max 7.3 ms, inside the 16 ms frame, so it runs on the UI thread. Box mode ~1 ms.
- **Your look:** a penumbra (from-scene, `penumbra:0`, and `examples/sandbox/penumbra.toml`) and a partly shaded string (box example strings with a step in their I-V curve; boxwing strings 1–2 under the mast). Pending.

**Notes**
- The sandbox has no thermal model; cells are at `[electrical] temperature_c`.
- The spec's `--svg` sandbox export is not built: `--out` writes the same state as CSV, and `sunlyt check --svg` already draws layouts.
