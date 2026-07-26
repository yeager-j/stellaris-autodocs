# DDS asset evaluation

Status: Complete against Stellaris `Pegasus v4.4.6` and `image_dds` `0.7.2`. `image_dds` is adopted with encoding disabled, the conversion recipe is pinned, and the independent second reading stays in the harness.

The hypothesis held. `image_dds` decodes every format the installed corpus contains, and the two DXT5 icons the earlier feasibility note converted were converted correctly. What the note could not establish is a contract, and the gap between the two turned out to be the whole spike.

Two icons is not a claim about 33,145 files, and the two that were opened were the rarest class among the technology icons they were meant to represent: 114 of 2,621 technology-path files are DXT5, against 1,257 uncompressed 32-bit and 880 uncompressed 24-bit. Worse, the failure mode this corpus actually has is silent. A decoder that reads the blue channel where red belongs returns a plausible image, and eleven files in the corpus are the only inputs capable of telling the two apart. No amount of looking at icons finds that.

So the method is the parser spike's rather than the feasibility note's: two independent readings of the same bytes, where every disagreement is either a defect or a finding. It found one defect in the second reading that no fixture would have contained, and one behaviour in `image_dds`'s convenience entry point that returns success on an image six times too tall.

## Decision

Adopt `image_dds` `0.7.2` with `default-features = false, features = ["ddsfile"]`, consumed through `Surface::decode_layers_mipmaps_rgba8` and never through `image_from_dds`. Output PNG. The conversion recipe is a versioned value whose fields are the choices that were measured to change the output, and the asset key derives from the resolved decoder and encoder versions rather than from a hand-maintained integer.

Two shapes are refused rather than decoded: multi-layer surfaces and premultiplied-alpha four-character codes. Both refusals cost nothing in the reachable set and both alternatives are a quietly wrong image.

## Reproducible record

```bash
cargo test --manifest-path tools/dds-spike/Cargo.toml
cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin generate
cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin census -- --capture
cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin decode -- --capture
cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin recipe -- --capture
cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin failures -- --capture
cargo run --release --manifest-path tools/dds-spike/Cargo.toml --bin verify
```

`verify` recomputes every corpus tree digest, re-hashes every artifact, and compares the recorded `image_dds`, `bcdec_rs`, `texture2ddecoder`, `png`, `image-webp`, rustc, and Stellaris versions against the current machine, printing `ok` or `DRIFT` per record and exiting non-zero on any drift — the same contract as `tools/oracle/verify.py`.

It was shown red before being trusted, twice: with `STELLARIS_WORKSHOP_ROOT` pointed elsewhere, and with one byte of `fixtures/assets/dds/valid/bgra8_2x2.dds` altered. The second demonstration found a real gap rather than confirming a working gate — `d4-failures` reported `ok` while running against the altered fixture, because its manifest recorded the game corpora but not the fixture corpus it also consumes. The record now names it, and the same demonstration now fails it.

Corpus roots are environment-overridable exactly as the oracle harness's are. No corpus content is committed: records hold tree digests, file counts, and byte totals, which is what a licensed local installation needs to reproduce a run. The fixtures are committed, are original work, and carry per-file digests in every record — for a binary fixture that is the only integrity check a reader can apply without running the generator.

