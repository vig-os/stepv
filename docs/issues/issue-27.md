---
type: issue
state: closed
created: 2026-10-04T21:41:09Z
updated: 2026-10-05T07:37:34Z
author: gerchowl
author_url: https://github.com/gerchowl
url: https://github.com/vig-os/stepv/issues/27
comments: 3
labels: feature, priority:medium
assignees: none
milestone: none
projects: none
parent: none
children: none
synced: 2026-10-05T08:45:48.133Z
---

# [Issue 27]: [Spike: #21 viewer stack — egui+wgpu (native) vs three.js (web)](https://github.com/vig-os/stepv/issues/27)

## Motivation

#21 has chosen option **B**, a cross-platform viewer: a model tree with show/hide, cross sections, and measurements on the exact B-rep. Linux has no viewer beyond `stepv view` (minifb plus the CPU rasteriser, which can only orbit), and macOS has nothing beyond Quick Look. Before building, we need to pick **the rendering and UI stack**, because it fixes:

- packaging (a GPU stack in the release binaries, or a JS bundle);
- how picking, sections and the tree get built;
- whether a hosted web preview later comes for free;
- the security story (#18: the kernel stays sandboxed; does the viewer add attack surface?).

## Decision

Two candidates, built as spike prototypes against **the same kernel output**: the STEPVMSH v3 mesh plus the `--topology` JSON from #26.

1. **egui + wgpu (eframe), native Rust.** `stepv view` opens a window. egui provides the tree, the panels and the measurement read-outs; wgpu renders the mesh. Picking works by ray against the triangles, then face id, then topology. A section is a clip plane in the shader. The same code can build to wasm later.
2. **A web viewer: three.js in a browser or webview.** `stepv view` runs the (sandboxed) kernel, serves the mesh and topology on `127.0.0.1` with a random token, and opens the default browser or a wry webview. three.js renders, with its `Raycaster` for picking, `clippingPlanes` for sections, and an HTML tree. The STEPVMSH decoder is either JS or Rust compiled to wasm.

**Best guess before the spike:** egui + wgpu. One language, one binary, no server, no JS toolchain in a nix and Rust repo, and the CPU rasteriser's camera and picking maths reused. The web route wins only if three.js's ready-made picking, clipping and UI save more than the localhost server, the JS build and the browser dependency cost.

## What already exists

- `src/occt.rs::read_mesh`: the STEPVMSH v3 decoder. `src/topology.rs` (#26): the topology types, with `check_against(&Scene)`.
- `src/render.rs`: `Camera`, `for_scene` (the face-on view), the view transform, and the broken-face overlay rules (amber stripes and red outlines from `FaceStatus`).
- `src/viewer.rs`: the minifb viewer and its pure `Controls` state machine, which is unit-tested.
- `src/glb.rs`: a glTF writer. three.js reads glTF natively, so the web path could load `--glb` instead of STEPVMSH.
- Kernel sandbox (#18): the viewer process itself is not sandboxed; only the kernel is.

## Scope

**P0 (the spike):**

- [ ] Prototype A (egui/wgpu): load mesh + topology, orbit and zoom, a model tree with per-part show/hide, a click on a face showing its surface (type, radius, area), and one clip plane.
- [ ] Prototype B (three.js): the same feature set, with `stepv` serving the data on localhost.
- [ ] Measure both:
  - added binary size;
  - added dependencies and build time;
  - cold start to first frame;
  - frame time on the 221 MB stress assembly (≈4M triangles?);
  - Linux runtime deps (Vulkan/GL, or a browser);
  - lines of code for the same features.
- [ ] A recommendation, plus an ADR-style record in plan.md.

**P1:** the chosen stack productised (a follow-up issue). **P2:** wasm or hosted preview.

## Pitfalls

- **wgpu on Linux:** needs Vulkan or GL drivers. VMs, CI and old Intel GPUs fall back to llvmpipe or fail. The minifb viewer works everywhere today, so B must not lose that.
- **Binary size:** wgpu + egui + winit add roughly 10–20 MB. The Quick Look extensions must not link it; that is the same feature-unification trap as minifb today.
- **The web route's local server:** it's an attack surface. It needs a token, a loopback bind, CORS refusal and a lifetime tied to the window. "Opens a browser tab" is a weaker UX than a window. A webview (wry) brings WebKitGTK on Linux, which is heavy and version-fragile.
- **JS toolchain:** three.js through npm and a bundler is new tooling for a nix/Rust repo. Vendoring a single ESM file avoids the bundler.
- **Large models:** 4M+ triangles, 240 parts and 40k faces. Per-face picking needs face ids per triangle on the GPU (an id buffer) or a CPU BVH. three.js's naive raycast over 4M triangles is slow without `three-mesh-bvh`.
- **Section capping:** both stacks clip easily but cap cut faces poorly (the stencil trick). Plan for clip-only in v1.
- **Measurement truth:** numbers must come from the topology, not the mesh. Shape-to-shape distance needs a kernel query, which means a viewer↔kernel channel. The sandboxed kernel is one-shot today.

## Acceptance

- [ ] Both prototypes run on macOS against `tests/data/assembly.step` and the stress assembly. At least one has been tried on Linux, in a container or through a software renderer.
- [ ] A table of the measurements above, with a direct recommendation.
- [ ] Follow-up issue(s) for productising the winner.

## References

#21 (the viewer), #26 (topology), #18 (sandbox), #19; `plan.md` §6 (occt-import-js as a web path); egui/eframe, wgpu, three.js `Raycaster` / `clippingPlanes`, three-mesh-bvh.

---
**Outcome:** egui + wgpu; see the results comment. Follow-ups: #28 scaffold + fallback + capi tripwire, #29 id-buffer picking + inspector, #30 virtualised tree, #31 overlay + edges (STEPVMSH v4), #32 CI lavapipe + packaging, #33 measurement query channel, #34 capping + fat lines. **Awaiting the maintainer accepting the verdict.**
---

# [Comment #1]() by [gerchowl]()

_Posted on October 4, 2026 at 09:43 PM_

## Round-1 reviews (design)

Three fresh reviews from different angles: a graphics engineer, a release-engineering and security reviewer, and a CAD-product and maintenance reviewer.

**They agree unanimously on egui + wgpu.** Their decisive reasons:

- **No localhost server.** It would add an unsandboxed listener: DNS rebinding, CSRF from other tabs, token and lifetime handling. That undoes #18's posture.
- **One Rust/nix toolchain.** No npm or vendored ESM supply chain, and no churn from three.js's r-number releases.
- **Reuse.** `Camera::for_scene`, the `Controls` state machine, the FaceStatus overlay rules and `Topology::check_against` are already tested Rust.
- **UX.** Users expect double-clicking a file to open a window, not a browser tab.
- **Optionality.** It is weaker than it looks: eframe builds to wasm, and a hosted viewer would be a different app (occt-import-js).

**Pitfalls added:**

- **Picking:** a GPU **id buffer** (R32Uint, `part<<20 | face`, one-texel readback) instead of a CPU or BVH raycast. three-mesh-bvh costs about 2–5 s and 100–200 MB on 4M triangles.
- **Fallback:** keep minifb as `--software`, auto-fall-back when no wgpu adapter exists, and report `"backend"` the way `"sandbox"` is reported.
- **Quick Look:** a CI tripwire asserting that `libstepv_capi.a` has no eframe, winit or wgpu symbols.
- **Linux:** a lavapipe smoke test; the tarball launcher's host-libs-second order is right for `libvulkan` and `libGL`.
- **Tree:** the model tree needs virtualised rows (`ScrollArea::show_rows`), not nested CollapsingHeaders, at about 10k nodes.
- **Dependencies:** `cargo deny` plus a binary-size budget, so a wgpu bump can't double the binary.
- **Edges:** thin LineList in v1, fat lines later. No capping in v1.

**What would flip them:** the egui tree stalling at 240 parts and 40k faces even when virtualised, accessibility becoming a requirement, or a frontend contributor who would own a JS lane.

**Next:** both prototypes get built and measured as P0 says. The egui one uses the id-buffer design; the three.js one exists for the numbers.

---

# [Comment #2]() by [gerchowl]()

_Posted on October 4, 2026 at 10:18 PM_

## Spike results

Both prototypes are on `feature/27-viewer-stack-spike` (`spikes/viewer-egui`, `spikes/viewer-web`). Each has a model tree with show/hide, a section clip plane, and face picking that shows the exact surface (type, radius, area) and the part's volume from `--topology`. Measured on an Apple Silicon Mac (sage), unless the row says Linux.

| | **A: egui 0.36 + wgpu 30** | **B: three.js 0.186 + loopback server** |
| --- | --- | --- |
| Code for the same features | 596 lines of Rust | 150 lines of Rust (server) + 182 of HTML/JS |
| Crates | 188 (stepv's own: 31) | 31 (the server is std only) |
| Binary | 12.3 MB macOS, 19.5 MB Linux (stepv today: 1.0 MB) | 1.7 MB, of which 0.83 MB is three.js |
| Clean release build | 185 s | 85 s |
| Small assembly: first frame after load | 0.2 s | 0.08 s after the page loads, but **5.6 s** to launch the browser |
| Stress assembly (240 parts, 1.77M triangles), Metal: **GPU-synced frame** at 1280×800 with MSAA | **0.86 ms** p50, 0.98 ms p95 (offscreen render plus `device.poll(Wait)`) | **≤ 1 ms** p50, 1 ms p95 (`render()` plus `gl.finish()`; `performance.now()` is clamped to 1 ms) |
| Stress assembly: interactive loop | CPU loop rate 1.6–2.4 ms with vsync off (submission only, *not* render cost) | a steady 60 fps, capped by rAF |
| Stress assembly: data to the viewer | in-process | 80 MB over loopback, fetched and parsed in 0.1 s |
| Stress assembly: pick latency, the same 81 rays | mean 0.0, max **0.2 ms** (CPU, per-part box culling) | mean 0.3, max **9 ms** (naive Raycaster, no BVH) |
| Stress assembly: memory | **2.08 GB** RSS, 1.4 GB footprint (the prototype keeps a CPU copy of every buffer; the product would drop it) | not measured (`ps` is entitlement-restricted here); the browser adds its own processes |
| Model tree, egui | 40k nodes in naive nested headers: **38 ms/frame**, 382 MB. Virtualised (`show_rows`): **2.2 ms**, 105 MB, the same as no tree | DOM `<details>` (not measured) |
| Kernel load of the stress assembly (same in both) | 23–25 s | 24 s |
| Linux, no GPU (Debian container, Xvfb) | lavapipe Vulkan: small 7 ms; stress **164 ms** (≈6 fps), usable but degraded. llvmpipe GL: 2.5–2.7 ms, discarded as a submission-only artefact | not tried (needs a browser in the container) |
| Linux build | the 4 GiB VM **OOM-killed** `rustc` on `ash` at default parallelism; it needed `-j2` | trivial |
| Repo tooling | fits | the vendored minified JS fails the typo and end-of-file hooks, so it is fetched and pinned by sha256 instead |
| API churn hit during the spike | egui 0.36 renamed `App::update` to `ui` and `SidePanel` to `Panel`; wgpu 30 made several descriptor fields `Option`; glam 0.34 removed `Mat4::perspective_rh` | none (pinned version) |
| Attack surface | none new | a loopback listener: token, Host check, CSP, lifetime |

**Methodology** (after the round-2 review): the first frame-time row compared a CPU submission rate with no vsync (A) against a vsync-capped rAF (B). It was replaced by GPU-synchronised timing on both at matched settings. Picks now use the same 81 rays on both. The browser's cold launch (5.6 s) would be sub-second with a warm browser.

## Verdict: egui + wgpu

The data decided it. The opinions only agreed with it.

- **Rendering speed is not the differentiator.** Both stacks draw the 1.77M-triangle stress assembly in **under 1 ms** of GPU time. Picking is fast enough in both, and the product will use an id buffer anyway.
- **What does differ: one process, one language, no listener.**
  - B needs a loopback server, which means token, Host-check, CSP and lifetime management. That is an unsandboxed listener next to a kernel #18 just sandboxed.
  - B needs a browser launch (5.6 s cold) or a webview (WebKitGTK on Linux).
  - B needs JS that fights the repo's hooks, and logic duplicated outside the tested Rust: `Camera::for_scene`, the FaceStatus overlay, the topology checks.
- **What A costs, and the plan for each:**
  - **188 crates and a 12–20 MB binary:** feature-gated as `viewer` and kept out of capi by a CI tripwire.
  - **API churn:** three breaking renames hit in one afternoon. Pin versions and budget for upgrades.
  - **A heavier Linux build:** the 4 GiB VM needed `-j2`.
  - **Software Vulkan is only usable, not good:** about 6 fps on the stress assembly. Keep minifb as the `--software` fallback.
- **The flip condition was tested and does not trigger.** A 40k-node egui tree virtualised with `show_rows` costs 2.2 ms per frame. Naive nested headers would have been the trap: 38 ms per frame.

**Round-2 reviews:**

1. **Methodology:** it caught that the first frame-time row compared CPU submission with vsync; it was fixed by the matched GPU-synced measurement above.
2. **Productisation:** keep prototype A as a reference, but build the product as a fresh `view::` module. It needs id-buffer picking, per-part SoA buffers with per-face material storage, an offscreen pass the viewer owns, a virtualised tree, and the overlay in WGSL. Do not port `Gpu::new` or `pick()`.

The follow-ups are filed below.


---

# [Comment #3]() by [gerchowl]()

_Posted on October 5, 2026 at 07:37 AM_

**Verdict accepted by the maintainer: egui + wgpu.** Productisation proceeds via #28–#34, starting with #28.

