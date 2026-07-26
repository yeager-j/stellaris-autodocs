# DDS asset evaluation

Status: Feasibility validated; reproducibility artifacts must be captured before implementation planning.

## Established result

The installed corpus contained uncompressed RGB or RGBA plus DXT1, DXT3, and DXT5 technology icons. A representative Vanilla DXT5 icon and modded DXT5 icon were converted to 52×52 RGBA PNG and visually inspected successfully. This establishes feasibility, not a reproducible decoder contract.

## Required reproducibility record

The repository must retain:

- Exact `image_dds` crate version and all enabled features.
- Rust toolchain version and conversion helper source.
- Complete invocation and conversion parameters.
- Input logical path, byte length, dimensions, detected pixel format, and SHA-256.
- Output format, byte length, dimensions, and SHA-256.
- Visual-inspection result.
- Expected failure behavior for malformed and unsupported samples.

License-compatible samples are committed under `fixtures/assets/dds/`. Proprietary Vanilla or mod assets are not redistributed without permission. For those, the record retains the logical path and checksum so a user with the licensed local installation can reproduce the run.

## Artifact matrix

| Sample | Redistribution | Input format | Input SHA-256 | Converter | Output SHA-256 | Status |
| --- | --- | --- | --- | --- | --- | --- |
| Vanilla technology icon | Local licensed installation only | DXT5 | Pending recapture | Pending recapture | Pending recapture | Required |
| Mod technology icon | Permission-dependent | DXT5 | Pending recapture | Pending recapture | Pending recapture | Required |
| Synthetic or permissively licensed fixture | Commit to repository | RGB or RGBA | Pending | Pending | Pending | Required |
| Synthetic or permissively licensed fixture | Commit to repository | DXT1 | Pending | Pending | Pending | Required |
| Synthetic or permissively licensed fixture | Commit to repository | DXT3 | Pending | Pending | Pending | Required |
| Synthetic or permissively licensed fixture | Commit to repository | DXT5 | Pending | Pending | Pending | Required |
| Malformed fixture | Commit to repository | Invalid DDS | Pending | Pending | Not applicable | Required |

## Acceptance

Asset conversion implementation begins only after the converter source, pinned versions, commands, hashes, and license-compatible fixtures are present. The asset test suite must reproduce every successful output hash and every typed failure outcome.
