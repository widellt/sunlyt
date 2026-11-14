---
name: sunlyt-physics-verification
description: How to verify any physics or numerics change in sunlyt (irradiance, eclipses, shadowing, PV cell and string models, thermal, interpolation). Use whenever adding or changing a model, constant, tolerance or test gate in crates/core, or when a result "looks off".
---

# Verifying physics in sunlyt

A physics change is done only when it is checked against something independent of the code under test. Passing a test written from the same formula proves nothing.

## The rules

1. **Every model gets an analytic case** that it reproduces to a stated tolerance, with the tolerance justified (rounding ≈ 1e-12 relative; formula approximation; sampling error).
2. **Every approximation gets a reference check** against a slower but exact method (brute-force sampling, a finer step, a closed form) over the range where it is used. Say in the doc comment what is neglected and how big it is.
3. **Invariants as tests:** bounds (fractions in [0, 1], power ≥ 0), monotonicity (ingress hides more), continuity (no jumps between steps), symmetry (rotating the frame changes nothing), limits (flattening → 0 gives the sphere).
4. **Every constant cites a source** in a comment: IAU 2015 B3 solar radius 695,700 km; S₀ = 1361 W/m² (Kopp & Lean 2011); WGS-84 a = 6378.137 km, 1/f = 298.257223563; IAU 2012 AU = 149,597,870,700 m; σ = 5.670374419e-8 W m⁻² K⁻⁴ (CODATA 2018).
5. **SI inside, km only at I/O.** Name fields with units (`_m`, `_w_m2`, `_k`).
6. **Precision traps:** use `AngleExt::angle_exact` and the local `slerp`, never glam's `angle_between`/`slerp` (~2e-8 rad error). Prefer `atan2` forms over `acos` near 0 and π.
7. **Real data only through `SUNLYT_SAMPLE_CSV`.** Never commit mission files or name the mission. Gates on real data print their numbers with `--nocapture`; `progress.md` records only that they passed, never the numbers, dates or row counts (they describe the mission). Keep the numbers outside the repo. Tests on real data assert invariants, not mission-specific values.

## Reference cases per model

| Model | Analytic / reference check |
|---|---|
| Irradiance | 1 AU → S₀ exactly; 2 AU → S₀/4 |
| Flat plate | P = S₀(1 AU/r)²·A·η·cos θ to 1e-9 relative; zero with the Sun behind |
| Disk occultation | No overlap 0, full cover 1, annular b²/a², two unit circles 1 apart: (2π/3 − √3/2)/π; curved limb vs. brute-force spherical sampling < 2e-3 |
| Eclipse geometry | Montenbruck & Gill, *Satellite Orbits* §3.4 (conical shadow); oblate limb grazing the pole hides half the disk |
| Several bodies on the disk | Separate parts add exactly (1 − f₁ − f₂). The same part covered twice counts once. A body inside another's shadow adds nothing. Two full eclipses give 0, never negative. Partial overlap is checked against brute-force union sampling (< 3e-3), and max fᵢ ≤ hidden ≤ Σ fᵢ always holds |
| Eclipse on real data | Rows with > 50 % hidden equal the count of rows with the Sun centre inside the limb |
| Cell I-V (slice 4) | Fit passes through datasheet Isc, Voc, Imp/Vmp at 28 °C AM0 within 1 %, checked with an MPP search that never sees the fit's slope condition. Lambert-W against Newton < 1e-12 A, with ideal and finite shunt. Golden values: W(1) = Ω, W(e) = 1, and the ideal-diode closed forms Voc = a·ln(Iph/I0 + 1), Vmp = a·(W(e(Iph+I0)/I0) − 1); pvlib `singlediode` (De Soto et al. 2006) when Python is available. Temperature: Voc and Isc follow the coefficients exactly, Pmp within 1 % over 28–140 °C. Golden-section search locates an argmax only to ~√ε; compare P, not V, at that precision |
| Strings (slice 4) | Series adds V at equal I, parallel adds I at equal V. Current stays continuous; a bypass diode clamps reverse voltage at −V_f; a half-shaded string matches a hand calculation |
| Shadowing (slice 3) | Cube over plate: lit fraction vs. geometric projection < 0.5 %. Sun-direction cache vs. ray caster < 0.5 % per cell. A cell that is self-shadowed during an eclipse gets cell lit fraction × disk visible fraction: one is never subtracted from the other, and the result is never < 0 |
| Thermal (slice 5) | Steady state T = (α S cos θ / ((ε_f + ε_b) σ))^¼ within 0.5 K, using the actual cos θ. Energy-balance residual ≈ 0 with P_elec. Eclipse cooling vs the exact T = (T0⁻³ + 3kt)^(−1/3). RK4 order: measure it in the asymptotic range (the T⁴ stiffness inflates ratios at coarse h). Planet flux: quadrature vs the closed-form plate-to-sphere view factor, and total albedo/IR through an enclosing sphere = A·S·πR² and (1 − A)·S·πR². Cadence: the panel time constant (~100 s) is shorter than 60 s rows, so check how row spacing changes T and say what dominates (here: trajectory interpolation, second order) |
| Energy conservation (any coupled model) | Electrical power taken off a panel must never exceed what it absorbs. A layout or preset that breaks this (e.g. tiny cells given full datasheet current) shows up as a runaway temperature, so test the coupled run on a deliberately mismatched scene |

## When a result looks off

Reduce to the smallest geometry that shows it. Compute the expected value by hand (or in a scratch test). Then find which assumption differs. Don't loosen a tolerance until you can explain the gap, and write that explanation next to the new tolerance.

## Performance changes

Speed work must leave the numbers unchanged:

1. **Measure before changing anything.** Use `sunlyt plate --timings` or the gate's serial/parallel benchmark, and record what you measured in `progress.md`.
2. **Parallel and serial runs must agree bit for bit** (`assert_eq!` on the samples).
3. **Prefer iteration, flat arrays, precomputed tables and caches keyed on the Sun direction.**
   - Rust does not guarantee tail calls, so recursion is never faster than a loop and can overflow the stack.
   - Trees such as the BVH are walked with an explicit stack.
4. **Sort or bin rows only when it lets work be reused** (e.g. grouping by Sun direction for the shadow cache). Results always go back into time order.
