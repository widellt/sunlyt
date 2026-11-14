# Scene file reference

A scene is one TOML file. It names the spacecraft's OBJ mesh, lays cell grids on the panel faces, and optionally sets the electrical (`[electrical]`) and thermal (`[thermal]`) models. Every key is listed here with its unit and default. Unknown keys are errors, so a typo never passes silently.

After any edit, run:

```bash
sunlyt check my-scene.toml --svg layout/
```

## Coordinate frame and units

- **Body frame.** The OBJ coordinates are the spacecraft body frame: the frame the trajectory's attitude quaternion rotates into. Directions such as `normal` and `u_axis` are body-frame vectors. Only directions matter for sunlight; the OBJ origin can be anywhere.
- **Units.** Lengths are metres after `scale`; electrical values are SI; temperatures are in °C in the files.
- **"Cell side."** A panel's cells face along its `normal`. The view in `check --svg` looks at the cells from that side, with u to the right and v up.

## Top level

```toml
obj = "craft.obj"   # path to the OBJ file, relative to this TOML
scale = 1.0         # OBJ units → metres (0.001 for millimetres)
samples = 2         # shadow rays per cell = samples², on a regular grid
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `obj` | path | required | OBJ mesh. Polygons are triangulated; materials, lines and points are ignored. |
| `scale` | number | `1.0` | Multiplies every OBJ coordinate to give metres. |
| `samples` | integer ≥ 1 | `2` | Shadow rays per cell side. A cell's lit fraction is the share of its `samples × samples` points that see the Sun. A higher value gives finer partial shading and costs time linearly in `samples²`. |

## `[[panel]]`: a grid of cells on an OBJ face

Repeat `[[panel]]` once per grid. Two grids may share one OBJ face if they don't overlap; give each its own `offset`.

```toml
obj = "craft.obj"

[[panel]]
name = "+Y wing"            # label in output columns and the viewer
object = "WING_PY"          # OBJ `o` or `g` name of the substrate
normal = [0, 0, 1]          # which side carries the cells (needed for slabs)
rows = 40                   # cells along v
cols = 12                   # cells along u
cell = [0.08, 0.04]         # cell size along (u, v), metres
gap = [0.002, 0.002]        # spacing between neighbouring cells (u, v), metres
u_axis = [1, 0, 0]          # column direction, projected onto the face
offset = [0.0, 0.0]         # grid centre from the face centroid (u, v), metres
strings = { series = 15 }   # wiring: 15 cells per series string
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `name` | string | the `object` | Panel label. Output columns use it with characters other than letters, digits, `+`, `-` and `_` replaced by `_`. |
| `object` | string | required | The OBJ object or group that holds the panel substrate. `sunlyt check` lists every group in the file. |
| `normal` | `[x, y, z]` | none | Outward normal of the cell side, body frame (any length). Only the object's triangles facing this way (within ~8°) form the face, so a slab with a top and bottom works. Without it the whole object must be one flat, consistently wound surface, and its winding gives the normal. |
| `rows`, `cols` | integers ≥ 1 | required | Grid size. Columns run along u, rows along v. |
| `cell` | `[u, v]` metres | required | Cell size along u and v. If it differs from the cell datasheet's area, current is scaled to the layout cell's area (`check` says so). |
| `gap` | `[u, v]` metres | `[0, 0]` | Space between adjacent cells. The grid measures `cols·cell_u + (cols−1)·gap_u` by `rows·cell_v + (rows−1)·gap_v`. |
| `u_axis` | `[x, y, z]` | body +X projected onto the face (+Y if the normal is near ±X) | Direction of the columns. It is projected onto the face plane; v = normal × u. Use it to rotate the grid on the face. |
| `offset` | `[u, v]` metres | `[0, 0]` | Moves the grid centre away from the face's area centroid, along u and v. |
| `strings` | table or list | one string per row | How cells are wired in series (below). |

**Fit rule:** every corner of every cell must lie on the face, or loading stops with the row and column of the first cell that sticks out. `check` prints the smallest margin.

### `strings`: series wiring

Each string is cells in series, with one bypass diode per cell, feeding the bus through one blocking diode. Strings on a panel are in parallel.

**Chunked** (the usual case): walk the grid serpentine-fashion and cut the walk every `series` cells. `series` must divide `rows × cols`.

```toml
obj = "craft.obj"

[[panel]]
object = "WING_PY"
rows = 40
cols = 12
cell = [0.08, 0.04]
strings = { series = 15, order = "rows" }  # or "columns"
```

- `order = "rows"` (default): row 0 left to right, row 1 right to left, and so on.
- `order = "columns"`: column 0 bottom to top, column 1 top to bottom, and so on.

**Explicit:** list every string as `[row, col]` pairs in wiring order. Each cell must appear exactly once. Rows count from v = min and columns from u = min.

```toml
obj = "craft.obj"

[[panel]]
object = "SMALL"
rows = 2
cols = 3
cell = [0.08, 0.04]
strings = [
  [[0, 0], [0, 1], [0, 2]],
  [[1, 2], [1, 1], [1, 0]],
]
```

`check --svg` draws each string's path from its first cell (the numbered dot) to its last.

