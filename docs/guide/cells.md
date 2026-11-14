# Solar cells

The cell model is the single-diode model, fitted to a datasheet's four I-V points (Isc, Voc, Imp, Vmp) and moved to other temperatures with the datasheet's temperature coefficients. One cell type is used for the whole array; choose it with `[electrical] cell`.

## Built-in presets

| Name | Cell |
|---|---|
| `azur-3g30a-advanced-4x8` | AZUR SPACE 3G30A-Advanced 4x8 CIC (triple junction, 100 µm coverglass, bypass diode), from datasheet DB 00010891-01 |
| `solaero-imm-alpha` | SolAero IMM-α (inverted metamorphic, 32.0 % BOL), **per cm²**, from the April 2018 preliminary sheet. Conditions BOL, 5E14, 1E15, 5E15. Its temperature coefficients are projected rather than measured, and the test temperature isn't printed (28 °C assumed). See the comments in [the file](../../crates/core/presets/cells/solaero-imm-alpha.toml). |

Changing cells usually means changing the string length too. IMM-α gives about 3.9 V per cell at Vmp when hot, against 2.2 V for the AZUR cell, so 15-cell strings on a 28 V bus waste half their power. `sunlyt check` says so and suggests a length (8 here).

Look at a preset's fitted curve with:

```bash
sunlyt cell azur-3g30a-advanced-4x8 --condition BOL --temperature-c 60
```

## Your own cell file

Copy this template next to your scene, fill it from your datasheet or ICD, and point the scene at it with `cell = "my-cell.toml"`. Every number is the value printed on the datasheet, for one cell (or CIC), at the reference conditions.

```toml
name = "Example TJ cell 4x8"
source = "Vendor datasheet number, revision, page"  # where the numbers come from
area_cm2 = 30.18                    # cell area, cm²
reference_irradiance_w_m2 = 1367.0  # test irradiance (AM0), W/m²
reference_temperature_c = 28.0      # test temperature, °C
bypass_vf_v = 0.8                   # optional: bypass diode forward drop, V
# rsh_ohm = 5000                    # optional: shunt resistance, Ω (default: ideal)

[[condition]]
name = "BOL"                # beginning of life
voc_v = 2.700               # open-circuit voltage, V
isc_a = 0.515               # short-circuit current, A
vmp_v = 2.411               # maximum-power voltage, V
imp_a = 0.499               # maximum-power current, A
dvoc_dt_v_per_k = -0.0062   # temperature coefficient of Voc, V/K
disc_dt_a_per_k = 0.00036   # temperature coefficient of Isc, A/K
dvmp_dt_v_per_k = -0.0067   # temperature coefficient of Vmp, V/K
dimp_dt_a_per_k = 0.00024   # temperature coefficient of Imp, A/K

[[condition]]
name = "1E15"               # e.g. after 1E15 e/cm² of 1 MeV electrons (EOL)
voc_v = 2.480
isc_a = 0.505
vmp_v = 2.170
imp_a = 0.485
dvoc_dt_v_per_k = -0.0068
disc_dt_a_per_k = 0.00030
dvmp_dt_v_per_k = -0.0070
dimp_dt_a_per_k = 0.00020
```

(The `1E15` numbers are illustrative only. Use your datasheet's.)

### Datasheets that give values per cm²

Many sheets print current densities (Jsc, Jmp in mA/cm², coefficients in µA/cm²/°C) and no single cell size. Write the file for a **1 cm² cell**: `area_cm2 = 1.0`, `isc_a` = Jsc in A, coefficients in A/K. Your layout's `cell = [u, v]` then sets the real size, and sunlyt scales the current to that area (voltages unchanged). `solaero-imm-alpha` is built this way.

### EOL conditions from remaining factors

Some sheets give EOL as remaining factors (EOL/BOL ratios) instead of I-V points. Multiply: `voc_v` = BOL Voc × the Voc factor, and likewise for Isc, Vmp and Imp. As a check, Vmp × Imp should then reproduce the sheet's Pmp factor.

### When the fit needs a shunt

Typical datasheet values are averaged column by column, and very square cells (fill factor ~0.85) can't be matched by a single diode with series resistance alone. The fit then sets Rs = 0 and solves for a shunt resistance instead. That is the same model, so it still passes exactly through your four points. `sunlyt cell` prints which one it used (`Rs = 0.0000 Ω, Rsh = …`). If you know the real shunt, set `rsh_ohm` and the fit uses it.

### Where to find each number

| Field | On a typical space-cell datasheet |
|---|---|
| `voc_v`, `isc_a`, `vmp_v`, `imp_a` | "Electrical data" or "typical performance" table, one column per condition (BOL, 2.5E14, 5E14, 1E15 …) |
| `d…_dt_…_per_k` | "Temperature coefficients" table, per condition. Datasheets often print mV/°C or µA/(cm²·°C): convert to V/K and A/K for the whole cell (multiply per-cm² values by the area). |
| `area_cm2` | Cell area (not the glassed area, if both are given) |
| `reference_*` | The test conditions under the table, e.g. "AM0, 1367 W/m², 28 °C" |
| `bypass_vf_v` | Integrated bypass diode section |

**Checks after editing:**
- `sunlyt cell my-cell.toml` fits the model and prints your Isc, Voc, Vmp and Imp next to the fitted model's. They should agree to within about 1 %. If the four points are inconsistent (e.g. a typo, or Vmp ≥ Voc), the fit fails with a message naming the cell and condition.
- `sunlyt check scene.toml` then shows a full-sun string against your bus.

### What the model leaves out

- Spectral mismatch and angle-of-incidence losses beyond the cosine law.
- Cell-to-cell mismatch.
- Harness resistance.
- Bypass diodes shared by several cells: the model uses one per cell, as on CIC datasheets.

Raise an issue if your ICD specifies any of these; they are model changes, not configuration.
