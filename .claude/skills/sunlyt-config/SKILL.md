---
name: sunlyt-config
description: Edit sunlyt scene, cell and trajectory-mapping TOML files for the user (add or move panels, lay out cell grids, rewire strings, create a cell file from a datasheet or ICD, set the bus or thermal properties), then verify with `sunlyt check`. Use whenever the user asks to change a scene.toml, a cell .toml or a mapping .toml, or asks how to configure their spacecraft, panels, strings or cells.
---

# sunlyt configuration

The configuration is strict TOML: unknown keys are errors. The authority on every key, unit and default is `docs/guide/scene-reference.md`, with `docs/guide/cells.md` for cell files. Read the relevant one before editing; don't rely on memory.

## Loop

1. **See what sunlyt sees.** Run `sunlyt check <scene.toml>` (build with `cargo build --release` if needed; the binary is `target/release/sunlyt`). Use its group list for OBJ names instead of reading the OBJ.
2. **Edit the smallest thing that does the job.** Keep the user's comments and ordering. Put a short comment on each value you set, with its unit and where it came from (the user's drawing, the datasheet page).
3. **Never invent numbers.** Cell parameters, dimensions and diode drops come from the user or their documents. If something is missing, ask, or leave the default and say so.
4. **Verify.** Run `sunlyt check <scene.toml> --svg <dir>`. For a cell file, also run `sunlyt cell <file>` and compare the printed Isc/Voc/Vmp/Imp with the source (within ~1 %).
5. **Report** in a few lines:
   - what changed;
   - the check output that confirms it (panel faces and normals, cell counts, strings, the full-sun string against the bus);
   - the SVG path for the user to look at;
   - any warning, e.g. "Vmp below bus + blocking diode".

## Rules that catch people out

- `series` must divide `rows × cols`. Explicit strings must use every cell exactly once.
- Columns run along u and rows along v. v = normal × u. `u_axis` is projected onto the face.
- Every cell corner must be on the face. `offset` is measured from the face's area centroid.
- Slab substrates need `normal` for the cell side.
- One cell type and condition per array (`[electrical]`).
- Datasheet temperature coefficients are often per cm² or in mV/°C. The cell file wants V/K and A/K per cell.
- Mission trajectory files are never committed or pasted anywhere. Use `sunlyt synth` for examples.
