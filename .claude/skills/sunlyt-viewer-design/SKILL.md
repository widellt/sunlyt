---
name: sunlyt-viewer-design
description: Interaction, motion and visual rules for the sunlyt 3D viewer (eframe/egui + wgpu), adapted from Emil Kowalski's design-engineering principles (emil-design-eng skill). Use when building or reviewing anything in crates/viewer, the timeline, plots or controls.
---

# sunlyt viewer design

The viewer should feel expensive: fast, quiet and exact. Most of its quality is in details nobody notices. This adapts Emil Kowalski's design-engineering principles (load the global `emil-design-eng` skill for the full reasoning) to an egui/wgpu desktop tool for engineers.

## Motion

- **Should it animate at all?** Ask how often the user sees it.
  - Scrubbing, stepping frames, keyboard shortcuts, play/pause: **never animate.** The display jumps to the new state on the same frame.
  - Panels opening and tooltips: brief.
  - Nothing in the viewer is decorative.
- **Simulation time is never eased.** The timeline, plots and 3D scene show exact simulated values. Easing applies only to UI chrome, never to data.
- **Durations stay under 300 ms.** Press feedback 100–160 ms, tooltips 125–200 ms, panels and popovers 150–250 ms.
- **Easing:**
  - Entering or exiting: strong ease-out, `cubic-bezier(0.23, 1, 0.32, 1)`.
  - Moving on screen: `cubic-bezier(0.77, 0, 0.175, 1)`.
  - Never use ease-in.
  - Put the curves in one `motion.rs` module and pass them to `ctx.animate_value_with_time`. Do not use egui's linear default.
- **Camera:** orbit and zoom use critically damped springs (no overshoot) so motion keeps momentum and can be interrupted. Camera presets ("look from Sun", "+X") glide in ≤ 250 ms; double-click re-centres the camera instantly.
- **Exits run faster than entries.**
- **Reduced motion:** a setting (default from the OS where available) turns off camera glides and panel slides but keeps opacity fades.

## Feedback

- **Latency budget: every input shows a visible response within 200 ms; aim for the next frame (≤ 16 ms).** Scrubbing reads cached results. A step that has to be recomputed shows the cached neighbour at once and swaps in the exact value when it arrives. Work that takes longer than 200 ms shows progress immediately.
- Pressed buttons scale to 0.97 or darken immediately. Every control responds on the frame it is pressed.
- Tooltips wait about 400 ms before appearing. Once one is open, neighbouring tooltips appear instantly with no animation.
- Long jobs (baking the shadow cache, a full run) show real progress, never a spinner alone.

## Visual

- **Cell colour maps are perceptually uniform** (viridis/cividis for illumination, a diverging map for current). Every map has a legend with units.
- **Eclipse states are labelled.** Mark umbra and penumbra with shaded bands on the timeline, as well as by colour, so they read for colour-blind users.
- **Numbers:**
  - Use tabular figures with fixed decimals so values don't jitter while playing.
  - Always show units.
  - Show UTC timestamps to the second.
- **Theme:** colours are defined once (light and dark) and referenced by role (`surface`, `text`, `accent`, `umbra`). No hard-coded colours in widgets.
- **Defaults are good enough** that users never need to change them: sensible camera framing on load, speed 60×, and the power plot visible.

## Plots and data

Plots (power vs. time, eclipse bands, per-string I-V) follow the installed `dataviz` skill:

- One y-axis per chart. Power and temperature go in stacked charts that share the time axis, never on a dual axis.
- Categorical series colours (strings, panels) come in a fixed order from a palette checked with `scripts/validate_palette.js` in light and dark mode. Colour follows the entity and never changes when filters change.
- Line and area plots get a crosshair and tooltip that snap to the nearest step and show UTC plus values with units.
- Charts with two or more series have a legend. Up to four series are also labelled directly on the plot.
- Every plot can be exported as a table (CSV) with the same numbers that are drawn.

## Review format

When reviewing viewer code, report issues as a single table:

| Before | After | Why |
|---|---|---|
| Camera preset snaps | 220 ms ease-out glide | Spatial continuity |
| Plot cursor eased toward scrub position | Cursor set on the same frame | Data must never lag input |
