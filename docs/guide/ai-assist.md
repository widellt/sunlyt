# AI assistance

sunlyt's configuration is plain TOML, and every key is documented in [scene-reference.md](scene-reference.md). That makes it a good fit for an AI assistant (Claude, or any other) to edit, as long as you give it the reference and your own numbers, and check its work with `sunlyt check`. Unknown or misspelled keys are errors, so an invented setting fails loudly instead of being ignored.

## What to give the assistant

1. **The reference:** paste or attach [scene-reference.md](scene-reference.md), plus [cells.md](cells.md) for cell work.
2. **Your current files:** the scene TOML, and the cell TOML if you have one.
3. **What sunlyt sees now:** the output of `sunlyt check scene.toml`. It lists the OBJ group names, so the assistant doesn't have to read a large OBJ.
4. **Your source numbers:** the datasheet or ICD table, panel drawing dimensions, or wiring diagram, as text.

Don't paste export-controlled trajectory data or anything you couldn't put in an email. The scene and cell files don't need it.

## Prompts that work

**New panel layout from a drawing**

> Here is scene-reference.md, my scene.toml and the `sunlyt check` output. Add a `[[panel]]` on object `PANEL_PX` (cells face +X): 8 rows × 14 columns of 80 × 40 mm cells, long side along body Y, 1.5 mm gaps, grid centred 20 mm toward +Z from the face centre. Wire it as 14-cell strings along the rows. Change nothing else, and explain each value you set.

**Rewire strings**

> In scene.toml, change the `+Y wing` strings from 15 cells to 16 cells per string, walking down the columns instead of the rows. If 16 doesn't divide the cell count, say so and propose the nearest workable option instead of guessing.

**New cell from a datasheet or ICD**

> Here is cells.md and the electrical table from our cell datasheet (below). Write `cells/our-cell.toml` with a BOL and an EOL condition. Convert all temperature coefficients to V/K and A/K for the whole cell (the sheet gives them per cm² and in mV/°C). Put the document number and page in `source`. Don't invent values: if a field is missing, leave it out and tell me.

**Explain a check failure**

> `sunlyt check` says: `<paste the error>`. Here is my scene.toml. What is wrong and what is the smallest fix?

## The check loop

After every edit the assistant makes:

```bash
sunlyt check scene.toml --svg layout/
```

1. Read the printed face sizes, normals, cell counts and strings. Do they match the drawing?
2. Open `layout/<panel>.svg` and look at where the cells sit and how each string walks.
3. For cell files, also run `sunlyt cell cells/our-cell.toml`. Its printed Isc, Voc, Vmp and Imp should match your sheet to within about 1 %.
4. Only then run a trajectory.

## Claude Code skill

In this repository, Claude Code has a project skill, `sunlyt-config` (in `.claude/skills/`), that follows the loop above. Ask Claude Code something like "add a panel to PANEL_PX with …" and it reads the reference, edits the TOML, runs `sunlyt check --svg`, and reports what changed and what the check says.
