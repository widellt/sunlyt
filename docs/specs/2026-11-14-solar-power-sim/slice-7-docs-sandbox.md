# Slice 7 — rename, user docs and FAQ, panel sandbox: design discussion

Status: APPROVED 2026-11-14 (repo + folder renamed too; sandbox shadows = boxes + from scene; slices 7 docs / 8 sandbox / 9 cache-Parquet-summary; one cell type per array; Q2 ICD fields still open, blocks slice 8 only) · parent: [design.md](design.md) · follows slice 6 (merged as widellt/sunlit#6)

## 1. Current state (research)

- **Name.** `sunlit` appears in 42 tracked files: the crates `sunlit-core` and `sunlit-viewer`, the binary `sunlit`, the env vars `SUNLIT_SAMPLE_CSV` and `SUNLIT_SCREENSHOT` / `AT` / `SWEEP` / `MODE`, the skills `.claude/skills/sunlit-*`, the README and the specs. The repo is `widellt/sunlit`.
- **Docs.** `README.md` (196 lines) covers build, use, a commented scene example, input format, physics, layout and development. The full key reference exists only in code comments: `geometry/layout.rs:1-24` (scene and panel keys), `power/circuit.rs:205-235` (`[electrical]`) and `thermal/mod.rs` (`[thermal]`). There are no FAQ or guide pages.
- **How panels are found** (`geometry/scene.rs`, `layout.rs:47-70`):
  - `object` names an OBJ `o`/`g` group, which is the substrate.
  - `normal` picks the cell side of a slab.
  - The grid has `rows × cols` cells of size `cell` with `gap` spacing. It is centred on the face centroid, or moved by `offset (u, v)`.
  - `u_axis` sets the column direction (body X projected onto the face by default).
  - Cells must fit inside the face or loading fails.
  - Everything else in the OBJ is an occluder.
- **Strings** (`layout.rs:86-104`): `strings = { series = N, order = "rows" | "columns" }` walks the grid serpentine-fashion and cuts it into strings of N cells. `strings = [[[row, col], …], …]` is explicit wiring. The default is one string per row.
- **Cells** (`power/cell.rs:55-135`):
  - `[electrical] cell` is a built-in preset name or a path ending in `.toml`.
  - One preset ships (AZUR 3G30A 4x8): datasheet Isc/Voc/Imp/Vmp per condition (BOL, fluences), temperature coefficients and bypass Vf.
  - **One cell type, condition, regulation and bus for the whole array.** No per-panel setting exists.
- **Examples.** `examples/boxwing` has an OBJ and a scene, but no trajectory: the only one is a user-supplied, possibly export-controlled CSV, which is never committed. A clean clone cannot run `sunlit array` or `sunlit view`.
- **Viewer** (slice 6): egui/wgpu. The deferred I-V plot needs the string curve at an instant; `Circuit` solves operating points but has no public curve sampler.
- **The old slice-7 scope** (design.md §7): Sun-direction cache, Parquet output, summary report, synthetic example, README.

## 2. Desired end state

1. **The tool is called `sunlyt`.** That covers the binary, the crates (`sunlyt-core`, `sunlyt-viewer`), the env vars (`SUNLYT_*`), the skills, the docs and the window title. The physics and numbers do not change: the test suite is unchanged apart from names.
2. **A fresh clone runs end to end:**
   - `sunlyt synth` writes a synthetic lunar-orbit trajectory CSV. It is not mission data, so the repo can carry the example.
   - `examples/boxwing` then runs with `sunlyt array` / `sunlyt view` / `sunlyt sandbox` in three commands from the README.
3. **User docs** in `docs/guide/` (the README links to them and shrinks to a quick start):
   - `getting-started.md`
   - `scene-reference.md`: every key, its unit and default, and an example.
   - `cells.md`: presets, and turning a datasheet into a cell file.
   - `faq.md`: the questions below plus troubleshooting.
   - `ai-assist.md`
4. **`sunlyt check scene.toml`** validates a scene without a trajectory. It prints panels, faces found, normals, cell counts, strings and the cell fit, and `--svg out/` writes one layout map per panel: cells, string colours and walk order, and the u/v axes. Every FAQ answer ends with "run `sunlyt check` to confirm".
5. **The panel sandbox**, `sunlyt sandbox`: one flat 2-D panel with its cells, strings and cell type. It shows the Sun, Earth and Moon directions, and shadowing at one instant. It cross-checks the full simulation (§4 S2).

FAQ questions to answer (yours first):

| Question | Answered in |
|---|---|
| How do I add my own spacecraft's OBJ file? | faq → scene-reference `obj`, `scale`, export tips (Blender, CAD → OBJ, units, groups) |
| How do I specify which polygons in the OBJ are solar panels? | faq → `object` = an OBJ `o`/`g` name; `normal` for slabs; `sunlyt check` lists the groups it found |
| How do I position the cells on the polygon? | faq → grid, `u_axis`, `offset`, `gap`, fit rule; `check --svg` picture |
| How do I change the string configuration per panel? | faq → `strings = { series, order }`, explicit wiring; the layout map |
| How do I change the cell type? Is there a config file? | cells.md → preset vs. `my-cell.toml`, fields, where to find them on a datasheet |
| How can I use AI to change the configuration? | ai-assist.md → what to give the AI, prompt templates, check loop, Claude Code skill |
| *(added)* Why is a panel 0 W in full sun? Where is the body frame? Which way do the quaternions turn? How long does a run take? | faq |

## 3. Constraints and patterns

- Never commit mission data or name the mission. The synthetic trajectory is generated, not copied.
- Docs state units and defaults as the code has them. A doc test parses every TOML block in `docs/guide/` and `examples/` through the real config types, so docs cannot drift.
- Folder per concern: the sandbox lives in `crates/viewer/src/sandbox/`, and core gains nothing UI-related.
- The viewer-design skill applies to the sandbox: no easing on data, tabular figures, theme tokens.
- Physics changes follow `sunlit-physics-verification` (renamed). The sandbox adds an independent shadow calculation, so it falls under that skill.

## 4. Options with trade-offs

**S1 What the sandbox is.**
- (a) **A second egui window mode in the viewer crate** ✅. It shows a 2-D face-on view of the panel (cells, string colours, shadow polygons), controls for the Sun direction or an instant from a trajectory, the per-string I-V curves with the bus line and operating points (the deferred I-V plot lands here), and an eclipse inset drawing the Sun and Moon/Earth disks at their true angular sizes. It reuses the theme, readout and workers, and needs no wgpu (painter only).
- (b) CLI only, writing CSV and SVG. It is scriptable, but loses the "move the Sun and watch" sanity check.
- → (a), plus `--svg` / `--csv` flags so a sandbox state can be saved and diffed.

**S2 How it checks the full simulation.** Running the same core code a second time would only check that the code agrees with itself. The sandbox therefore computes two things independently:
- **Shadows, exactly:** occluder triangles are projected onto the panel plane along the Sun direction, and each cell rectangle is clipped against the projected polygons (exact polygon area). That gives each cell's lit fraction with no sampling, which the sandbox compares with the ray caster's n×n estimate cell by cell, highlighting differences above a threshold. This is a true check of the BVH ray caster and the sample grid.
- **Eclipse:** the visible solar disk as two drawn disks, next to `visible_fraction`.
- **Electrical:** the I-V curve of each string with the bus line. The operating point is read off the curve on screen and checked against `Circuit::solve`.
- **"From scene" mode** (`--scene … --panel "+Y wing" --csv … --at UTC`) takes one panel from a full scene, with every other triangle as an occluder, at a trajectory instant. Its power must equal the full simulation's panel power at that instant (asserted in a test), and its exact shadows are compared with the rays.

**S3 Sandbox input.**
- (a) **A small `sandbox.toml` using the same keys as a scene's `[[panel]]` plus `[electrical]`** ✅, with:
  - a rectangular `size` instead of an OBJ;
  - `[sun]` as azimuth and elevation in the panel frame, or a body-frame vector;
  - optional `[[box]]` occluders (centre, size, rotation);
  - optional Earth/Moon vectors.
  What you learn in the sandbox carries straight over to scene files. The sandbox writes a temporary OBJ in memory, so the shadow and power path is the real one.
- (b) Only interactive controls, with no file. Less to learn, but states can't be saved or reviewed.
- → (a). The UI edits the in-memory state; "Save" writes the TOML back.

**S4 AI assistance.** In order of payoff:
1. `scene-reference.md` written so it can be pasted into any chat, with every key, unit and constraint.
2. `ai-assist.md`: what to give the AI (scene-reference, your datasheet or ICD fields, `sunlyt check` output, the OBJ group list from `check`), prompt templates for the common edits, and "always run `sunlyt check` / `sandbox` on the result".
3. A Claude Code project skill `.claude/skills/sunlyt-config` that knows the schema and runs `sunlyt check` after each edit.
4. A JSON Schema of the TOML (`sunlyt schema`) for editors and AI tools. **Recommend deferring** this one: (1)–(3) cover the need without it.

## 5. Resolved decisions

- The rename is mechanical and lands first, in a separate commit, so later diffs stay readable.
- `sunlyt synth` writes the existing synthetic generator (from the `thermal_run` tests) as a CLI command: a circular orbit, a configurable attitude mode (inertial or Sun-pointing roll), and a duration. Docs and examples use it.
- The docs' TOML blocks are tested (§3).
- The panel sandbox reuses `Scene::build` with an in-memory mesh: no second geometry code path.

## 6. Open questions for you

1. **Rename reach.** Should I also rename the GitHub repo (`widellt/sunlit` → `widellt/sunlyt`; GitHub redirects the old URL) and the local folder? *Blocks:* 7a. Recommendation: yes, both.
2. **"Traits from ICD".** Which cell and string parameters does your ICD specify? Field names are enough, no values. Are they beyond the cell file's Isc/Voc/Imp/Vmp per condition, temperature coefficients, bypass Vf, string length and blocking diode (e.g. per-k-cell bypass groups, harness resistance, cell-to-cell mismatch)? *Blocks:* the sandbox and cell-file schema. Recommendation: map the ICD fields onto the cell file. Model anything new (e.g. harness resistance) as a separate physics change.
3. **What shadows the sandbox panel?** *Blocks:* S3 scope. Recommendation: both box occluders and the "from scene" mode.
4. **Slice split.** *Blocks:* ordering. Recommendation:
   - **Slice 7:** rename, synth, `check`, docs, FAQ and the AI skill.
   - **Slice 8:** the sandbox, with the I-V plot.
   - **Slice 9:** Sun-direction cache, Parquet and the summary report (the old slice-7 items).
5. **Per-panel cell types** (e.g. different cells or BOL/EOL conditions per wing). Today it is one per array. *Blocks:* nothing now; it changes one FAQ answer. Recommendation: document it as a limit; add `[[panel]] cell = …` later if you need it.

## 7. Structure outline

```
crates/core      → sunlyt-core      (rename only, plus:)
  src/trajectory/synth.rs   pub struct SynthOrbit { altitude_km, period?, duration_s, step_s, attitude: SynthAttitude }
                            pub fn write_csv(&SynthOrbit, W) -> io::Result<()>   // stk-body-vectors columns
  src/geometry/check.rs     pub struct SceneReport { panels: Vec<PanelReport>, groups: Vec<String> }
                            pub fn report(&Scene, &SceneConfig) -> SceneReport
                            pub fn layout_svg(&Scene, panel: usize) -> String
  src/shadow/exact.rs       pub fn exact_lit_fractions(scene, panel, sun_dir_b) -> Vec<f64>   // polygon clipping (slice 8)
  src/power/circuit.rs      pub fn Circuit::string_curve(&self, groups, n) -> Vec<(f64, f64)>  // for the I-V plot (slice 8)
crates/cli       → sunlyt           commands/{synth, check, sandbox}.rs
crates/viewer    → sunlyt-viewer    src/sandbox/{mod, config.rs (SandboxConfig), view2d.rs, iv.rs, eclipse.rs}   (slice 8)
docs/guide/{getting-started, scene-reference, cells, faq, ai-assist}.md
crates/core/tests/doc_toml.rs       every ```toml block in docs/guide and examples/ parses
.claude/skills/{sunlyt-physics-verification, sunlyt-viewer-design, sunlyt-config}/
examples/boxwing/{scene.toml, craft.obj, README.md}   + `sunlyt synth` command in its README
```

| # | Scope | Test gate |
|---|---|---|
| 7a | Rename everything to sunlyt | `cargo test --workspace` identical counts; `rg -i sunlit` hits only the history in specs and progress |
| 7b | `sunlyt synth`, `sunlyt check` (+ `--svg`) | Clean clone: synth → array → view runs; `check` on boxwing lists 2 panels × 480 cells, 32 strings each; the SVG shows the serpentine walk |
| 7c | docs/guide, README quick start, FAQ, AI skill | `doc_toml` test passes; a fresh-eyes read-through by you; each FAQ command runs |
| 8 | Sandbox (S1–S3), I-V plot, exact shadows | Exact vs. ray: max per-cell error < 1/n² + edge term on the cube-on-plate case; from-scene power equals the full simulation at the instant; you look at a penumbra and a partly shaded string |