| Pinned | Value |
| --- | --- |
| Stellaris | `Pegasus v4.4.6 (fdde)`, `v4.4.6`, mods-compat `4.4`, Steam |
| `image_dds` | `0.7.2`, default features off, `ddsfile` only |
| `bcdec_rs` | `0.2.0` — transitive, and what actually decodes the compressed classes |
| `texture2ddecoder` | `0.1.2` |
| `png` | `0.18.1` |
| `image-webp` | `0.2.4` |
| Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)`, macOS, aarch64 |

## Method

Two readings were built over one application-owned model, and neither is allowed to know about the other.

- **Path A — `image_dds`.** `Surface::decode_layers_mipmaps_rgba8(0..1, 0..1)` over the byte range the container reader already proved present.
- **Path B — the independent reading.** Uncompressed layouts are reinterpreted straight from the `DDS_PIXELFORMAT` masks: shift from `trailing_zeros`, width from the mask's run of ones, expansion by bit replication. No format table is consulted at all. BC1, BC2, and BC3 are decoded from the S3TC specification.

The asymmetry between them is deliberate. Path A asks *which of the formats I know does this bit pattern equal*; path B asks *what do these four masks say*. A misreading of a mask survives path A's lookup unchanged and cannot survive path B.

The container reader is a third piece of the same discipline. It parses the DDS header from the specification rather than through `ddsfile`, even though `image_dds` already depends on `ddsfile`. If classification used the decoder's own parser, "malformed" and "unsupported" would both reduce to *the decoder said no*, and `analysis::finalize` could not scope its Analysis Issue.

### Controls

- **Cross-check.** The divergence count and its shape is the measurement. Every file both paths accept is compared byte for byte at mip 0.
- **Spec-derived expectations.** Two decoders agreeing shows they are not independently wrong. Fixture endpoints are chosen so the BC interpolants are exact integers, and the expected pixels are computed from the format specification, which is what shows they are not *jointly* wrong.
- **Negative controls, shown red before their green result was used.** Two faults injected into path B, each asserting the shape of the failure rather than that it is non-zero; two architecture-gate inversions; two drift-gate demonstrations. All six are enumerated in [Findings](#findings) and in the run docs.
- **Denominators.** Every coverage claim is reported beside the census, and beside a second denominator — the reachable set, being the distinct texture paths a sprite definition actually names. 33,145 files is the wrong measure for a tool that converts the icons its content references.

### Why two readings rather than one

A single decoder would have had nothing to be wrong against. The game does not report what a correct decode is, and 33,145 files cannot be inspected. The corpus discriminated where fixtures could not: path B's original BC3 implementation was wrong, on 22 real files, in a way no fixture would have been written to contain.

## Corpus

| Corpus | Files | Uncompressed 32-bit | 24-bit | 16-bit | DXT1 | DXT3 | DXT5 | Not DDS |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Vanilla `Pegasus v4.4.6` | 22,469 | 14,369 | 634 | 15 | 659 | 156 | 6,634 | 2 |
| Installed Workshop mods | 10,649 | 6,477 | 851 | 0 | 398 | 1,006 | 1,917 | 0 |
| `fixtures/assets/dds` | 27 | — | — | — | — | — | — | 3 |
| **Total** | **33,145** | | | | | | | |

Selection is every `.dds` under each root, with no directory allowlist. Unlike the parser spike's `.txt`, this extension is unambiguous — and enumerating everything is the point, because the two vanilla files that carry the extension without the format are a result.

Shapes the earlier artifact matrix did not list: 844 block-compressed surfaces whose dimensions are not multiples of four, down to 1x1; 520 24-bit surfaces whose rows are not four-byte aligned; 12 cube maps; mip chains up to 13 levels; one DX10-header file; and 15 files in a 16-bit A1R5G5B5 layout.

The reachable set is smaller and differently shaped: 7,241 distinct texture paths named by vanilla sprite definitions, of which 7,194 resolve, and 3,911 named by workshop mods, of which 3,905 resolve.

Sprite definitions under `pdx_launcher/`, `pdx_online_assets/`, `previewer_assets/`, and `tweakergui_assets/` are excluded. They describe the Paradox launcher and internal developer tools; including them adds 83 references and 22 dangling paths that say nothing about game content.

## Evidence matrix

| # | Requirement | Verdict | Record |
| --- | --- | --- | --- |
| 1 | Decode every format the corpus contains | **Met.** 33,104 of 33,145 inputs decode. The 41 that do not are 2 non-DDS files, 12 cube maps refused by policy, and 27 fixtures deliberately built to fail | `d1`, `d2` |
| 2 | Correctness, not merely success | **Met, and this was the work.** Zero divergences beyond rounding across 33,145 files; worst per-channel delta 1 | `d2` |
| 3 | The cross-check can actually fail | **Met.** Two injected faults, each caught on exactly the set of files it could reach: 20,567 of 20,567 and 9 of 9 | `d2` |
| 4 | One typed outcome per input, decided before decoding | **Met for three of four.** Totality holds on every corpus; `ConversionFailure` is unreachable from any real input and remains unexercised | `d4` |
| 5 | Malformed distinguishable from unsupported | **Met.** Decided by the container reader, the recipe policy, and the supported-format set — three lookups, no decoding | `d4` |
| 6 | Conversion recipe parameters pinned by measurement | **Met.** Four parameters measured; the colorspace declaration provably changes nothing, and is retained as a declaration rather than dropped | `d3` |
| 7 | The asset key is a function of source bytes and recipe | **Met.** Stable, and changes with output format, recipe version, and source bytes | `d3` |
| 8 | Output format chosen on evidence | **Met.** PNG and lossless WebP both round-trip byte-identical; WebP is 6.9% and 7.4% smaller | `d3` |
| 9 | License-clean committed fixtures, reproducible | **Met.** 27 fixtures, all original work, generated by committed source and checked byte for byte | `d4` |
| 10 | No decoder type crosses the application boundary | **Met.** Enforced by a test with two negative controls | — |
| 11 | Conversion is deterministic across processes | **Met.** A second process re-captures a byte-identical record, encoder-output digests included | `d3` |
| 12 | Reproducible from a licensed local installation | **Met.** Drift gate shown red twice; no proprietary content committed | all |

No requirement is unmet. Requirement 4 is partial and the limitation is disclosed rather than rounded up.

## Findings

### The formats the feasibility check validated were the rarest ones it could have picked

Of 2,621 files on a technology path, 1,257 are uncompressed 32-bit, 880 uncompressed 24-bit, 292 DXT3, 114 DXT5, and 78 DXT1. The earlier note converted two DXT5 icons and generalized from them to "the observed formats". DXT5 is 4.3% of that set.

This is not a criticism of the note, which claimed feasibility and established it. It is the reason feasibility and a contract are different artifacts: the classes carrying 82% of the technology icons had never been decoded when the design began depending on them.

### Eleven files are the only thing in the corpus that can catch a red-and-blue swap

Almost every uncompressed 32-bit surface in the corpus declares `A8R8G8B8` — 14,361 in vanilla and 6,473 in the workshop. Eleven declare `A8B8G8R8`, whose red and blue masks are exchanged:

```
gfx/interface/buttons/standard_button_200_24_dlc_overlay_animated.dds
gfx/interface/buttons/standard_button_200_24_dlc_overlay_mask.dds
gfx/interface/fleet_view/paladin_resolution_ability.dds
gfx/interface/fleet_view/paladin_class_ability.dds
gfx/interface/icons/ship_parts/ship_part_placeholder.dds
gfx/interface/icons/resolutions/resolution_placeholder.dds
gfx/map/storms/NebulaOpacity.dds
gfx/map/storms/infestation.dds
1623423360/gfx/ui_overhaul_qhd/research/tech_bg_unlocked.dds
3449760193/gfx/ui_overhaul_qhd/research/tech_bg_unlocked.dds
1121692237/gfx/interface/tiles/dark_area_70_percent.dds
```

A decoder keyed on bit count reads all 20,846 of them identically and gets eleven of them wrong. The result is a plausible image with its red and blue exchanged, which no visual inspection rejects and no structural check notices.

The `assume-bgra` injection measures exactly this. Reading every 32-bit surface as the majority layout diverges from the correct reading on 9 files — the eleven above, less two whose red and blue channels are already equal in every pixel — and the cross-check caught all 9. That is a very small target in a 33,145-file corpus, and hitting it is the reason the correctness claim means anything.

### The second decoder was wrong about BC3, and only the corpus could have said so

Path B originally decoded the compressed classes through `texture2ddecoder`, chosen because its lineage is unrelated to `image_dds`'s `bcdec_rs`. Over the corpus the two paths agreed on everything except 22 files, all BC3, all Gigastructures model textures, differing by up to 255 in a channel on roughly half their pixels.

`texture2ddecoder::decode_bc3_block` forwards its colour half to `decode_bc1_block`, which still selects BC1's three-colour punch-through mode when the endpoints are ordered `c0 <= c1`. The BC2 and BC3 specifications forbid that: those formats carry alpha separately, so their colour block always uses four-colour interpolation and the endpoint ordering has no second meaning. `bcdec_rs` implements the rule; `texture2ddecoder` does not.

So `image_dds` was right and the independent reading was wrong. BC1, BC2, and BC3 are now decoded from the specification directly, which is a stronger independent reading than a second library anyway — it is the statement both libraries are trying to implement. The 22 divergences went to zero.

The finding that matters is about method, not about either crate. No fixture would have contained a BC3 block with reversed endpoints, because the specification says such an ordering is meaningless; nobody writes a test for a case the standard defines away. The corpus contained 22 of them. `fixtures/assets/dds/valid/dxt5_reversed_endpoints_4x4.dds` now states the rule in isolation, written after the corpus found it.

### Two conformant BC decoders do not agree byte for byte

Across 33,145 files the largest per-channel disagreement between the two readings is 1. `bcdec_rs` folds the 5-bit-to-8-bit expansion into its interpolation with one rounding; the specification-derived reading expands first and interpolates second with another. Both are legitimate, and neither is a defect.

This is a constraint on what "reproduce the output hash" can mean. The asset test suite reproduces the hashes of *the pinned decoder*, not of BC decoding in general. Swapping decoders is a recipe-version change even when both are correct.

### The convenience entry point turns a cube map into a strip and reports success

`image_dds::image_from_dds` decodes every array layer and stacks them vertically. A 2x2 six-face cube map comes back as 2x12; `gfx/map/sky_core.dds`, at 2048x2048, would come back as 2048x12288. No error is returned, and nothing in the result suggests the caller should question it.

The recipe therefore selects one layer explicitly through `Surface::decode_layers_mipmaps_rgba8` and refuses anything with more, and the `image` feature is disabled so the convenience function is not reachable at all. All 12 cube maps in the pinned corpora live under `gfx/map/` and `gfx/worldgfx/`, and `d4` confirms none of them appears in the reachable set, so refusing them costs nothing.

Reading "33,133 of 33,145 decoded" as a result would have recorded 99.96% for a path that is silently wrong on 12 files, and would be silently wrong on every `X8R8G8B8` a foreign mod ships.

### A DDS carries no colorspace, and `image_dds` applies none either — so the recipe declares rather than converts

33,144 of the 33,145 inputs carry no colorspace flag; only the single DX10-header file does. The recipe therefore has to choose an interpretation, and the obvious worry is that the choice silently applies a transfer function to every icon.

Measured, it does not. Decoding the same bytes declared `UNORM` and declared `UNORM_SRGB` produced byte-identical RGBA8 on every surface where both declarations exist. `image_dds` dispatches both to the same 8-bit decode. The recipe's colorspace field is a label carried into the output — no ICC profile, no `gAMA` chunk, untagged PNG read as sRGB by convention — rather than a conversion applied to it.

The field is retained despite changing nothing today, because it is what a future resampling or compositing step would have to consult, and because the measurement now doubles as a regression detector for a future `image_dds` upgrade.

### Identical pixels, six different files

One decoded image, encoded eight ways through the same `png` crate at the same version, produced six distinct digests: 332, 143, 88, and 93 bytes across compression and filter settings, with the pixels never changing.

This is why encoder identity is in the asset key. A key derived from source bytes and pixel content alone would address two different files at once in a content-addressed store. And the version alone is not enough: `png`'s own documentation states that its DEFLATE implementation may evolve without a semver-breaking release, so the settings are named in the recipe too.

The same argument reaches one level deeper. `image_dds` decodes the compressed classes through `bcdec_rs`, so a bump there changes output pixels with no first-party change and no change to `image_dds`'s own version. The recipe records both, and `verify` compares both.

### Conversion is deterministic across processes, and a record that says otherwise is a record with a clock in it

The Asset Store addresses blobs by content, so the same source bytes under the same recipe must produce the same output bytes in a later process on a later day. Capturing `d3-recipe` twice in separate process invocations produces a byte-identical `recipe.json`, including all eight PNG encoder-output digests. The encoder writes no `tIME` or `tEXt` chunk, so nothing time-dependent enters the output.

Establishing that also found a defect in the record itself. The first re-capture differed — in two fields, both wall-clock encode times. A record the drift gate compares must be reproducible from unchanged inputs, and a timing figure makes every re-capture differ for a reason that has nothing to do with the evidence. The timings were removed; directional performance figures belong in a record of their own, as the parser spike keeps them.

### Block-unaligned compressed textures are the normal case, not the edge

844 block-compressed surfaces in the pinned corpora have dimensions that are not multiples of four — 262 in vanilla, 582 in the workshop — down to a 1x1 DXT5 texture and a 323x10 strip. `image_dds` declares a `NonIntegralDimensionsInBlocks` error and never raises it in this version; the decode rounds up to whole blocks and crops. Both readings agree on every one of them.

Separately, 520 24-bit surfaces have rows that are not four-byte aligned. `image_dds` ignores `dwPitchOrLinearSize` entirely and treats rows as tightly packed, which is what these files are. A decoder that trusted the pitch field, or that assumed four-byte row alignment, would read every row after the first at the wrong offset — and would do it on 520 shipped files while looking correct on the other 965.

### Vanilla ships two files that are not DDS, and one of them is reachable

`gfx/interface/icons/traits/all_negative.dds` is zero bytes. `gfx/interface/main/paused_bar_glow.dds` is three bytes holding a UTF-8 byte order mark. Both are in the installed corpus at `Pegasus v4.4.6 (fdde)`.

They are not equivalent. `interface/main.gfx` names `paused_bar_glow.dds` as the `animationtexturefile` of a sprite, so a malformed-media outcome is on a path the product can actually reach. Nothing in the scanned sprite definitions names `all_negative.dds`.

Malformed handling is therefore a live path rather than a defensive one, which is a different justification for the same code — and one that would have been invisible without scanning the reachable set.

### Forty-seven vanilla sprite references name a file that does not exist

Of 7,241 distinct texture paths named by vanilla sprite definitions, 47 resolve to nothing, including `gfx/interface/tech_view/tech_selection_bg.dds` in the technology view itself. A further 14 references are written with doubled separators and resolve after normalization.

`MissingBytes` is therefore not a hypothetical either. It is decided at the source boundary rather than in the asset adapter, which is why this adapter never produces it and why `d4` records the count separately from its own totals.

### All four typed outcomes are declared; three occur, and the fourth is unreachable

Totality holds on every corpus: 27 fixtures, 22,469 vanilla, and 10,649 workshop files each produced exactly one outcome, and the counts sum to the input counts.

`ConversionFailure` was produced by nothing. Every failure the corpus contains is a property of the input, caught by the container reader, the recipe policy, or the supported-format set before any decoding — which is the contract working as intended, and also means the variant is untested. Reaching it requires injecting a failure into the encoder or the staging write, which this run does not do. It is recorded as a warning in `d4-failures` rather than presented as coverage.

### WebP is 7% smaller, and PNG is still the choice

Over icon-sized surfaces both encoders round-trip byte-identical with zero failures, so both are genuinely lossless. On 11,560 vanilla icons PNG produces 56.7 MB against WebP's 52.8 MB; on 7,756 workshop icons, 43.8 MB against 40.6 MB. WebP wins by 6.9% and 7.4%.

PNG is the recommendation anyway. The saving is small in absolute terms — a revision materializes the icons its content references, not the whole corpus — and `output_format` is already a component of the asset key, so adopting WebP later is additive rather than a migration. On the smallest surfaces WebP is larger than PNG, because its container overhead exceeds the payload.

Both candidates are pure Rust. `image` 0.25 no longer ships a WebP encoder and the popular `webp` crate binds libwebp in C, which is the same cross-platform build risk that disabling `image_dds`'s `encode` feature avoids.

### DLC archives ship no textures at this build

`docs/technical-design.md` stated that DLC archives may still supply referenced visual assets through the installation. All 30 archives under `dlc/` were opened — central directory only, no decompression — and they hold 1,241 `.wav`, 67 `.ogg`, 38 `.asset`, 33 extensionless, and 28 `.txt` entries. Nine are empty. There is no image of any format in any of them.

The sentence was inherited rather than measured, which is the kind of claim a spike exists to check. It is now narrowed in both the design and the decision log to say the case is unexercised at the pinned build, so the asset module reads textures from the installation tree and a future build that ships images inside an archive becomes a new source-selection question rather than a silently covered one.

### Disabling encoding removes a build-time toolchain from three release platforms

`image_dds`'s default features include `encode`, which depends on `intel_tex_2`, which compiles through the Intel ISPC compiler and ships no precompiled kernels for every target. Decoding never needs it, and [ADR 0005](../adr/0005-develop-on-macos-with-a-three-platform-release-target.md) commits to macOS, Windows, and Linux releases.

`default-features = false, features = ["ddsfile"]` drops `encode`, `image`, and `strum` in one line. It also happens to remove the `image_from_dds` entry point described above, so the configuration that protects the build is the same one that removes the wrong API.

## Rejected shortcuts

- **Validating whichever format you happened to open.** Two DXT5 icons, when DXT5 is 4.3% of the technology icons and 1,257 of them are a format that had never been decoded. The measurement that would have taken this shortcut is the census, which costs one pass over the headers.
- **Treating a successful decode as a correct one.** `image_from_dds` succeeds on all 12 cube maps and returns a six-fold vertical strip. Any coverage number built from "did it return `Ok`" would have recorded them as passes.
- **Round-tripping through the decoder's own encoder.** It shares the format tables the check exists to test, so the channel-order bug class is invariant under it; it pulls the ISPC toolchain into three release platforms; and BC encoding is lossy, so the comparison would need a tolerance. A check that is usually right is the failure mode the parser spike's span finding was written about.
- **Trusting a second library as the independent reading.** It was the plan, and it was wrong on 22 files. A second implementation is evidence about disagreement; the specification is evidence about correctness.
- **Counting divergences as a negative control.** A large divergence count says nothing about whether the check discriminates. Both injections assert the exact set of files the fault could reach — and one of those sets has 9 members out of 33,145.
- **Enumerating every `.dds` as the denominator.** 33,145 files exist; 7,241 vanilla paths are actually referenced, and 47 of those do not exist. A percentage over the first number is a fact about the filesystem.
- **Hand-editing binary fixtures.** Not reviewable, not reproducible, and a fixture nobody can regenerate becomes an assertion about itself.

## Completion model

### Evidence collection

Complete. Four records, every number traceable to one of them, the drift gate green across all four and demonstrated red twice.

### Decoder conformance

`image_dds` decodes every class the pinned corpora contain, and agrees with an independent specification-derived reading on all 33,104 inputs both accept, to within a one-count rounding difference. BC4, BC5, and BC7 are claimed by the adapter and exercised by nothing: the corpora contain none, so that support is declared but unmeasured.

### Known limitations, carried forward

- `ConversionFailure` is unreachable from any input in the pinned corpora and remains untested.
- BC4, BC5, BC6, and BC7 are unexercised.
- The reachable-set scan matches six sprite keys with a textual scan rather than a parse. It is a superset measure and is not the resolver; which sprite a documented concept resolves to remains `analysis`'s question.
- Model-material texture keys are excluded from the reachable set because their paths are relative to the declaring file rather than to the content root. That is a measured exclusion, not a preference, and it means the reachable set covers interface sprites only.
- Determinism is measured across processes on one machine. Cross-machine and cross-architecture reproducibility is not measured, and `image_dds` and `png` both contain code paths that could in principle differ; nothing here would detect that.
- No performance record was captured. Decode and encode throughput feed the revision-bundle spike's build budgets and remain unmeasured here.
- `verify` compares `rustc --version` byte for byte and the repository pins no toolchain, so every record here — and every parser record — goes red on the next Rust update. The recapture is cheap; the surprise is not.

### Current standing

| Dimension | Standing |
| --- | --- |
| Format coverage | Complete over the pinned corpora; three BC classes declared but unexercised |
| Correctness | Established by an independent reading plus spec-derived expectations |
| Typed outcomes | Total; three of four variants occur naturally |
| Recipe | Pinned, with every field measured against its alternative |
| Output format | PNG, with the WebP comparison recorded |
| Reproducibility | Complete; gate shown red before use |

## Captured records

| Run | Answers | Artifacts |
| --- | --- | --- |
| `d1-census` | What the corpus contains, what of it is reachable, and what the DLC archives hold | `census.json`, `header-faults.txt`, `dangling-references.txt`, `dlc-archives.txt` |
| `d2-decode` | Whole-corpus cross-check and per-input outcomes | `coverage.json`, `divergences.txt`, `outcomes.txt` |
| `d3-recipe` | Which parameters change the output; PNG against WebP | `recipe.json`, `canonical-recipe.txt` |
| `d4-failures` | The typed-outcome contract and its totality | `failures.json`, `fixture-outcomes.txt` |

Each manifest holds the run's purpose verbatim from its binary, the pinned environment, every corpus identified by tree digest, and every artifact by hash. The manifest is written last and hashes what is already on disk, so it can never name an artifact that was not produced.

The harness lives in `tools/dds-spike/` and is not a workspace member of `src-tauri`. [ADR 0008](../adr/0008-decode-source-textures-through-a-pinned-conversion-recipe.md) accepts `image_dds`; the dependency enters the application's Cargo graph when the production asset adapter is implemented.
