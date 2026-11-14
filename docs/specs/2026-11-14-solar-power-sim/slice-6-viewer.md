# Slice 6 — viewer: design discussion

Status: APPROVED (I-V plot deferred, CLI flags) · built 2026-11-14 · parent: [design.md](design.md) (D8, D9, §7) · rules: `.claude/skills/sunlit-viewer-design`

## 1. Current state (research)

- `sunlit_core::sim::array_at(table, scene, illum, model, env, t, panel_temp_k)` (`crates/core/src/sim/array.rs:335`) returns `(ArraySample, per-cell lit fractions)` for any interpolated `t`. This is the viewer's one-instant entry point.
- `ArrayRun { t, samples: Vec<ArraySample> }` holds per-row results only. It has no per-cell lit fractions, and `compute_thermal` (`sim/array.rs:79`) is one rayon call with no progress hook.
- `ArraySample` holds `sun: Sunlight` (irradiance, `sun_dir_b`, per-body `occulted[]`, `visible`), per-panel power, sunlit fraction and temperature, per-string I and V, and total power (`power/array.rs:32`).
- `Scene` (`geometry/scene.rs:56`) holds the mesh, panels (normal, centroid, `u`/`v`, rows × cols, cell and string ranges) and cells (centre, normal, area). Everything is in body-frame metres.
- `Epoch.bodies[k].vec_b` and its radii give Earth and Moon in the body frame (`trajectory/table.rs:116`).
- The CLI's shared `TrajectoryArgs` (CSV, `--mapping`, `--solar-constant`, `--landed-on/at`) live in `crates/cli/src/commands/mod.rs:31`. There is no `mission.toml` yet: runs take `<csv> --scene scene.toml`.
- `examples/boxwing` (2 wings, 960 cells, fixed 28 V bus, thermal) is the demo scene. The real CSV comes only through `SUNLIT_SAMPLE_CSV`.
- Crates available today: eframe / egui-wgpu 0.37, egui_plot 0.38, wgpu 30, arc-swap 1.9. Toolchain: rustc 1.100.

## 2. Desired end state

1. `sunlit view <csv> --scene scene.toml [same flags as sunlit array]` opens a window within about 1 s. The batch run fills in behind it with a real progress bar.
2. The 3D view shows the OBJ (occluders in neutral grey), cells coloured by a perceptually uniform map with a legend and units, body axes, and Sun, Earth and Moon direction markers.
3. The timeline has umbra and penumbra bands, a scrubber, play/pause, 1×…10⁵× speed (default 60×), row stepping, and UTC to the second.
4. Stacked plots share the time axis: total and per-panel power, panel temperature, and visible solar fraction. Each has a cursor synced to the scrubber, a snapping tooltip and CSV export.
5. **One source of truth:** at a row time, every number shown is the batch `ArraySample` for that row, bit for bit. Between rows, values come from `array_at` at that time.
6. Every input responds on the next frame (≤ 16 ms target, 200 ms hard budget) on the boxwing scene with the real trajectory.

## 3. Constraints and patterns

- The viewer depends on `sunlit-core` only. Core gains no GPU or UI dependency.
- Folder per concern: `crates/viewer/src/{scene,timeline,plots,ui}/` and `worker.rs`.
- Follow the viewer-design skill: data is never eased; scrubbing, stepping and play/pause never animate; chrome motion is under 300 ms with the curves in `ui/motion.rs`; colours come from role tokens in `ui/theme.rs`; tabular figures with units.
- Plots follow the dataviz skill: one y-axis per chart; the categorical palette is validated in light and dark.
- Desktop only, native Windows first. CI builds and clippies the viewer on Linux, but does not run it.
- Never commit the mission CSV.

## 4. Options with trade-offs

**V1 Entry point.**
- (a) **`sunlit view` subcommand behind a default-on cargo feature `viewer`** ✅: one binary, matching design.md §2. `--no-default-features` gives a headless CLI for servers. `TrajectoryArgs` is reused as is.
- (b) A separate `sunlit-view` binary: no feature flag, but it needs its own copy of the trajectory arguments.
- → (a). The viewer crate is a library: `sunlit_viewer::run(Inputs) -> Result<()>`.

**V2 Rendering the 3D pass.**
- (a) An egui paint callback drawing straight into egui's render pass. Simple, but it shares egui's pass, so getting depth and MSAA depends on eframe options.
- (b) **Render into our own offscreen colour and depth textures (4× MSAA), then show the result as a native texture in an egui image** ✅. We own depth and MSAA, the 3D pass stays a self-contained module, and resizing is explicit.
- → (b).

**V3 How per-instant results reach the UI (D8).**
- (a) Call `array_at` on the UI thread each frame. Simple, but 10k cells × 4 rays could cost a few ms per frame and is not bounded.
- (b) **Use two workers.** The *run worker* computes the whole `ArrayRun` once and publishes it through `ArcSwapOption`, with progress in an `AtomicUsize`. The *instant worker* serves the latest requested `t` only (older requests are dropped) and publishes `Arc<Instant>` through `ArcSwap`. The UI never blocks: until the exact instant arrives it draws the nearest cached one.
- → (b). It meets the skill rule "show the cached neighbour at once, swap in the exact value".

**V4 Cell colour modes.** The default is **effective illumination** = lit fraction × cos θ × visible fraction, from 0 to 1 on viridis. The second mode is **string current** in A, with a sequential map (cividis), because current is never negative in v1. A diverging map has no natural midpoint here. Panel temperature appears in the plots, not on the cells.

## 5. Resolved decisions