## `[electrical]`: cells, strings and the bus

Without this table every cell converts sunlight at a constant efficiency (0.3, or `--efficiency`). One cell type and condition apply to the whole array.

```toml
[electrical]
cell = "azur-3g30a-advanced-4x8"  # built-in preset, or a path ending in .toml
condition = "BOL"                 # a [[condition]] name in the cell file
temperature_c = 60.0              # cell temperature when there is no [thermal]
regulation = "fixed-bus"          # or "mppt"
bus_v = 28.0                      # fixed-bus voltage, V
blocking_vf_v = 0.7               # blocking diode forward drop, V
# bypass_vf_v = 0.8               # per-cell bypass diode, V; default from the cell file
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `cell` | string | required | Built-in preset name or a cell TOML path, relative to the scene file. See [Solar cells](cells.md). |
| `condition` | string | `"BOL"` | Which datasheet condition to use (BOL, or a fluence such as `"1E15"`). |
| `temperature_c` | °C | `28.0` | Cell temperature when there is no `[thermal]`. |
| `regulation` | `"fixed-bus"` \| `"mppt"` | `"fixed-bus"` | Fixed bus: every string is held at `bus_v + blocking_vf_v`. MPPT: an ideal tracker per string, which is an upper bound. |
| `bus_v` | V | `28.0` | Bus voltage for `fixed-bus`. |
| `blocking_vf_v` | V | `0.7` | One blocking diode per string. 0.7 V is an assumption; set it from your harness. |
| `bypass_vf_v` | V | from the cell file | Forward drop of each cell's bypass diode. |

`check` reports what one string in full normal sun delivers at `temperature_c`. It warns when the string's maximum-power voltage falls below `bus_v + blocking_vf_v`, which means the string needs more cells in series.

## `[thermal]`: panel temperature

Without this table the cells sit at `[electrical] temperature_c`. With it, each panel's temperature is integrated through the run (one lumped temperature per panel, from absorbed sunlight, Moon/Earth albedo and IR, radiation and the electrical power drawn).

```toml
[thermal]
heat_capacity_j_m2_k = 1800  # per m² of panel (substrate + cells)
absorptance_front = 0.91     # cells with coverglass, solar band
emittance_front = 0.85       # coverglass, infrared
absorptance_back = 0.90      # back face, solar band
emittance_back = 0.85        # back face, infrared
# back_sink_c = 20           # body-mounted panel: the back radiates to the bus at 20 °C
# initial_c = 20             # default: steady state at the first row
planets = true               # albedo and IR from the Moon and Earth
```

| Key | Unit | Default | Meaning |
|---|---|---|---|
| `heat_capacity_j_m2_k` | J/(m²·K) | 1800 | Areal heat capacity. The default is typical; set it from your design. |
| `absorptance_front` | 0–1 | 0.91 | The cell datasheet's bound with 100 µm AR coverglass. |
| `emittance_front` | 0–1 | 0.85 | Typical coverglass. |
| `absorptance_back`, `emittance_back` | 0–1 | 0.90, 0.85 | Typical black-painted back. |
| `back_sink_c` | °C | none | Set this for body-mounted panels: the back sees the bus, not space or sunlight. |
| `initial_c` | °C | steady state | Starting temperature. |
| `planets` | bool | `true` | Albedo and IR from the Moon and Earth (built in for bodies named `Moon` and `Earth`). |

## Trajectory input

`sunlyt array`, `view` and `inspect` read a CSV through a column mapping. The built-in mapping `stk-body-vectors` matches an STK report with these columns:

- `Time (UTCG)`, as `M/D/YYYY H:MM`;
- the J2000→body quaternion `J2000-qx, -qy, -qz, -qs` (scalar last);
- Earth-centred J2000 position, used only for a convention check;
- spacecraft→Sun, →Earth and →Moon vectors in the body frame, in km.

`sunlyt synth` writes this layout. For other files, write a mapping TOML and pass `--mapping my-map.toml`:

```toml
length_unit_m = 1000.0                 # metres per unit of every vector column
sun_body = ["SunX", "SunY", "SunZ"]    # spacecraft → Sun, body frame

[time]
column = "UTC"
format = "%Y-%m-%dT%H:%M:%S"           # chrono strftime; times are UTC, no leap seconds

[attitude]
columns = ["q1", "q2", "q3", "q0"]
order = "scalar_last"                  # or "scalar_first"
direction = "inertial_to_body"         # v_body = R(q)ᵀ v_inertial; or "body_to_inertial"

[[bodies]]
name = "Moon"                          # "Moon" and "Earth" get albedo/IR models
radius_km = 1737.4
columns = ["MoonX", "MoonY", "MoonZ"]  # spacecraft → body, body frame

[[bodies]]
name = "Earth"
radius_km = 6378.137
polar_radius_km = 6356.752             # optional: oblate spheroid
columns = ["EarthX", "EarthY", "EarthZ"]
```

The attitude is required: without it the viewer cannot interpolate between rows. `sunlyt inspect my.csv --mapping my-map.toml` loads a file and checks the quaternion convention when a `[position]` table is given.
