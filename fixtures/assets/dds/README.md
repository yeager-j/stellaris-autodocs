# DDS fixtures

Generated source for the [DDS asset evaluation](../../../docs/spikes/dds-evaluation.md). They
cover what the real corpus cannot: formats the game does not ship, faults no shipped file may
contain on purpose, and blocks whose bit patterns state a rule the corpus only implies.

## Design

Three directories, because they answer different questions.

| Directory | Claim | Used by |
| --- | --- | --- |
| `valid/` | Decodes cleanly through **both** readings to the same RGBA8 | `d2-decode`, `d3-recipe`, `d4-failures` |
| `unsupported/` | A well-formed container the recipe **refuses**, with the refusal stated | `d4-failures` |
| `malformed/` | Exactly **one** deliberate fault per file | `d4-failures` |

One fault per file is the discipline of `malformed/`, inherited from
[`fixtures/parser/README.md`](../../parser/README.md): the resolver spike learned the expensive
way that when two faults share a file, the first failure cascades and the second result becomes
unattributable.

## Where the prediction lives

`fixtures/parser/malformed/*.txt` states its prediction in a header comment, written before the
measurement so a surprising number is a finding rather than a retrofitted expectation. A binary
file cannot carry a comment, so the discipline moves into committed source instead.

Every fixture here is a **pure function** of a `Fixture` value in
[`tools/dds-spike/src/fixtures.rs`](../../../tools/dds-spike/src/fixtures.rs), which holds its
`claim`, its predicted `expected` outcome, and — where the value is computable by hand from the
format specification — its `expected_pixels`. Nothing here is hand-edited. `generate --check`
regenerates every file in memory and compares it byte for byte with what is committed, and
`tests/fixtures.rs` runs that check, so a fixture cannot drift from the statement describing it.

Headers are written from the DDS specification rather than through `image_dds`. A fixture produced
by the library under test would be a snapshot of that library, not an independent statement about
what the format means.

The spec-derived pixels matter for a reason agreement cannot supply. Two decoders agreeing shows
they are not *independently* wrong; a value computed from the specification shows they are not
*jointly* wrong. `dxt1_opaque_4x4.dds` chooses endpoints — pure red and pure blue at full 5-bit
precision — so the 1/3 and 2/3 interpolants land exactly on 170 and 85 with no rounding to argue
about.

## Why these cases

The pinned corpora already exercise DXT1, DXT3, DXT5, three uncompressed layouts, mip chains, cube
maps, and block-unaligned surfaces at a scale no fixture can match. These fill in what it does not
reach:

- **Channel order stated twice.** `bgra8_2x2.dds` and `rgba8_2x2.dds` differ only in their masks.
  A decoder keyed on bit count reads them identically and silently swaps red with blue. Eleven
  files in the pinned corpora declare the second layout, which is enough to catch the fault but
  not enough to explain it.
- **BC3 with reversed colour endpoints.** `dxt5_reversed_endpoints_4x4.dds` is the fault in
  isolation. The BC2 and BC3 specifications say their colour block always uses four-colour
  interpolation, so `c0 <= c1` carries no second meaning there. 22 files in the pinned corpora
  contain such blocks, and a decoder applying BC1's punch-through rule to them differs by up to
  255 in a channel. No fixture would have been written to contain an ordering the spec says is
  meaningless — the corpus found it, and this file explains it.
- **Formats the game does not ship.** `x8r8g8b8_2x2.dds` is well-formed, recognized by the
  container reader, and named by no `image_dds` format. Zero occurrences locally, and the likeliest
  form a foreign mod ships. `dxt2_premultiplied_4x4.dds` and `volume_2x2x2.dds` are the same
  argument for premultiplied alpha and volume textures.
- **Faults the corpus contains but cannot isolate.** Vanilla ships a zero-byte icon and a
  three-byte file holding only a byte order mark, so `empty.dds` and `bom_only.dds` mirror real
  inputs. The other five malformed files — bad magic, wrong header size, wrong pixel-format size,
  zero height, truncated pixel data, truncated DX10 header — have no shipped counterpart, and
  without them those branches would never execute.
- **Lossy round trips made explicit.** `bgr5a1_2x2.dds` stores a one-bit alpha, so an input alpha
  of 128 comes back as 255. The fixture states that rather than leaving it to be discovered.
- **Degenerate shapes.** `dxt5_1x1.dds` and `dxt5_3x2.dds` are smaller than one 4x4 block. The
  workshop corpus contains a 1x1 DXT5 texture, so this is a shipped case, not a hypothetical.

## Licensing

Every byte here is original work for this repository. No Stellaris content is copied. The images
are 1x1 to 8x8 synthetic colour patterns produced by the generator.

## Running

```bash
cargo run --manifest-path tools/dds-spike/Cargo.toml --bin generate
```

```bash
cargo run --manifest-path tools/dds-spike/Cargo.toml --bin generate -- --write
```

The first checks the committed bytes against their generator and exits non-zero on drift. The
second is the only thing that writes to this directory.