- The batch run is the truth at rows. The viewer reads `run.samples[i]` and does not recompute it. A test asserts `array_at(t_i, T_i) == run.samples[i]`, so interpolated values between rows join the batch values exactly.
- Panel temperature between rows is linear between the run's rows. That is display interpolation, not a new thermal model.
- **Small core addition:** `ArrayRun::compute_thermal_progress(.., progress: &AtomicUsize)` (and the same for `compute`), counting finished rows. The existing functions call it with a throwaway counter, so batch output does not change.
- Camera: orbit and zoom on critically damped springs; presets (Sun, ±X, ±Y, ±Z) glide in ≤ 250 ms; double-click re-centres instantly. The reduced-motion setting turns glides off.
- Playback: speed steps 1, 10, 60, 600, 3600, 10⁴, 10⁵×. Play stops at the end of the table. `Space` plays or pauses, `←`/`→` step one row, `Shift+←`/`→` step ±10 rows, `Home`/`End` jump to the ends.
- Theme: dark and light token sets; the OS setting picks the default.

## 6. Open questions for you

1. **I-V curve plot** (per string at the cursor). Build it in slice 6, or leave it for later? It needs the curve sampled from `Circuit` at the instant, which adds a core accessor. *Blocks:* the plots scope. Recommendation: later; slice 6 ships power, temperature and visible fraction.
2. **Entry point (V1).** Is `sunlit view <csv> --scene …` right, or do you want `mission.toml` now? A mission file is not in any slice yet. *Blocks:* the CLI wiring. Recommendation: CLI flags now; mission.toml together with slice 7's README.

## 7. Structure outline

```
crates/viewer/                     lib, deps: sunlit-core, eframe(wgpu), egui_plot, arc-swap, glam, bytemuck
  src/lib.rs        pub struct Inputs { table, scene, model, env, thermal: Option<ThermalModel>, title }
                    pub fn run(inputs: Inputs) -> anyhow::Result<()>
  src/app.rs        struct ViewerApp: eframe::App  — owns Clock, Camera, Workers, layout
  src/worker.rs     struct RunWorker   { result: ArcSwapOption<ArrayRun>, done_rows: AtomicUsize, total: usize }
                    struct InstantWorker { fn request(&self, t: f64); fn latest(&self) -> Option<Arc<Instant>> }
                    struct Instant { t: f64, sample: ArraySample, lit: Vec<f64>, epoch: Epoch, exact_row: Option<usize> }
                    fn instant_at(shared: &Shared, run: Option<&ArrayRun>, t: f64) -> Instant
  src/clock.rs      struct Clock { t, playing, speed_idx }  fn tick(&mut self, dt_wall) ; step_rows(n)
  src/scene/
    gpu.rs          struct SceneRenderer { pipelines, offscreen msaa+depth, mesh vb/ib, cell instance buf }
                    fn resize(.., size) ; fn update_cells(&[ [f32;4] ]) ; fn render(.., &Camera, &Markers) -> egui::TextureId
    mesh.rs         fn mesh_vertices(&Scene) -> (Vec<Vertex>, Vec<u32>) ; fn cell_quads(&Scene) -> Vec<CellInstance>
    camera.rs       struct Camera { target, yaw, pitch, dist, springs }  fn view_proj(aspect) ; fn preset(Preset)
    markers.rs      fn markers(&Epoch, &Sunlight) -> Markers   // Sun/Earth/Moon unit dirs + body axes
    colour.rs       enum ColourMode { Illumination, StringCurrent } ; fn cell_colours(&Scene,&Instant,mode) -> Vec<[f32;4]>
                    fn viridis(x) ; fn cividis(x)   // LUTs, 256 entries
    shader.wgsl
  src/timeline/
    mod.rs          fn timeline(ui, &mut Clock, &Bands, utc: &dyn Fn(f64)->String)
    bands.rs        struct Bands { umbra: Vec<Range<f64>>, penumbra: Vec<Range<f64>> } fn from_run(&ArrayRun)
  src/plots/
    mod.rs          fn plots(ui, &ArrayRun, &Scene, cursor_t, &mut Option<f64> /*click-to-seek*/)
    export.rs       fn write_csv(path, series: &[Series]) -> io::Result<()>
  src/ui/
    theme.rs        struct Theme { surface, surface_raised, text, text_muted, accent, umbra, penumbra, series: [Color32;8] } ; light(), dark()
    motion.rs       EASE_OUT, EASE_IN_OUT (cubic-bezier) ; fn animate(ctx, id, target, dur) -> f32
    readout.rs      UTC, P, per-panel P/T/sunlit, visible %, with tabular figures and units
crates/cli/         feature "viewer" (default) → commands/view.rs: TrajectoryArgs + --scene → sunlit_viewer::run
crates/core/        sim/array.rs: compute_progress / compute_thermal_progress (+ tests)
```

### Build order (slice 6 is built as sub-steps, each committed)

| # | Scope | Check |
|---|---|---|
| 6a | Crate, feature-flagged `sunlit view`, window, theme, workers, progress bar, readout | Window opens < 1 s; progress counts to N; `array_at(t_i,T_i)==samples[i]` test passes |
| 6b | Offscreen wgpu pass: mesh, cells coloured, markers, spring camera, presets | Screenshot of boxwing; cells dim when wings face away |
| 6c | Timeline: bands, scrub, play/speed/step, keys | Scrubbing never shows a frame > 16 ms (frame-time readout in debug build) |
| 6d | Plots: stacked power / temperature / visible, cursor, tooltip, CSV export | Exported CSV equals the batch CSV columns at the same rows |

**Slice gate.** Manual: with the real trajectory and boxwing, pause during eclipse ingress and step through the penumbra at sub-row times; the cells dim uniformly while the readout's visible % falls. Automated: the row-equality test; clippy and fmt across the workspace; CI builds the viewer on Linux.
