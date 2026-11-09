# ADR-0001 census: what the Rust fast path could cover

- **Measured:** 2026-11-09, by slice G2 (#134), on sage (Mac Studio, Apple Silicon), release build.
- **Command:** `nix develop -c cargo run --release --example graph_census -- tests/fixtures <Ultimaker 3>`
  (`examples/graph_census.rs`). It reads only the graph; no OCCT runs.
- **Replaces** the grep estimate in ADR-0001 §1 and the product-share guess in §2.6.

## Which files

- **Counted: 391 files.** The fetched corpus (`tests/fixtures`, 397 `.step`/`.stp` files) minus the
  7 malformed fixtures that refuse, plus Ultimaker 3 (143 MB, kept outside the repo):
  - `abc-dataset`: 300 files;
  - `nist-pmi`: 33;
  - `occt-import-js`: 55;
  - `stress-assembly`: 1 (`nist_ctc_02_asme1_rc-x60.step`, 221 MB);
  - Ultimaker 3: 1;
  - `malformed/header-only.step`: 1. It is valid Part 21 with an empty `DATA` section, so it is
    counted, with no entities.
- **Skipped: 7 files, all in `tests/fixtures/malformed/`,** each refused with a byte offset:
  `bitflipped.step` (byte 3172, expected `(`), `empty.step`, `not-step-png.step`,
  `random-bytes.step` and `whitespace.step` (not Part 21), `truncated-half.step` (byte 117585) and
  `truncated-header.step` (byte 140) (no `ENDSEC`).
- **Not on this host:** the exporter matrix (`tests/fixtures/manifest.toml`), which isn't fetched.
- **G2's acceptance on every counted file:** every entity's arguments, and every part's of every
  complex entity, parse, and the references they hold are exactly `refs`. **0 failures** over
  10,648,223 entities.

## What is counted

- **Faces:** `ADVANCED_FACE`s whose surface is `PLANE`, `CYLINDRICAL_SURFACE`, `CONICAL_SURFACE`,
  `SPHERICAL_SURFACE` or `TOROIDAL_SURFACE` (§2.6's faces in scope). Each face is counted once,
  so this is a face share, not §1's share of surface entities.
- **Edges:** `EDGE_CURVE`s whose 3-D curve, direct or through a `SURFACE_CURVE`/`SEAM_CURVE`, is an
  MVP curve: `LINE`, `CIRCLE` or `ELLIPSE` (analytic), or a `B_SPLINE_CURVE_WITH_KNOTS`, simple or
  complex (rational or not). P-curves are never counted.
- **Representations, not products, until G3.** "The share of products all of whose faces are
  analytic" needs the product → faces mapping, which is G3's structure. Until then the census
  counts per shape representation: each entity with a `…SHAPE_REPRESENTATION` part that reaches at
  least one `ADVANCED_FACE`, its faces found by reachability over `refs` without entering another
  representation (so a `MAPPED_ITEM`'s target counts once, for its own representation).
  - **In scope:** every face of the representation has an analytic surface and only MVP edges
    (§2.6).
  - In the ABC files a representation is a product: each file holds several products, one
    `ADVANCED_BREP_SHAPE_REPRESENTATION` each. In the assemblies, products without geometry of
    their own have no representation with faces and aren't counted.
  - Re-running this per product after G3 is filed as #153.

## Results

| Set | Files | Faces | Analytic faces | Representations with faces | In scope |
| --- | ---: | ---: | ---: | ---: | ---: |
| `abc-dataset` | 300 | 95,030 | 81.9% | 1,810 | 75.3% |
| `nist-pmi` | 33 | 7,902 | 97.8% | 71 | 69.0% |
| `occt-import-js` | 55 | 836 | 71.8% | 99 | 49.5% |
| stress assembly | 1 | 39,840 | 94.9% | 120 | 50.0% |
| Ultimaker 3 | 1 | 34,840 | 89.9% | 199 | 55.3% |
| **Total** | **391** | **178,448** | **87.0%** (155,276) | **2,299** | **70.9%** (1,631) |

- **Edges:** 430,501 of 430,532 `EDGE_CURVE`s are MVP edges (**100.0%**): 80.5% analytic, 19.5%
  B-spline. The other 31 are `QUASI_UNIFORM_CURVE`s, a B-spline form without explicit knots that
  §2.6 doesn't list. So in this corpus the edges never take a representation out of scope; the
  surfaces do, and the "in scope" share equals the "all surfaces analytic" share in every file.
- **Out by one or two faces:** 204 representations (8.9%) miss scope by one or two faces. That is
  the number §2.6 names as the trigger for per-face routing (v2).
- **What takes faces out of scope** (23,172 faces):

  | Surface | Faces |
  | --- | ---: |
  | complex rational B-spline (`BOUNDED_SURFACE … RATIONAL_B_SPLINE_SURFACE …`) | 11,755 |
  | `B_SPLINE_SURFACE_WITH_KNOTS` | 7,717 |
  | `SURFACE_OF_REVOLUTION` | 2,423 |
  | `SURFACE_OF_LINEAR_EXTRUSION` | 1,229 |
  | `OFFSET_SURFACE` | 43 |
  | `QUASI_UNIFORM_SURFACE` | 5 |

**Against §2.6's guess** (50–70% of products in mechanical assemblies, far less in ABC):
- the mechanical assemblies come out at the guess's low end: Ultimaker 55.3%, the stress assembly
  50.0%, `nist-pmi` 69.0%;
- ABC comes out *above* the guess, at 75.3%, not far below it. Its files hold several products each,
  many of them prismatic;
- the face share, 87.0%, is above §1's 80% entity estimate, which counted the B-spline surfaces
  that p-curves and unused geometry also reference.

## Time and heap (§2.7)

Measured by the same run: the scan with the forward CSR, and the heap beyond the map, without and
with `referrers` (the reverse CSR built on first use).

| Measure | Ultimaker 3 | Stress assembly | Target |
| --- | ---: | ---: | --- |
| entities | 1,517,999 | 3,283,040 | |
| scan: entity table + CSR | 0.53 s | 0.92 s | ≤ 1.0 s / ≤ 1.5 s |
| heap without `referrers` | 30.6 B per entity (46 MB) | 30.5 B per entity (100 MB) | ≤ 36 B, ≤ 120 MB |
| heap with `referrers` | 39.2 B per entity (60 MB) | 39.0 B per entity (128 MB) | ≤ 45 B, ≤ 150 MB |
| whole corpus, serial | 3.5 s of scanning over 391 files | | ≤ 20 s |

All within target. Ultimaker has 1.52 M entities, not the 2.1 M §1 extrapolated from the stress
file's density. The run's peak resident set was 453 MB (the map of the largest file plus its graph).

## Per file

| file | entities | faces | analytic faces | edges | MVP edges (analytic + B-spline) | reps | reps, analytic surfaces | reps in scope |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| tests/fixtures/abc-dataset/00000000_290a9120f9f249a7a05cfe9c_step_000.step | 863 | 25 | 92.0% | 33 | 100.0% (100.0% + 0.0%) | 6 | 83.3% | 83.3% |
| tests/fixtures/abc-dataset/00000001_1ffb81a71e5b402e966b9341_step_000.step | 7555 | 103 | 58.3% | 246 | 100.0% (56.5% + 43.5%) | 2 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000002_1ffb81a71e5b402e966b9341_step_001.step | 2545 | 44 | 100.0% | 44 | 100.0% (54.5% + 45.5%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000003_1ffb81a71e5b402e966b9341_step_002.step | 3491 | 25 | 68.0% | 38 | 100.0% (68.4% + 31.6%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000004_1ffb81a71e5b402e966b9341_step_003.step | 3212 | 48 | 77.1% | 108 | 100.0% (72.2% + 27.8%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000005_d4fe04f0f5f84b52bd4f10e4_step_000.step | 2155 | 60 | 100.0% | 120 | 100.0% (100.0% + 0.0%) | 10 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000006_d4fe04f0f5f84b52bd4f10e4_step_001.step | 595 | 15 | 100.0% | 32 | 100.0% (62.5% + 37.5%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000007_b33a147f86da49879455d286_step_000.step | 99 | 3 | 100.0% | 2 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000008_9b3d6a97e8de4aa193b81000_step_000.step | 699 | 21 | 100.0% | 34 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000009_9b3d6a97e8de4aa193b81000_step_001.step | 183 | 6 | 100.0% | 5 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000010_b4b99d35e04b4277931f9a9c_step_000.step | 4487 | 75 | 84.0% | 166 | 100.0% (74.1% + 25.9%) | 3 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000011_e909f412cda24521865fac0f_step_000.step | 107289 | 253 | 17.4% | 651 | 100.0% (29.3% + 70.7%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000012_f16882934f314832b639ffc0_step_000.step | 107289 | 253 | 17.4% | 651 | 100.0% (29.3% + 70.7%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000013_c2f4d27f35ed4c138caf5c18_step_000.step | 4356 | 99 | 72.7% | 205 | 100.0% (76.6% + 23.4%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000014_5b1c2f8a8c6f40fdaae1e69d_step_000.step | 9116 | 214 | 89.3% | 434 | 100.0% (92.4% + 7.6%) | 7 | 42.9% | 42.9% |
| tests/fixtures/abc-dataset/00000015_b08aa818955948c690fd9b6d_step_000.step | 15300 | 42 | 57.1% | 86 | 100.0% (44.2% + 55.8%) | 3 | 33.3% | 33.3% |
| tests/fixtures/abc-dataset/00000016_b08aa818955948c690fd9b6d_step_001.step | 1009 | 29 | 100.0% | 51 | 100.0% (100.0% + 0.0%) | 3 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000017_e8e79d1385fd4c78be414f6c_step_000.step | 13455 | 384 | 100.0% | 768 | 100.0% (100.0% + 0.0%) | 64 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000018_e8e79d1385fd4c78be414f6c_step_001.step | 7417 | 194 | 100.0% | 486 | 100.0% (100.0% + 0.0%) | 16 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000019_e8e79d1385fd4c78be414f6c_step_002.step | 7417 | 194 | 100.0% | 486 | 100.0% (100.0% + 0.0%) | 16 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000020_ad34a3f60c4a4caa99646600_step_000.step | 1196 | 29 | 93.1% | 70 | 100.0% (80.0% + 20.0%) | 2 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000021_ad34a3f60c4a4caa99646600_step_001.step | 545 | 3 | 0.0% | 2 | 100.0% (0.0% + 100.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000022_ad34a3f60c4a4caa99646600_step_002.step | 225 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000023_ad34a3f60c4a4caa99646600_step_003.step | 52343 | 454 | 37.4% | 1307 | 100.0% (20.8% + 79.2%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000024_ad34a3f60c4a4caa99646600_step_004.step | 1437 | 30 | 96.7% | 73 | 100.0% (95.9% + 4.1%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000025_ad34a3f60c4a4caa99646600_step_005.step | 3345 | 81 | 98.8% | 194 | 100.0% (98.5% + 1.5%) | 3 | 66.7% | 66.7% |
| tests/fixtures/abc-dataset/00000026_ad34a3f60c4a4caa99646600_step_006.step | 36905 | 202 | 68.3% | 575 | 100.0% (40.7% + 59.3%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000027_ad34a3f60c4a4caa99646600_step_007.step | 3434 | 74 | 75.7% | 216 | 100.0% (55.1% + 44.9%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000028_ad34a3f60c4a4caa99646600_step_008.step | 4428 | 99 | 81.8% | 263 | 100.0% (65.0% + 35.0%) | 3 | 66.7% | 66.7% |
| tests/fixtures/abc-dataset/00000029_ad34a3f60c4a4caa99646600_step_009.step | 407 | 14 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000030_ad34a3f60c4a4caa99646600_step_010.step | 12150 | 115 | 37.4% | 306 | 100.0% (21.2% + 78.8%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000031_ad34a3f60c4a4caa99646600_step_011.step | 941 | 27 | 100.0% | 49 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000032_ad34a3f60c4a4caa99646600_step_012.step | 3149 | 82 | 100.0% | 200 | 100.0% (83.0% + 17.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000033_ad34a3f60c4a4caa99646600_step_013.step | 58327 | 1420 | 43.5% | 3216 | 100.0% (7.4% + 92.6%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000034_ad34a3f60c4a4caa99646600_step_014.step | 1552 | 42 | 100.0% | 92 | 100.0% (93.5% + 6.5%) | 3 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000035_6763f7e2f51a489caaf599f0_step_000.step | 3137 | 82 | 100.0% | 188 | 100.0% (97.9% + 2.1%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000036_b169abf5f2444251b529c688_step_000.step | 4562 | 109 | 97.2% | 298 | 100.0% (75.5% + 24.5%) | 2 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000037_c7d977f326364e35bb5b5d27_step_000.step | 12421 | 193 | 85.5% | 420 | 100.0% (86.2% + 13.8%) | 7 | 85.7% | 85.7% |
| tests/fixtures/abc-dataset/00000038_c7d977f326364e35bb5b5d27_step_001.step | 665 | 17 | 100.0% | 25 | 100.0% (92.0% + 8.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000039_c7d977f326364e35bb5b5d27_step_002.step | 485 | 16 | 100.0% | 16 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000040_c7d977f326364e35bb5b5d27_step_003.step | 46975 | 359 | 41.5% | 838 | 100.0% (49.9% + 50.1%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000041_c7d977f326364e35bb5b5d27_step_004.step | 39809 | 193 | 62.2% | 461 | 100.0% (55.3% + 44.7%) | 5 | 20.0% | 20.0% |
| tests/fixtures/abc-dataset/00000042_c7d977f326364e35bb5b5d27_step_005.step | 873 | 23 | 100.0% | 38 | 100.0% (97.4% + 2.6%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000043_c7d977f326364e35bb5b5d27_step_006.step | 11834 | 36 | 75.0% | 70 | 100.0% (45.7% + 54.3%) | 7 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000044_c7d977f326364e35bb5b5d27_step_007.step | 371060 | 6081 | 75.3% | 17500 | 100.0% (62.2% + 37.8%) | 94 | 39.4% | 39.4% |
| tests/fixtures/abc-dataset/00000045_c7d977f326364e35bb5b5d27_step_008.step | 23522 | 334 | 93.4% | 971 | 100.0% (30.8% + 69.2%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000046_1a67c6032bbd479492910b39_step_000.step | 105183 | 2505 | 100.0% | 5010 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000047_1a67c6032bbd479492910b39_step_001.step | 3523 | 123 | 99.2% | 114 | 100.0% (100.0% + 0.0%) | 12 | 91.7% | 91.7% |
| tests/fixtures/abc-dataset/00000048_efe12a13ac2e4ea7880a66d1_step_000.step | 105225 | 2506 | 100.0% | 5012 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000049_aa208c45e8734584a147f595_step_000.step | 2301 | 76 | 100.0% | 98 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000050_80d90bfdd2e74e709956122a_step_000.step | 1671 | 43 | 100.0% | 98 | 100.0% (100.0% + 0.0%) | 3 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000051_666139e3bff64d4e8a6ce183_step_000.step | 4603 | 119 | 100.0% | 222 | 100.0% (98.2% + 1.8%) | 5 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000052_666139e3bff64d4e8a6ce183_step_001.step | 1107 | 34 | 100.0% | 24 | 100.0% (100.0% + 0.0%) | 14 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000053_666139e3bff64d4e8a6ce183_step_002.step | 4680 | 40 | 92.5% | 153 | 100.0% (45.1% + 54.9%) | 2 | 50.0% | 50.0% |
| tests/fixtures/abc-dataset/00000054_666139e3bff64d4e8a6ce183_step_003.step | 3432 | 72 | 100.0% | 210 | 100.0% (35.7% + 64.3%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000055_666139e3bff64d4e8a6ce183_step_004.step | 316973 | 7509 | 96.5% | 16639 | 100.0% (72.2% + 27.8%) | 252 | 91.3% | 91.3% |
| tests/fixtures/abc-dataset/00000056_666139e3bff64d4e8a6ce183_step_005.step | 5134 | 111 | 95.5% | 238 | 100.0% (86.6% + 13.4%) | 6 | 66.7% | 66.7% |
| tests/fixtures/abc-dataset/00000057_666139e3bff64d4e8a6ce183_step_006.step | 2002 | 49 | 100.0% | 135 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000058_666139e3bff64d4e8a6ce183_step_007.step | 1171 | 35 | 100.0% | 57 | 100.0% (57.9% + 42.1%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000059_767e4372b5f94a88a7a17d90_step_000.step | 2669 | 68 | 100.0% | 180 | 100.0% (100.0% + 0.0%) | 3 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000060_767e4372b5f94a88a7a17d90_step_001.step | 3129 | 76 | 100.0% | 216 | 100.0% (99.1% + 0.9%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000061_767e4372b5f94a88a7a17d90_step_002.step | 268 | 7 | 100.0% | 15 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000062_767e4372b5f94a88a7a17d90_step_003.step | 3397 | 82 | 100.0% | 234 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000063_767e4372b5f94a88a7a17d90_step_004.step | 225 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000064_767e4372b5f94a88a7a17d90_step_005.step | 7811 | 27 | 25.9% | 57 | 100.0% (47.4% + 52.6%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000065_767e4372b5f94a88a7a17d90_step_006.step | 2122 | 55 | 100.0% | 135 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000066_767e4372b5f94a88a7a17d90_step_007.step | 4893 | 39 | 64.1% | 98 | 100.0% (78.6% + 21.4%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000067_767e4372b5f94a88a7a17d90_step_008.step | 4897 | 38 | 57.9% | 92 | 100.0% (75.0% + 25.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000068_767e4372b5f94a88a7a17d90_step_009.step | 901 | 22 | 100.0% | 56 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000069_767e4372b5f94a88a7a17d90_step_010.step | 1439 | 36 | 100.0% | 92 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000070_767e4372b5f94a88a7a17d90_step_011.step | 561 | 14 | 100.0% | 36 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000071_767e4372b5f94a88a7a17d90_step_012.step | 3201 | 77 | 100.0% | 218 | 100.0% (96.3% + 3.7%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000072_767e4372b5f94a88a7a17d90_step_013.step | 1155 | 28 | 100.0% | 78 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000073_767e4372b5f94a88a7a17d90_step_014.step | 1052 | 27 | 100.0% | 69 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000074_767e4372b5f94a88a7a17d90_step_015.step | 3585 | 102 | 100.0% | 204 | 100.0% (100.0% + 0.0%) | 17 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000075_767e4372b5f94a88a7a17d90_step_016.step | 3237 | 90 | 100.0% | 192 | 100.0% (100.0% + 0.0%) | 13 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000076_767e4372b5f94a88a7a17d90_step_017.step | 1649 | 46 | 100.0% | 96 | 100.0% (100.0% + 0.0%) | 7 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000077_767e4372b5f94a88a7a17d90_step_018.step | 587 | 16 | 100.0% | 35 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000078_767e4372b5f94a88a7a17d90_step_019.step | 791 | 21 | 100.0% | 46 | 100.0% (95.7% + 4.3%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000079_767e4372b5f94a88a7a17d90_step_020.step | 747 | 19 | 100.0% | 48 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000080_ed3cecad25c64f7abea4cf64_step_000.step | 2705 | 70 | 100.0% | 186 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000081_201dac73881b409b8d57f008_step_000.step | 1734 | 32 | 100.0% | 101 | 100.0% (98.0% + 2.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000082_55b775e2dd7149a8bd9c9de7_step_000.step | 60564 | 1498 | 97.0% | 3110 | 100.0% (97.0% + 3.0%) | 64 | 95.3% | 95.3% |
| tests/fixtures/abc-dataset/00000083_9ea934d3400e45fb9d190941_step_000.step | 19590 | 85 | 22.4% | 192 | 100.0% (22.9% + 77.1%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000084_9a051df11afa4fc5a1d667cd_step_000.step | 102822 | 861 | 56.8% | 1819 | 100.0% (53.2% + 46.8%) | 22 | 36.4% | 36.4% |
| tests/fixtures/abc-dataset/00000085_b4f9335a60a14cecaa599159_step_000.step | 65848 | 1415 | 87.9% | 3545 | 100.0% (83.8% + 16.2%) | 14 | 78.6% | 78.6% |
| tests/fixtures/abc-dataset/00000086_b4f9335a60a14cecaa599159_step_001.step | 1276 | 6 | 83.3% | 4 | 100.0% (100.0% + 0.0%) | 2 | 50.0% | 50.0% |
| tests/fixtures/abc-dataset/00000087_b4f9335a60a14cecaa599159_step_002.step | 2042 | 3 | 66.7% | 4 | 100.0% (100.0% + 0.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000088_169bec34317b46e98eb1a403_step_000.step | 2612 | 61 | 100.0% | 172 | 100.0% (95.9% + 4.1%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000089_169bec34317b46e98eb1a403_step_001.step | 5131 | 126 | 100.0% | 358 | 100.0% (100.0% + 0.0%) | 3 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000090_169bec34317b46e98eb1a403_step_002.step | 890 | 15 | 80.0% | 33 | 100.0% (87.9% + 12.1%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000091_169bec34317b46e98eb1a403_step_003.step | 1284 | 30 | 100.0% | 75 | 100.0% (96.0% + 4.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000092_169bec34317b46e98eb1a403_step_004.step | 55 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/abc-dataset/00000093_169bec34317b46e98eb1a403_step_005.step | 745 | 18 | 100.0% | 44 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000094_a47a98a09959465188a093ad_step_000.step | 107289 | 253 | 17.4% | 651 | 100.0% (29.3% + 70.7%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000095_79c7c1db73274f22b599eef0_step_000.step | 795 | 22 | 100.0% | 0 | – (– + –) | 22 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000096_fcd027a9411a4ae7ac146826_step_000.step | 13367 | 308 | 96.1% | 805 | 100.0% (13.3% + 86.7%) | 2 | 50.0% | 50.0% |
| tests/fixtures/abc-dataset/00000097_a6920073c7fe448584f0d647_step_000.step | 10003 | 213 | 99.1% | 564 | 100.0% (25.5% + 74.5%) | 3 | 66.7% | 66.7% |
| tests/fixtures/abc-dataset/00000098_72cf0f9ca5324822a4d41a80_step_000.step | 8896 | 137 | 54.0% | 363 | 100.0% (17.9% + 82.1%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000099_7911d7c4c4b9417d92801ff6_step_000.step | 13993 | 358 | 100.0% | 920 | 100.0% (100.0% + 0.0%) | 17 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000100_5ed74bccca6f4e89829bcb5e_step_000.step | 13442 | 227 | 85.9% | 535 | 100.0% (83.2% + 16.8%) | 7 | 42.9% | 42.9% |
| tests/fixtures/abc-dataset/00000101_5ed74bccca6f4e89829bcb5e_step_001.step | 1733 | 44 | 97.7% | 98 | 100.0% (87.8% + 12.2%) | 5 | 80.0% | 80.0% |
| tests/fixtures/abc-dataset/00000102_5ed74bccca6f4e89829bcb5e_step_002.step | 99 | 3 | 100.0% | 2 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000103_52f7124261ae4c558f800775_step_000.step | 379 | 13 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000104_52f7124261ae4c558f800775_step_001.step | 337 | 11 | 100.0% | 11 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000105_52f7124261ae4c558f800775_step_002.step | 447 | 11 | 100.0% | 25 | 100.0% (68.0% + 32.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000106_52f7124261ae4c558f800775_step_003.step | 645 | 16 | 100.0% | 42 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000107_601f37ef630741969a417e51_step_000.step | 1619 | 42 | 100.0% | 108 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000108_ce24e0cbec8e45c89735d148_step_000.step | 26078 | 674 | 98.2% | 1549 | 100.0% (72.4% + 27.6%) | 13 | 92.3% | 92.3% |
| tests/fixtures/abc-dataset/00000109_ce24e0cbec8e45c89735d148_step_001.step | 69841 | 1356 | 86.7% | 2473 | 100.0% (71.9% + 28.1%) | 3 | 33.3% | 33.3% |
| tests/fixtures/abc-dataset/00000110_ce24e0cbec8e45c89735d148_step_002.step | 18162 | 409 | 95.1% | 941 | 100.0% (93.3% + 6.7%) | 9 | 88.9% | 88.9% |
| tests/fixtures/abc-dataset/00000111_ce24e0cbec8e45c89735d148_step_003.step | 1805 | 44 | 100.0% | 108 | 100.0% (70.4% + 29.6%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000112_ce24e0cbec8e45c89735d148_step_004.step | 1051 | 31 | 100.0% | 53 | 100.0% (77.4% + 22.6%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000113_ce24e0cbec8e45c89735d148_step_005.step | 3727 | 88 | 100.0% | 178 | 100.0% (79.8% + 20.2%) | 3 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000114_ce24e0cbec8e45c89735d148_step_006.step | 15734 | 371 | 100.0% | 991 | 100.0% (60.4% + 39.6%) | 3 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000115_ce24e0cbec8e45c89735d148_step_007.step | 8022 | 184 | 100.0% | 483 | 100.0% (78.7% + 21.3%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000116_ce24e0cbec8e45c89735d148_step_008.step | 2677 | 92 | 100.0% | 106 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000117_ce24e0cbec8e45c89735d148_step_009.step | 1921 | 65 | 100.0% | 79 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000118_ce24e0cbec8e45c89735d148_step_010.step | 15987 | 552 | 100.0% | 636 | 100.0% (100.0% + 0.0%) | 6 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000119_ce24e0cbec8e45c89735d148_step_011.step | 225 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000120_ce24e0cbec8e45c89735d148_step_012.step | 741 | 14 | 28.6% | 17 | 100.0% (76.5% + 23.5%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000121_43c44d81ee2f44428314b518_step_000.step | 1993 | 59 | 100.0% | 102 | 100.0% (100.0% + 0.0%) | 7 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000122_43c44d81ee2f44428314b518_step_001.step | 13080 | 118 | 78.0% | 432 | 100.0% (25.9% + 74.1%) | 5 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000123_b70e8b6a4f0046dc80a97c57_step_000.step | 99 | 3 | 100.0% | 2 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000124_807e1c7fc3594af09fd6f7c4_step_000.step | 13507 | 138 | 92.8% | 274 | 100.0% (78.1% + 21.9%) | 2 | 50.0% | 50.0% |
| tests/fixtures/abc-dataset/00000125_e18c7feacfb54f3880659b43_step_000.step | 2851 | 66 | 100.0% | 176 | 100.0% (4.5% + 95.5%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000126_e18c7feacfb54f3880659b43_step_001.step | 551 | 15 | 100.0% | 29 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000127_e18c7feacfb54f3880659b43_step_002.step | 1991 | 48 | 100.0% | 120 | 100.0% (96.7% + 3.3%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000128_e18c7feacfb54f3880659b43_step_003.step | 1933 | 57 | 100.0% | 105 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000129_e18c7feacfb54f3880659b43_step_004.step | 4695 | 35 | 82.9% | 74 | 100.0% (78.4% + 21.6%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000130_e18c7feacfb54f3880659b43_step_005.step | 4695 | 35 | 82.9% | 74 | 100.0% (78.4% + 21.6%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000131_945244874ed84e599987daa9_step_000.step | 141 | 4 | 100.0% | 4 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000132_438507a38e404bfdbd51a16b_step_000.step | 141 | 4 | 100.0% | 4 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000133_08b739e0511f4305b161cd05_step_000.step | 141 | 4 | 100.0% | 4 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000134_1418f3c3ca954e799c8860af_step_000.step | 225 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000135_5c71ed27ac0945e4933d7c76_step_000.step | 6055 | 95 | 87.4% | 219 | 100.0% (90.9% + 9.1%) | 3 | 66.7% | 66.7% |
| tests/fixtures/abc-dataset/00000136_5c71ed27ac0945e4933d7c76_step_001.step | 1749 | 45 | 100.0% | 88 | 100.0% (81.8% + 18.2%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000137_2fc54fcd110d4f49969163c4_step_000.step | 1566 | 39 | 100.0% | 89 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000138_2fc54fcd110d4f49969163c4_step_001.step | 6405 | 170 | 100.0% | 380 | 100.0% (100.0% + 0.0%) | 5 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000139_2fc54fcd110d4f49969163c4_step_002.step | 1113 | 30 | 93.3% | 36 | 100.0% (94.4% + 5.6%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000140_2fc54fcd110d4f49969163c4_step_003.step | 943 | 25 | 100.0% | 52 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000141_2fc54fcd110d4f49969163c4_step_004.step | 1614 | 40 | 100.0% | 103 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000142_2fc54fcd110d4f49969163c4_step_005.step | 796 | 19 | 100.0% | 50 | 100.0% (96.0% + 4.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000143_2fc54fcd110d4f49969163c4_step_006.step | 2375 | 28 | 89.3% | 72 | 100.0% (93.1% + 6.9%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000144_2fc54fcd110d4f49969163c4_step_007.step | 183 | 5 | 100.0% | 6 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000145_2fc54fcd110d4f49969163c4_step_008.step | 1025 | 25 | 100.0% | 68 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000146_1d9ece3a9c8348a5a11e7153_step_000.step | 569 | 10 | 40.0% | 24 | 100.0% (50.0% + 50.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000147_d9a2aa6d24764b809c265460_step_000.step | 32659 | 717 | 88.4% | 1633 | 100.0% (95.5% + 4.5%) | 11 | 72.7% | 72.7% |
| tests/fixtures/abc-dataset/00000148_d9a2aa6d24764b809c265460_step_001.step | 32550 | 881 | 91.9% | 1277 | 100.0% (90.6% + 9.4%) | 21 | 38.1% | 38.1% |
| tests/fixtures/abc-dataset/00000149_d9a2aa6d24764b809c265460_step_002.step | 32714 | 875 | 91.9% | 1257 | 100.0% (91.1% + 8.9%) | 21 | 38.1% | 38.1% |
| tests/fixtures/abc-dataset/00000150_a951550b8a1043a68423c541_step_000.step | 16092 | 443 | 99.3% | 881 | 100.0% (99.0% + 1.0%) | 24 | 95.8% | 95.8% |
| tests/fixtures/abc-dataset/00000151_a951550b8a1043a68423c541_step_001.step | 3161 | 52 | 78.8% | 89 | 100.0% (55.1% + 44.9%) | 4 | 75.0% | 75.0% |
| tests/fixtures/abc-dataset/00000152_a951550b8a1043a68423c541_step_002.step | 3003 | 69 | 100.0% | 210 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000153_a951550b8a1043a68423c541_step_003.step | 1945 | 45 | 100.0% | 135 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000154_a951550b8a1043a68423c541_step_004.step | 1317 | 34 | 100.0% | 80 | 100.0% (90.0% + 10.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000155_a951550b8a1043a68423c541_step_005.step | 35437 | 398 | 54.8% | 827 | 100.0% (60.8% + 39.2%) | 3 | 66.7% | 66.7% |
| tests/fixtures/abc-dataset/00000156_a951550b8a1043a68423c541_step_006.step | 35261 | 392 | 54.1% | 821 | 100.0% (60.5% + 39.5%) | 3 | 66.7% | 66.7% |
| tests/fixtures/abc-dataset/00000157_8208fd8b44b64dce91322f84_step_000.step | 812 | 21 | 100.0% | 53 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000158_559b9a459ca348529d9f4761_step_000.step | 30182 | 513 | 82.8% | 1042 | 100.0% (79.7% + 20.3%) | 11 | 90.9% | 90.9% |
| tests/fixtures/abc-dataset/00000159_48a3f2e6f67a4aea9d6d097b_step_000.step | 229 | 7 | 100.0% | 9 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000160_48a3f2e6f67a4aea9d6d097b_step_001.step | 15099 | 255 | 92.2% | 547 | 100.0% (86.8% + 13.2%) | 3 | 33.3% | 33.3% |
| tests/fixtures/abc-dataset/00000161_a583c6c3cdc143bab3f6c856_step_000.step | 812 | 21 | 100.0% | 53 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000162_4dcda9fab6dd46cf98fb194e_step_000.step | 434 | 11 | 100.0% | 27 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000163_ff9b2ed255484db78bb0e6da_step_000.step | 812 | 21 | 100.0% | 53 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000164_91adbf180e114b698160c9a4_step_000.step | 812 | 21 | 100.0% | 53 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000165_4070a99d6ebe45f5bd5d71eb_step_000.step | 812 | 21 | 100.0% | 53 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000166_2246cab58dbe4a2fb00fed50_step_000.step | 812 | 21 | 100.0% | 53 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000167_9c0acf3497254733a0db3edc_step_000.step | 3203 | 84 | 100.0% | 212 | 100.0% (100.0% + 0.0%) | 4 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000168_bfdc19149a8b4e72b3d0f5c5_step_000.step | 557 | 14 | 100.0% | 34 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000169_154ce2da51284e048f6177f9_step_000.step | 14324 | 288 | 100.0% | 660 | 100.0% (83.3% + 16.7%) | 7 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000170_b21aa794b6b64d87a57c245e_step_000.step | 841 | 25 | 100.0% | 33 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000171_b21aa794b6b64d87a57c245e_step_001.step | 267 | 9 | 100.0% | 8 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000172_b21aa794b6b64d87a57c245e_step_002.step | 1729 | 46 | 100.0% | 108 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000173_b21aa794b6b64d87a57c245e_step_003.step | 155 | 5 | 100.0% | 4 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000174_b21aa794b6b64d87a57c245e_step_004.step | 1471 | 37 | 100.0% | 100 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000175_b21aa794b6b64d87a57c245e_step_005.step | 141 | 4 | 100.0% | 4 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000176_b21aa794b6b64d87a57c245e_step_006.step | 141 | 4 | 100.0% | 4 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000177_b21aa794b6b64d87a57c245e_step_007.step | 309 | 8 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000178_b21aa794b6b64d87a57c245e_step_008.step | 267 | 7 | 100.0% | 10 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000179_b21aa794b6b64d87a57c245e_step_009.step | 1822 | 57 | 100.0% | 86 | 100.0% (96.5% + 3.5%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000180_b21aa794b6b64d87a57c245e_step_010.step | 1143 | 32 | 100.0% | 65 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000181_b21aa794b6b64d87a57c245e_step_011.step | 3442 | 60 | 50.0% | 174 | 100.0% (0.0% + 100.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000182_b21aa794b6b64d87a57c245e_step_012.step | 4247 | 86 | 93.0% | 212 | 100.0% (92.5% + 7.5%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000183_6c4172e8a17140f89d3b1c0a_step_000.step | 669 | 17 | 100.0% | 38 | 100.0% (94.7% + 5.3%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000184_6c4172e8a17140f89d3b1c0a_step_001.step | 10965 | 290 | 100.0% | 656 | 100.0% (100.0% + 0.0%) | 36 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000185_6c4172e8a17140f89d3b1c0a_step_002.step | 7155 | 204 | 100.0% | 408 | 100.0% (100.0% + 0.0%) | 34 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000186_6c4172e8a17140f89d3b1c0a_step_003.step | 4173 | 118 | 100.0% | 242 | 100.0% (100.0% + 0.0%) | 3 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000187_6c4172e8a17140f89d3b1c0a_step_004.step | 1313 | 38 | 100.0% | 72 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000188_6c4172e8a17140f89d3b1c0a_step_005.step | 379 | 13 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000189_d476135bde7244248d97aaa6_step_000.step | 248879 | 5896 | 100.0% | 17424 | 100.0% (100.0% + 0.0%) | 44 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000190_185ecbc783f34408aef52119_step_000.step | 225 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000191_222c0df7c7474deb967b5baa_step_000.step | 32659 | 717 | 88.4% | 1633 | 100.0% (95.5% + 4.5%) | 11 | 72.7% | 72.7% |
| tests/fixtures/abc-dataset/00000192_222c0df7c7474deb967b5baa_step_001.step | 32550 | 881 | 91.9% | 1277 | 100.0% (90.6% + 9.4%) | 21 | 38.1% | 38.1% |
| tests/fixtures/abc-dataset/00000193_222c0df7c7474deb967b5baa_step_002.step | 32714 | 875 | 91.9% | 1257 | 100.0% (91.1% + 8.9%) | 21 | 38.1% | 38.1% |
| tests/fixtures/abc-dataset/00000194_5513cf564670446690695cec_step_000.step | 248879 | 5896 | 100.0% | 17424 | 100.0% (100.0% + 0.0%) | 44 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000195_5513cf564670446690695cec_step_001.step | 4294 | 64 | 81.2% | 123 | 100.0% (87.8% + 12.2%) | 3 | 33.3% | 33.3% |
| tests/fixtures/abc-dataset/00000196_c9b71ecdd0654716891b0db8_step_000.step | 1773 | 49 | 95.9% | 112 | 100.0% (89.3% + 10.7%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000197_826494e894454504b0971fd1_step_000.step | 24798 | 226 | 59.3% | 443 | 100.0% (43.6% + 56.4%) | 4 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000198_826494e894454504b0971fd1_step_001.step | 21902 | 359 | 82.2% | 826 | 100.0% (35.4% + 64.6%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000199_826494e894454504b0971fd1_step_002.step | 11083 | 82 | 18.3% | 219 | 100.0% (30.6% + 69.4%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000200_826494e894454504b0971fd1_step_003.step | 6055 | 95 | 87.4% | 219 | 100.0% (90.9% + 9.1%) | 3 | 66.7% | 66.7% |
| tests/fixtures/abc-dataset/00000201_826494e894454504b0971fd1_step_004.step | 100722 | 1573 | 0.3% | 4352 | 100.0% (0.1% + 99.9%) | 12 | 8.3% | 8.3% |
| tests/fixtures/abc-dataset/00000202_89aafdfc86b04301be0478c6_step_000.step | 32659 | 717 | 88.4% | 1633 | 100.0% (95.5% + 4.5%) | 11 | 72.7% | 72.7% |
| tests/fixtures/abc-dataset/00000203_89aafdfc86b04301be0478c6_step_001.step | 32550 | 881 | 91.9% | 1277 | 100.0% (90.6% + 9.4%) | 21 | 38.1% | 38.1% |
| tests/fixtures/abc-dataset/00000204_89aafdfc86b04301be0478c6_step_002.step | 32714 | 875 | 91.9% | 1257 | 100.0% (91.1% + 8.9%) | 21 | 38.1% | 38.1% |
| tests/fixtures/abc-dataset/00000205_4a076a2799764163819ad3bd_step_000.step | 107289 | 253 | 17.4% | 651 | 100.0% (29.3% + 70.7%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000206_27417003e762409185cb2b69_step_000.step | 2043 | 51 | 84.3% | 104 | 100.0% (92.3% + 7.7%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000207_6edad88f1b7749c498b67f86_step_000.step | 107289 | 253 | 17.4% | 651 | 100.0% (29.3% + 70.7%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000208_33bd159d563f438fbbebd9fa_step_000.step | 32659 | 717 | 88.4% | 1633 | 100.0% (95.5% + 4.5%) | 11 | 72.7% | 72.7% |
| tests/fixtures/abc-dataset/00000209_33bd159d563f438fbbebd9fa_step_001.step | 32550 | 881 | 91.9% | 1277 | 100.0% (90.6% + 9.4%) | 21 | 38.1% | 38.1% |
| tests/fixtures/abc-dataset/00000210_33bd159d563f438fbbebd9fa_step_002.step | 32714 | 875 | 91.9% | 1257 | 100.0% (91.1% + 8.9%) | 21 | 38.1% | 38.1% |
| tests/fixtures/abc-dataset/00000211_e3ea4a27cfc74c25abff290c_step_000.step | 4071 | 101 | 100.0% | 171 | 100.0% (90.1% + 9.9%) | 12 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000212_fcaeaf04911442ccabeb275d_step_000.step | 1343 | 34 | 100.0% | 82 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000213_5324c79b172f41e8a49afc6f_step_000.step | 8255 | 203 | 98.0% | 560 | 100.0% (98.6% + 1.4%) | 6 | 66.7% | 66.7% |
| tests/fixtures/abc-dataset/00000214_5324c79b172f41e8a49afc6f_step_001.step | 40843 | 818 | 66.6% | 1975 | 100.0% (69.8% + 30.2%) | 7 | 71.4% | 71.4% |
| tests/fixtures/abc-dataset/00000215_5324c79b172f41e8a49afc6f_step_002.step | 1933 | 54 | 100.0% | 112 | 100.0% (100.0% + 0.0%) | 4 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000216_5324c79b172f41e8a49afc6f_step_003.step | 53381 | 1549 | 100.0% | 3442 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000217_5324c79b172f41e8a49afc6f_step_004.step | 268808 | 4381 | 0.0% | 14815 | 100.0% (0.0% + 100.0%) | 15 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000218_5324c79b172f41e8a49afc6f_step_005.step | 16917 | 346 | 15.9% | 963 | 100.0% (0.0% + 100.0%) | 2 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000219_5324c79b172f41e8a49afc6f_step_006.step | 18661 | 329 | 63.5% | 797 | 100.0% (70.1% + 29.9%) | 14 | 57.1% | 57.1% |
| tests/fixtures/abc-dataset/00000220_5324c79b172f41e8a49afc6f_step_007.step | 16996 | 406 | 92.1% | 1059 | 100.0% (94.0% + 6.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000221_5324c79b172f41e8a49afc6f_step_008.step | 16996 | 406 | 92.1% | 1059 | 100.0% (94.0% + 6.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000222_97b354a907114b7183faa9c4_step_000.step | 7753 | 160 | 95.0% | 303 | 100.0% (90.1% + 9.9%) | 8 | 87.5% | 87.5% |
| tests/fixtures/abc-dataset/00000223_97b354a907114b7183faa9c4_step_001.step | 14436 | 257 | 86.0% | 577 | 100.0% (84.4% + 15.6%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000224_97b354a907114b7183faa9c4_step_002.step | 1712 | 52 | 100.0% | 87 | 100.0% (97.7% + 2.3%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000225_9a5e4860c0044dd9825667d2_step_000.step | 119147 | 254 | 17.3% | 657 | 100.0% (29.2% + 70.8%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000226_000ace8ff6634150be81fe86_step_000.step | 32659 | 717 | 88.4% | 1633 | 100.0% (95.5% + 4.5%) | 11 | 72.7% | 72.7% |
| tests/fixtures/abc-dataset/00000227_000ace8ff6634150be81fe86_step_001.step | 32486 | 878 | 91.9% | 1274 | 100.0% (90.6% + 9.4%) | 21 | 38.1% | 38.1% |
| tests/fixtures/abc-dataset/00000228_000ace8ff6634150be81fe86_step_002.step | 32714 | 875 | 91.9% | 1257 | 100.0% (91.1% + 8.9%) | 21 | 38.1% | 38.1% |
| tests/fixtures/abc-dataset/00000229_682d529d5e8c4387bc59b3a1_step_000.step | 24978 | 574 | 83.4% | 1499 | 100.0% (82.9% + 17.1%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000230_a01a91841c8f48a5bf75924b_step_000.step | 2098 | 10 | 0.0% | 24 | 100.0% (25.0% + 75.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000231_f2f1249e743349808fca42a3_step_000.step | 6404 | 74 | 79.7% | 171 | 100.0% (80.1% + 19.9%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000232_5e5904b362dd41179fc77b86_step_000.step | 9833 | 266 | 99.2% | 527 | 100.0% (98.5% + 1.5%) | 15 | 93.3% | 93.3% |
| tests/fixtures/abc-dataset/00000233_9d5b83dd0a7d477f86f04710_step_000.step | 5265 | 150 | 100.0% | 300 | 100.0% (100.0% + 0.0%) | 25 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000234_38a16b120d49465b86d5bd82_step_000.step | 127105 | 1391 | 63.8% | 3360 | 100.0% (71.8% + 28.2%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000235_0fb1ef6e79de4ba78e052c4b_step_000.step | 12123 | 330 | 100.0% | 716 | 100.0% (100.0% + 0.0%) | 29 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000236_0fb1ef6e79de4ba78e052c4b_step_001.step | 154930 | 3015 | 89.3% | 5181 | 100.0% (72.3% + 27.7%) | 5 | 80.0% | 80.0% |
| tests/fixtures/abc-dataset/00000237_4829bc0645d6414d9b5cec31_step_000.step | 830 | 24 | 100.0% | 49 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000238_4829bc0645d6414d9b5cec31_step_001.step | 1369 | 38 | 100.0% | 84 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000239_4829bc0645d6414d9b5cec31_step_002.step | 1842 | 38 | 92.1% | 73 | 100.0% (79.5% + 20.5%) | 4 | 50.0% | 50.0% |
| tests/fixtures/abc-dataset/00000240_4829bc0645d6414d9b5cec31_step_003.step | 2065 | 53 | 100.0% | 134 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000241_4829bc0645d6414d9b5cec31_step_004.step | 211 | 7 | 100.0% | 6 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000242_4829bc0645d6414d9b5cec31_step_005.step | 2477 | 64 | 100.0% | 152 | 100.0% (100.0% + 0.0%) | 3 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000243_4829bc0645d6414d9b5cec31_step_006.step | 625 | 17 | 100.0% | 34 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000244_4829bc0645d6414d9b5cec31_step_007.step | 13822 | 294 | 95.9% | 736 | 100.0% (89.8% + 10.2%) | 5 | 40.0% | 40.0% |
| tests/fixtures/abc-dataset/00000245_c78d3dcf76d64ec9b46e7cef_step_000.step | 142909 | 3124 | 84.4% | 6500 | 100.0% (100.0% + 0.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000246_c78d3dcf76d64ec9b46e7cef_step_001.step | 309 | 9 | 100.0% | 14 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000247_c78d3dcf76d64ec9b46e7cef_step_002.step | 20334 | 456 | 16.0% | 1302 | 100.0% (41.2% + 58.8%) | 11 | 9.1% | 9.1% |
| tests/fixtures/abc-dataset/00000248_e87554b09cdc48c3add39db9_step_000.step | 741 | 14 | 28.6% | 17 | 100.0% (76.5% + 23.5%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000249_07a2da801a60492fb1e5eeb3_step_000.step | 985 | 24 | 100.0% | 64 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000250_641f03cbe6a145508c6ae055_step_000.step | 852 | 21 | 100.0% | 53 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000251_66fe5f0992fd4065b4838668_step_000.step | 99 | 3 | 100.0% | 2 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000252_a367cf6202d34e9ca31daf52_step_000.step | 11739 | 47 | 0.0% | 354 | 100.0% (0.0% + 100.0%) | 5 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000253_e6f00e94f0d94212b8a856bc_step_000.step | 27125 | 714 | 98.9% | 1318 | 100.0% (95.4% + 4.6%) | 31 | 93.5% | 93.5% |
| tests/fixtures/abc-dataset/00000254_10b0a59b1c58452a9ca3b329_step_000.step | 32659 | 717 | 88.4% | 1633 | 100.0% (95.5% + 4.5%) | 11 | 72.7% | 72.7% |
| tests/fixtures/abc-dataset/00000255_845fb06d7f2f4802a806a600_step_000.step | 28889 | 744 | 99.7% | 1811 | 100.0% (98.0% + 2.0%) | 10 | 90.0% | 90.0% |
| tests/fixtures/abc-dataset/00000256_845fb06d7f2f4802a806a600_step_001.step | 5742 | 159 | 100.0% | 324 | 100.0% (99.4% + 0.6%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000257_845fb06d7f2f4802a806a600_step_002.step | 141 | 4 | 100.0% | 4 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000258_845fb06d7f2f4802a806a600_step_003.step | 421 | 14 | 100.0% | 14 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000259_845fb06d7f2f4802a806a600_step_004.step | 1197 | 33 | 100.0% | 67 | 100.0% (73.1% + 26.9%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000260_845fb06d7f2f4802a806a600_step_005.step | 1161 | 32 | 100.0% | 66 | 100.0% (75.8% + 24.2%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000261_845fb06d7f2f4802a806a600_step_006.step | 1161 | 32 | 100.0% | 66 | 100.0% (75.8% + 24.2%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000262_845fb06d7f2f4802a806a600_step_007.step | 365 | 12 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000263_845fb06d7f2f4802a806a600_step_008.step | 589 | 16 | 100.0% | 26 | 100.0% (69.2% + 30.8%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000264_845fb06d7f2f4802a806a600_step_009.step | 10321 | 260 | 100.0% | 692 | 100.0% (99.4% + 0.6%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000265_845fb06d7f2f4802a806a600_step_010.step | 589 | 20 | 100.0% | 20 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000266_845fb06d7f2f4802a806a600_step_011.step | 1045 | 26 | 100.0% | 66 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000267_845fb06d7f2f4802a806a600_step_012.step | 365 | 12 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000268_845fb06d7f2f4802a806a600_step_013.step | 1005 | 32 | 100.0% | 47 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000269_845fb06d7f2f4802a806a600_step_014.step | 2065 | 60 | 100.0% | 123 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000270_845fb06d7f2f4802a806a600_step_015.step | 1309 | 40 | 100.0% | 62 | 100.0% (61.3% + 38.7%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000271_845fb06d7f2f4802a806a600_step_016.step | 1421 | 44 | 100.0% | 66 | 100.0% (63.6% + 36.4%) | 2 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000272_648a7c23a21f4e88bf25ffeb_step_000.step | 197 | 6 | 100.0% | 6 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000273_648a7c23a21f4e88bf25ffeb_step_001.step | 553 | 14 | 100.0% | 16 | 100.0% (87.5% + 12.5%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000274_648a7c23a21f4e88bf25ffeb_step_002.step | 897 | 22 | 100.0% | 56 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000275_648a7c23a21f4e88bf25ffeb_step_003.step | 757 | 22 | 100.0% | 30 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000276_648a7c23a21f4e88bf25ffeb_step_004.step | 929 | 24 | 100.0% | 57 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000277_c740d53bc77c4e1b871b89cf_step_000.step | 7145 | 157 | 91.7% | 379 | 100.0% (91.8% + 8.2%) | 2 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000278_8f19db5c98a746c49d8075f3_step_000.step | 28105 | 506 | 80.2% | 1063 | 100.0% (92.9% + 7.1%) | 5 | 60.0% | 60.0% |
| tests/fixtures/abc-dataset/00000279_5a6ef3285b89452ea6b390e7_step_000.step | 70664 | 719 | 70.1% | 1575 | 100.0% (69.7% + 30.3%) | 17 | 76.5% | 76.5% |
| tests/fixtures/abc-dataset/00000280_5a6ef3285b89452ea6b390e7_step_001.step | 1263 | 34 | 82.4% | 68 | 100.0% (100.0% + 0.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000281_5a6ef3285b89452ea6b390e7_step_002.step | 21363 | 404 | 91.6% | 764 | 100.0% (85.9% + 14.1%) | 20 | 85.0% | 85.0% |
| tests/fixtures/abc-dataset/00000282_5a6ef3285b89452ea6b390e7_step_003.step | 1263 | 34 | 82.4% | 68 | 100.0% (100.0% + 0.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000283_5a6ef3285b89452ea6b390e7_step_004.step | 268 | 7 | 100.0% | 15 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000284_5a6ef3285b89452ea6b390e7_step_005.step | 665 | 18 | 100.0% | 40 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000285_d215b7bc60ae468288e5f1ea_step_000.step | 1147 | 25 | 100.0% | 82 | 100.0% (97.6% + 2.4%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000286_09d7ee9962c8427aac9ba7cb_step_000.step | 36845 | 444 | 52.9% | 1424 | 100.0% (41.9% + 58.1%) | 3 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000287_58f8ff46b7564e989f9695b7_step_000.step | 685 | 18 | 100.0% | 38 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000288_58f8ff46b7564e989f9695b7_step_001.step | 6053 | 166 | 98.8% | 366 | 100.0% (96.7% + 3.3%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000289_58f8ff46b7564e989f9695b7_step_002.step | 901 | 28 | 100.0% | 44 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000290_58f8ff46b7564e989f9695b7_step_003.step | 398 | 6 | 66.7% | 6 | 100.0% (33.3% + 66.7%) | 1 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000291_58f8ff46b7564e989f9695b7_step_004.step | 612 | 17 | 100.0% | 34 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000292_58f8ff46b7564e989f9695b7_step_005.step | 313 | 8 | 100.0% | 18 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000293_58f8ff46b7564e989f9695b7_step_006.step | 253 | 8 | 100.0% | 8 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000294_c4af21184d19433ea87f09b8_step_000.step | 32659 | 717 | 88.4% | 1633 | 100.0% (95.5% + 4.5%) | 11 | 72.7% | 72.7% |
| tests/fixtures/abc-dataset/00000295_52a153d2417747b5926ff9df_step_000.step | 553 | 14 | 100.0% | 36 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000296_52a153d2417747b5926ff9df_step_001.step | 3459 | 93 | 100.0% | 216 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000297_52a153d2417747b5926ff9df_step_002.step | 14226 | 306 | 93.8% | 815 | 100.0% (71.3% + 28.7%) | 2 | 0.0% | 0.0% |
| tests/fixtures/abc-dataset/00000298_52a153d2417747b5926ff9df_step_003.step | 225 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/abc-dataset/00000299_52a153d2417747b5926ff9df_step_004.step | 27155 | 674 | 100.0% | 1900 | 100.0% (100.0% + 0.0%) | 5 | 100.0% | 100.0% |
| tests/fixtures/malformed/header-only.step | 0 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/nist-pmi/AP203 geometry only/nist_ctc_01_asme1_rd.stp | 5325 | 139 | 100.0% | 370 | 100.0% (98.9% + 1.1%) | 1 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/AP203 geometry only/nist_ctc_02_asme1_rc.stp | 20822 | 664 | 94.9% | 1498 | 100.0% (91.2% + 8.8%) | 3 | 33.3% | 33.3% |
| tests/fixtures/nist-pmi/AP203 geometry only/nist_ctc_03_asme1_rc.stp | 5560 | 139 | 100.0% | 390 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/AP203 geometry only/nist_ctc_04_asme1_rd.stp | 14668 | 520 | 100.0% | 1214 | 100.0% (98.9% + 1.1%) | 2 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/AP203 geometry only/nist_ctc_05_asme1_rd.stp | 6681 | 209 | 95.7% | 514 | 100.0% (91.6% + 8.4%) | 3 | 33.3% | 33.3% |
| tests/fixtures/nist-pmi/AP203 geometry only/nist_ftc_06_asme1_rd.stp | 4992 | 144 | 100.0% | 310 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/AP203 geometry only/nist_ftc_07_asme1_rd.stp | 7792 | 269 | 92.6% | 583 | 100.0% (100.0% + 0.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/nist-pmi/AP203 geometry only/nist_ftc_08_asme1_rc.stp | 7990 | 270 | 100.0% | 705 | 100.0% (99.7% + 0.3%) | 1 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/AP203 geometry only/nist_ftc_09_asme1_rd.stp | 5848 | 158 | 100.0% | 454 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/AP203 geometry only/nist_ftc_10_asme1_rb.stp | 6433 | 214 | 99.5% | 439 | 100.0% (96.4% + 3.6%) | 1 | 0.0% | 0.0% |
| tests/fixtures/nist-pmi/AP203 geometry only/nist_ftc_11_asme1_rb.stp | 164 | 6 | 100.0% | 6 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/AP203 with PMI/nist_ctc_01_asme1_ap203.stp | 17160 | 139 | 100.0% | 370 | 100.0% (98.9% + 1.1%) | 2 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/AP203 with PMI/nist_ctc_02_asme1_ap203.stp | 46100 | 487 | 94.3% | 942 | 100.0% (97.9% + 2.1%) | 1 | 0.0% | 0.0% |
| tests/fixtures/nist-pmi/AP203 with PMI/nist_ctc_03_asme1_ap203.stp | 25922 | 139 | 100.0% | 390 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/AP203 with PMI/nist_ctc_04_asme1_ap203.stp | 22759 | 406 | 100.0% | 831 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/AP203 with PMI/nist_ctc_05_asme1_ap203.stp | 15520 | 156 | 97.4% | 331 | 100.0% (92.1% + 7.9%) | 2 | 0.0% | 0.0% |
| tests/fixtures/nist-pmi/nist_ctc_01_asme1_ap242-e1.stp | 4350 | 117 | 100.0% | 318 | 100.0% (95.6% + 4.4%) | 1 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/nist_ctc_02_asme1_ap242-e2.stp | 22304 | 637 | 94.7% | 1416 | 100.0% (88.7% + 11.3%) | 7 | 71.4% | 71.4% |
| tests/fixtures/nist-pmi/nist_ctc_03_asme1_ap242-e2.stp | 5920 | 120 | 100.0% | 354 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/nist_ctc_04_asme1_ap242-e1.stp | 20456 | 484 | 100.0% | 1110 | 100.0% (79.6% + 20.4%) | 3 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/nist_ctc_05_asme1_ap242-e1.stp | 13394 | 156 | 97.4% | 331 | 100.0% (92.1% + 7.9%) | 1 | 0.0% | 0.0% |
| tests/fixtures/nist-pmi/nist_ftc_06_asme1_ap242-e2.stp | 10034 | 187 | 100.0% | 476 | 100.0% (95.2% + 4.8%) | 3 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/nist_ftc_07_asme1_ap242-e2.stp | 17010 | 306 | 93.5% | 871 | 100.0% (56.6% + 43.4%) | 3 | 33.3% | 33.3% |
| tests/fixtures/nist-pmi/nist_ftc_08_asme1_ap242-e1-tg.stp | 12630 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/nist-pmi/nist_ftc_08_asme1_ap242-e2.stp | 10072 | 247 | 100.0% | 616 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/nist_ftc_09_asme1_ap242-e1.stp | 11662 | 163 | 100.0% | 474 | 100.0% (98.3% + 1.7%) | 2 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/nist_ftc_10_asme1_ap242-e2.stp | 21230 | 282 | 99.6% | 957 | 100.0% (55.6% + 44.4%) | 3 | 33.3% | 33.3% |
| tests/fixtures/nist-pmi/nist_ftc_11_asme1_ap242-e2.stp | 2032 | 42 | 100.0% | 104 | 100.0% (90.4% + 9.6%) | 1 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/nist_stc_06_asme1_ap242-e3.stp | 11780 | 144 | 100.0% | 310 | 100.0% (100.0% + 0.0%) | 5 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/nist_stc_07_asme1_ap242-e3.stp | 17470 | 306 | 93.5% | 871 | 100.0% (78.3% + 21.7%) | 6 | 16.7% | 16.7% |
| tests/fixtures/nist-pmi/nist_stc_08_asme1_ap242-e3.stp | 11548 | 271 | 100.0% | 709 | 100.0% (100.0% + 0.0%) | 5 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/nist_stc_09_asme1_ap242-e3.stp | 10393 | 125 | 100.0% | 392 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/nist-pmi/nist_stc_10_asme1_ap242-e2.stp | 12073 | 256 | 99.6% | 551 | 100.0% (96.9% + 3.1%) | 1 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/as1-oc-214/as1-oc-214.stp | 6425 | 53 | 47.2% | 126 | 100.0% (33.3% + 66.7%) | 5 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/as1-oc-214.stp | 6425 | 53 | 47.2% | 126 | 100.0% (33.3% + 66.7%) | 5 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/as1-tu-203.stp | 2362 | 53 | 47.2% | 126 | 100.0% (55.6% + 44.4%) | 10 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/as1_pe_203.stp | 2881 | 53 | 100.0% | 126 | 100.0% (100.0% + 0.0%) | 10 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/dm1-id-214.stp | 1189 | 24 | 0.0% | 51 | 39.2% (0.0% + 39.2%) | 3 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/dm1-oc-214.stp | 3399 | 31 | 54.8% | 72 | 100.0% (41.7% + 58.3%) | 3 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/dm1-pe-214.stp | 1633 | 31 | 100.0% | 72 | 100.0% (100.0% + 0.0%) | 6 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/io1-cm-214.stp | 917 | 29 | 100.0% | 70 | 100.0% (100.0% + 0.0%) | 3 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/io1-tu-203.stp | 1363 | 29 | 17.2% | 70 | 100.0% (100.0% + 0.0%) | 2 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/FOOT.stp | 105 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/FOOT_BACK_000.stp | 436 | 13 | 100.0% | 26 | 100.0% (84.6% + 15.4%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/FOOT_FRONT_000.stp | 436 | 13 | 100.0% | 26 | 100.0% (84.6% + 15.4%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/HEAD.stp | 105 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/HEAD_BACK.stp | 595 | 12 | 100.0% | 20 | 100.0% (50.0% + 50.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/HEAD_FRONT.stp | 214 | 5 | 100.0% | 6 | 100.0% (33.3% + 66.7%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/MAINBODY.stp | 105 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/MAINBODY_BACK.stp | 1487 | 31 | 80.6% | 62 | 100.0% (53.2% + 46.8%) | 1 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/MAINBODY_FRONT.stp | 1126 | 22 | 72.7% | 44 | 100.0% (50.0% + 50.0%) | 1 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/TAIL.stp | 118 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/TAIL_MIDDLE_PART.stp | 703 | 20 | 100.0% | 56 | 100.0% (96.4% + 3.6%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/TAIL_TURBINE.stp | 704 | 20 | 100.0% | 56 | 100.0% (89.3% + 10.7%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/s1-c5-214/s1-c5-214.stp | 198 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/occt-import-js/cax-if/s1-tu-203/footback.stp | 431 | 9 | 11.1% | 16 | 100.0% (100.0% + 0.0%) | 2 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1-tu-203/footfront.stp | 565 | 13 | 38.5% | 28 | 100.0% (100.0% + 0.0%) | 2 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1-tu-203/headback.stp | 573 | 12 | 16.7% | 22 | 100.0% (81.8% + 18.2%) | 2 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1-tu-203/headfront.stp | 291 | 5 | 20.0% | 8 | 100.0% (100.0% + 0.0%) | 2 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1-tu-203/mainbodyback.stp | 783 | 20 | 30.0% | 38 | 100.0% (100.0% + 0.0%) | 2 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1-tu-203/mainbodyfront.stp | 1351 | 37 | 45.9% | 80 | 100.0% (100.0% + 0.0%) | 2 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1-tu-203/s1-tu-203.stp | 775 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/occt-import-js/cax-if/s1-tu-203/tailmiddlepart.stp | 679 | 20 | 90.0% | 50 | 100.0% (100.0% + 0.0%) | 2 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1-tu-203/tailturbine.stp | 1047 | 21 | 81.0% | 56 | 100.0% (94.6% + 5.4%) | 2 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/foot_asm.stp | 206 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/foot_back_prt.stp | 366 | 9 | 100.0% | 16 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/foot_front_prt.stp | 544 | 13 | 100.0% | 28 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/head_asm.stp | 206 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/head_back_prt.stp | 466 | 12 | 100.0% | 22 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/head_front_prt.stp | 266 | 5 | 100.0% | 8 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/main_body_back_prt.stp | 854 | 20 | 80.0% | 38 | 100.0% (100.0% + 0.0%) | 2 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/main_body_front_prt.stp | 1114 | 29 | 86.2% | 56 | 100.0% (100.0% + 0.0%) | 2 | 0.0% | 0.0% |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/mainbody_asm.stp | 206 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/s1_pe_214.stp | 358 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/tail_asm.stp | 239 | 0 | – | 0 | – (– + –) | 0 | – | – |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/tail_middle_part_prt.stp | 930 | 23 | 100.0% | 56 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/s1_pe_214/tail_turbine_prt.stp | 934 | 20 | 100.0% | 54 | 100.0% (96.3% + 3.7%) | 2 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cax-if/sg1-c5-214.stp | 460 | 16 | 100.0% | 32 | 100.0% (100.0% + 0.0%) | 2 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/conical-surface/conical-surface.step | 173 | 5 | 100.0% | 8 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cube-10x10mm/Cube 10x10.stp | 202 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cube-fcstd/cube.step | 386 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cube-fcstd/cube2.step | 410 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cube-fcstd/cube3.step | 377 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cube-units/cube-in.step | 391 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cube-units/cube-m.step | 385 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/cube-units/cube-mm.step | 385 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/rounded-cube/rounded-cube.step | 450 | 7 | 100.0% | 15 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/occt-import-js/simple-basic-cube/cube.stp | 185 | 6 | 100.0% | 12 | 100.0% (100.0% + 0.0%) | 1 | 100.0% | 100.0% |
| tests/fixtures/stress-assembly/nist_ctc_02_asme1_rc-x60.step | 3283040 | 39840 | 94.9% | 89880 | 100.0% (91.2% + 8.8%) | 120 | 50.0% | 50.0% |
| Ultimaker 3 (`9066.STEP`) | 1517999 | 34840 | 89.9% | 96561 | 100.0% (83.4% + 16.6%) | 199 | 55.3% | 55.3% |
