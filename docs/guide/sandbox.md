# The panel sandbox

`sunlyt sandbox` shows one flat panel face on: its cells, how they are wired into strings, the shadows on it, where the Sun is, an inset of the Sun with any eclipsing body at true size, and each string's I-V curve with the bus line. Use it to try a layout or a string length before putting it in a scene, and to see why a panel in a full run makes the power it does.

It also checks the full simulation with two calculations that share no code with it:

- **Exact shadows.** Every triangle in the way is projected onto the panel along the Sun direction, and each cell's shaded area is computed exactly by polygon clipping. The full simulation samples each cell with n × n rays instead. The sandbox shows both, cell by cell, and the panel power on each.
- **I-V curves.** Each string's operating point is drawn on its curve. On a fixed bus it sits where the curve crosses the bus line.

## Two ways in

**A sandbox file** (box mode). One panel, the Sun, and boxes that cast shadows:

```
sunlyt sandbox examples/sandbox/sandbox.toml
```

Drag the azimuth and elevation sliders (or press ← →) and the shadows, curves and power follow at once. **Save** writes the file with the new Sun direction to `<name>.saved.toml` beside it (`--save-to` picks another path; TOML comments are not kept).

**One panel of a scene** (from-scene mode). Every other triangle of the scene, including the other panels, becomes an occluder, and the Sun and eclipses come from a trajectory:

```
sunlyt sandbox --scene examples/boxwing/scene.toml --panel "+Y wing" --csv orbit.csv --at 2030-01-01T00:35:38
```

`--panel` takes the panel's `name` or its OBJ `object`. `--at` takes UTC or seconds after the first row; the default is the first row. Step rows with ← → (Shift for 10), drag the time slider, or press **Next penumbra** to jump to the next instant with half the Sun hidden. The panel's power here equals the full simulation's power for that panel at that instant (a test checks this).

The sandbox has no thermal model: cells are at `[electrical] temperature_c`. To compare with a `sunlyt view` run of a scene with `[thermal]`, set `temperature_c` to the panel temperature the viewer shows.

## Reading the window

- **Panel view.** Cells are coloured by their lit fraction (0 shaded, 1 fully lit) from the rays, from exact clipping, or as the difference (switch at the top right). A coloured line runs through each string's cells in series order, starting at the dot. White outlines are the shadows of individual occluder triangles. The dome at the top left shows the Sun: the centre is the panel normal, the rim the panel plane.
- **Eclipse inset.** The solar disk and every body's disk at their true angular sizes, beside the visible fraction the simulation uses and each body's share.
- **Panel readout.** Power on ray-cast and on exact shadows, and their difference; how many cells and strings are partly shaded; and the worst cell difference against its bound (below).
- **I-V plot.** One curve per distinct lighting: strings under identical light share a curve, and the legend says how many. The dots are the operating points; the dashed line is the bus plus the blocking diode. A partly shaded string shows a step where its shaded cells' bypass diodes take over. Hover a curve or a cell to highlight the string in both places.

## How close rays and exact shadows should be

A cell that no shadow edge crosses must agree exactly. A cell an edge crosses is sampled on an n × n grid of points, which misreads a straight edge by at most half a row of samples, 1/(2n) of the cell; a shadow corner puts two edges in one cell, so the bound is 1/n. With `samples = 2` that allows up to 0.5 on single cells, while the panel's total power usually differs by well under 1 %. Raise `samples` in a scene if single partly shaded cells matter (for example, a string whose only shaded cell sets its current).

## Sandbox file reference

The panel keys are those of a scene's `[[panel]]`, and `[electrical]` is a scene's `[electrical]`; see the [scene reference](scene-reference.md). What works here carries straight over to a scene. The sandbox frame is the panel's own: x along the panel's u axis (columns), y along v (rows), z out of the cell side. So `object`, `normal` and `u_axis` are not allowed in `[panel]`.

```toml
samples = 4                 # n × n shadow rays per cell (default 2)
size = [0.52, 0.36]         # substrate (u, v), metres, centred on the origin

[panel]
rows = 8                    # along v
cols = 6                    # along u
cell = [0.08, 0.04]         # (u, v), metres
gap = [0.002, 0.002]
strings = { series = 8 }

[sun]
azimuth_deg = 200           # from +u towards +v (default 0)
elevation_deg = 50          # above the panel (default 90)
# vector = [0.3, 0.2, 0.9]  # instead of azimuth and elevation
distance_au = 1.0           # default 1

[[box]]                     # any number of occluders
centre = [-0.08, -0.05, 0.14]  # (u, v, height above the panel), metres
size = [0.10, 0.06, 0.08]
rotation_deg = 15           # about the panel normal (default 0)

[[body]]                    # optional eclipsing sphere
name = "Moon"
direction = [-0.87728, -0.31930, -0.35837]  # from the panel, sandbox frame
distance_km = 1837.4        # to the centre
radius_km = 1737.4

[electrical]                # omit for a constant 30 % efficiency (no I-V curves)
cell = "solaero-imm-alpha"
temperature_c = 60.0
regulation = "fixed-bus"
bus_v = 28.0
```

[`examples/sandbox/penumbra.toml`](../../examples/sandbox/penumbra.toml) places the Moon so its limb crosses just past the Sun's centre (47.7 % of the disk visible).

## Saving the numbers

`--out dir/` writes the instant's numbers and exits without a window:

| File | Columns |
|---|---|
| `cells.csv` | row, col, string, u_m, v_m, lit_rays, lit_exact |
| `strings.csv` | string, cells, current_a, voltage_v |
| `iv.csv` | curve, first_string, strings (how many share it), current_a, voltage_v |

## Limits

- One panel at a time, flat. In box mode the occluders are boxes; use from-scene mode for any other shape.
- The Sun is a point for shadows, as in the full simulation (its 0.27° radius blurs an edge by about 9 mm per metre of distance to the occluder). The finite disk is used for eclipses.
- An oblate body (Earth) is drawn as a circle in the inset; the visible fraction uses its true limb.

Run `sunlyt check` on a scene before `--scene` to confirm its panels load.
