# FAQ

Every answer ends with a command that shows whether the change did what you meant. Run it every time.

- [How do I add my own spacecraft's OBJ file?](#how-do-i-add-my-own-spacecrafts-obj-file)
- [How do I specify which polygons in the OBJ are solar panels?](#how-do-i-specify-which-polygons-in-the-obj-are-solar-panels)
- [How do I position the solar cells on the polygon?](#how-do-i-position-the-solar-cells-on-the-polygon)
- [How do I change the string configuration for each panel?](#how-do-i-change-the-string-configuration-for-each-panel)
- [How do I change the type of solar cell? Is there a config file?](#how-do-i-change-the-type-of-solar-cell-is-there-a-config-file)
- [How can I use AI to help change the configuration?](#how-can-i-use-ai-to-help-change-the-configuration)
- [Why is a panel at 0 W in full sun?](#why-is-a-panel-at-0-w-in-full-sun)
- [Which way is the body frame, and which way does the quaternion turn?](#which-way-is-the-body-frame-and-which-way-does-the-quaternion-turn)
- [My cells are in a weird shape. Can sunlyt do non-rectangular layouts?](#my-cells-are-in-a-weird-shape-can-sunlyt-do-non-rectangular-layouts)
- [How long does a run take, and how do I make it faster?](#how-long-does-a-run-take-and-how-do-i-make-it-faster)
- [Can I use my mission's trajectory data?](#can-i-use-my-missions-trajectory-data)

---

### How do I add my own spacecraft's OBJ file?

1. **Export the spacecraft as Wavefront OBJ** in body-frame coordinates: the frame your trajectory's attitude quaternion rotates into.
   - **Blender:** File → Export → Wavefront (.obj). Set *Forward* = Y and *Up* = Z to keep Blender's coordinates unchanged (Blender's default export swaps to Y-up). Tick *Objects as OBJ Objects*.
   - **CAD:** export a mesh (STL → OBJ, or OBJ directly) with each panel substrate as its own body or part, so it becomes its own OBJ object.
   - Units don't matter: set `scale` to convert (0.001 for mm). Materials, normals and textures are ignored; polygons are triangulated on load.
2. **Give each panel substrate its own object or group name** (`o WING_PY` or `g WING_PY` in the file). Everything that isn't a panel still casts shadows.
3. **Write a scene TOML next to it.** Start from `examples/boxwing/scene.toml`:

   ```toml
   obj = "my-craft.obj"
   scale = 0.001          # the OBJ is in millimetres

   [[panel]]
   name = "+X panel"
   object = "PANEL_PX"
   normal = [1, 0, 0]
   rows = 10
   cols = 6
   cell = [0.08, 0.04]
   gap = [0.002, 0.002]
   ```

4. **Check:** `sunlyt check my-scene.toml --svg layout/`. The mesh size it prints should match your spacecraft in metres. If it is 1000× too big, fix `scale`.

### How do I specify which polygons in the OBJ are solar panels?

By name. `object = "…"` in a `[[panel]]` names an OBJ object or group, and that object's triangles form the panel face.

- **Find the names:** `sunlyt check` lists every group with its triangle count and marks which ones are panels. You can also search the OBJ text for lines starting with `o ` or `g `.
- **Slab substrates** (a box with a top and a bottom): add `normal = [x, y, z]` for the side that carries the cells. Only triangles facing that way (within ~8°) count as the face. Without `normal`, the object must be one flat, consistently wound surface.
- **The face is one plane.** A panel can't wrap around a corner. For cells on two faces, split them into two objects and two `[[panel]]` entries.
- If your CAD export merges everything into one object, separate the substrates in Blender (Edit mode → select → P → Selection) and name them.

Check: `sunlyt check` prints each panel's face area and normal. Compare them with your drawing.

### How do I position the solar cells on the polygon?

The grid is a rectangle of `rows × cols` cells, centred on the face's area centroid:

- **Size:** `cell = [u, v]` and `gap = [u, v]`, in metres.
- **Rotation:** `u_axis` sets the direction of the columns. It is projected onto the face; v = normal × u. The default is body +X projected (or +Y for faces whose normal is near ±X).
- **Position:** `offset = [u, v]` moves the grid centre along u and v, in metres.
- **Fit rule:** every cell corner must lie on the face. If one doesn't, loading stops and names the first row and column that stick out.

Check: `sunlyt check scene.toml --svg layout/`, then open `layout/<panel>.svg`. The grey shape is the face and the coloured rectangles are the cells, viewed from the cell side with u to the right and v up. `check` also prints the grid size, the centre offset and the smallest margin to the face edge.

### How do I change the string configuration for each panel?

Each `[[panel]]` has its own `strings`:

```toml
obj = "craft.obj"

[[panel]]
object = "WING_PY"
rows = 40
cols = 12
cell = [0.08, 0.04]
strings = { series = 15, order = "rows" }
```

- `series = N`: N cells per string. N must divide `rows × cols`.
- `order = "rows"` walks row 0 left to right, row 1 right to left, and so on. `"columns"` does the same up and down the columns.
- For any other wiring, list each string's cells explicitly as `[row, col]` pairs; see [the reference](scene-reference.md#strings-series-wiring).
- Omitting `strings` gives one string per row.
- Strings on one panel are in parallel. Each cell has a bypass diode and each string a blocking diode. The bus voltage, regulation and diode drops are in `[electrical]` and apply to every string.

Check: `sunlyt check --svg` draws each string's path from its first cell (the numbered dot) to its last. `check` also reports whether a full-sun string reaches `bus_v + blocking_vf_v`. If it warns, add cells in series.

### How do I change the type of solar cell? Is there a config file?

Yes, two levels:

1. **In the scene,** `[electrical] cell = "…"` picks the cell, and `condition = "…"` picks BOL or an irradiated condition from it:

   ```toml
   [electrical]
   cell = "cells/my-cell.toml"   # a built-in preset name, or a path relative to the scene
   condition = "BOL"
   ```

2. **The cell file** holds the datasheet numbers: Isc, Voc, Imp and Vmp per condition, temperature coefficients, area, test conditions and bypass diode drop. [Solar cells](cells.md) has a template and says where each number is on a typical datasheet.

One cell type applies to the whole array.

**A new cell usually needs a new string length.** A cell's voltage sets how many fit in series under the bus. For example, `solaero-imm-alpha` gives about 3.9 V per cell when hot, against 2.2 V for `azur-3g30a-advanced-4x8`. With the 15-cell strings that suit the AZUR cell on 28 V, IMM-α strings deliver half their power; with 8 cells (as in `examples/boxwing`), the array beats the AZUR version by 15 %. `sunlyt check` prints this and suggests a length.

If your datasheet gives values per cm², or gives EOL as remaining factors, see [Solar cells](cells.md#datasheets-that-give-values-per-cm).

Check: `sunlyt cell cells/my-cell.toml` (the fit against your datasheet points), then `sunlyt check scene.toml` (a string against your bus, and the suggested string length).

### How can I use AI to help change the configuration?

Yes. The configuration is plain TOML with a strict schema, which AI assistants edit well when they have the reference and your numbers. See [AI assistance](ai-assist.md) for what to give the assistant, ready-made prompts, and the check loop. In short:

1. Give it [scene-reference.md](scene-reference.md) (and [cells.md](cells.md) for cell work), your current scene file, and the output of `sunlyt check`.
2. Ask for the specific change.
3. Run `sunlyt check --svg` on the result and look at the drawing. Unknown keys are errors, so invented settings are caught immediately.

In Claude Code, the project skill `sunlyt-config` does this loop for you.

### Why is a panel at 0 W in full sun?

Usually the Sun is behind the panel: its cells face away (cos θ < 0). The readout in `sunlyt view` shows the panel's lit fraction. If that is 0 % while the solar disk is 100 % visible, the panel faces away or something shadows it. Other causes:

- On a fixed bus, a string whose voltage at its temperature can't reach `bus_v + blocking_vf_v` delivers nothing. `sunlyt check` warns about this for full sun. Hot panels and irradiated (EOL) conditions make it worse.
- A wrong `normal` puts the cells on the side that faces the bus.

### Which way is the body frame, and which way does the quaternion turn?

The body frame is whatever frame your trajectory's vectors are given in. The OBJ must use the same axes. The STK preset expects a J2000→body quaternion (`v_body = R(q)ᵀ v_J2000`, scalar last). `sunlyt inspect my.csv` checks the convention when the file has an inertial position column: it rebuilds the Earth vector from the position and quaternion, and reports the residual. Kilometres of residual mean the convention is wrong.

### My cells are in a weird shape. Can sunlyt do non-rectangular layouts?

Each `[[panel]]` is a rectangular grid, but you can put several `[[panel]]` grids on the same OBJ face, each with its own `offset`, size and wiring. For example, a 10 × 12 block and a 6 × 8 block on one wing. Keep them from overlapping; `check --svg` shows them. Fully irregular cell positions aren't supported yet.

### How long does a run take, and how do I make it faster?

About 1–4 s for 12,000 rows and 10,000 cells on a 12-core desktop. Time scales with rows × cells × `samples²`. `samples = 1` is 4× faster than the default 2 and fine when little self-shadowing happens.

### Can I use my mission's trajectory data?

Yes; it never leaves your machine. Keep mission files out of the repository: `data/private/` is git-ignored for that. Use `sunlyt synth` for anything you share or commit.
