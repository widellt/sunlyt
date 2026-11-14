# Getting started

sunlyt computes the electrical power of a spacecraft's body-mounted solar array along a trajectory. It handles cells, strings, self-shadowing, eclipses by the Moon and Earth, and panel temperature. This page takes you from a fresh clone to a run and the 3D viewer in about five minutes, with no mission data.

## 1. Build

You need Rust (stable, 1.95 or newer) from <https://rustup.rs>. On Windows, use the MSVC toolchain with the Visual Studio Build Tools.

```bash
cargo build --release
```

The program is `target/release/sunlyt` (`sunlyt.exe` on Windows). The commands below call it `sunlyt`. Put `target/release` on your `PATH`, or run `cargo run --release -- <command>` instead.

On a server with no display, `cargo build --release -p sunlyt --no-default-features` builds the CLI without the viewer.

## 2. Make a trajectory

sunlyt reads a trajectory CSV: time, the attitude quaternion, and Sun/Earth/Moon vectors in the spacecraft body frame (see [Trajectory input](scene-reference.md#trajectory-input)). For a first run, generate a synthetic one: 24 h in a 100 km circular lunar orbit, with body +Z pointed at the Sun.

```bash
sunlyt synth -o orbit.csv
```

`sunlyt synth --help` lists the knobs: `--altitude-km`, `--beta-deg` (0 gives the longest eclipse; around 71° and above at 100 km gives none), `--hours`, `--attitude inertial|sun` and `--sun-axis`.

## 3. Check the spacecraft

`examples/boxwing` is a box bus with two wings, each 40 × 12 SolAero IMM-α cells (4 × 8 cm) wired into 8-cell strings on a 28 V bus.

```bash
sunlyt check examples/boxwing/scene.toml --svg layout/
```

`check` needs no trajectory. It lists the OBJ groups and which ones are panels, each panel's face, normal, grid and strings, and whether a string in full sun reaches the bus. `layout/+Y_wing.svg` draws where every cell sits and how the strings are wired. Run `check` after every change to a scene.

## 4. Run

```bash
sunlyt array orbit.csv --scene examples/boxwing/scene.toml -o power.csv
```

This prints eclipse counts, mean, minimum and maximum power, energy, and per-panel temperatures. `power.csv` has one row per trajectory row: total and per-panel power, sunlit fraction, string current, and temperature.

## 5. Look at it

```bash
sunlyt view orbit.csv --scene examples/boxwing/scene.toml
```

- **3D view:** drag to orbit, scroll to zoom, double-click to reset. Cells are coloured by illumination, or by string current with the toggle.
- **Timeline:** Space plays and pauses, ←/→ step a row, Shift steps 10 rows, `[` and `]` change speed. The shaded bands are umbra and penumbra.
- **Plots:** click a plot to jump there. "Export CSV" saves the plotted numbers in the same columns as `array -o`.

At any row the viewer shows exactly the numbers `sunlyt array` writes.

## Next

- Your own spacecraft: [FAQ](faq.md#how-do-i-add-my-own-spacecrafts-obj-file), then [Scene file reference](scene-reference.md).
- Your own cells: [Solar cells](cells.md).
- Letting an AI assistant edit the configuration: [AI assistance](ai-assist.md).
