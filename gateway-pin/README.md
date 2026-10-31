# The Gateway pin

`pin.json` here records the one Gateway release Workshop obtains its
sidecar from, and the checksums that prove the bytes.

## What the release must publish

A Gateway release supplies everything the pin needs through the three files
cargo-dist writes for it:

- `sha256.sum`, one line of sha256 and asset name per released file.
- `dist-manifest.json`, the build record, which names each archive's target
  triple, its checksum, and the executable inside it.
- The five platform archives themselves, one per supported triple.

The release's own asset list supplies the size of each archive. The script
reads the name and hash from `dist-manifest.json` and requires `sha256.sum`
to state the same hash for that name, then checks the digest GitHub reports
for the uploaded asset when the release carries one. Any disagreement stops
the run before a byte is written.

No OpenAPI or AsyncAPI document is expected. The Gateway repository
publishes none, so the pin records no spec and says nothing about one. If a
later release attaches `openapi.yaml` or `asyncapi.yaml`, the script copies
it here and records its sha256.

## Filling the pin

Run `node tools/bump-gateway.mjs <tag>`. It downloads the release's two
checksum sources, cross-checks them, rewrites `pin.json`, and prints a diff.
Nothing is written if any download or check fails.

`bump-gateway.mjs` fills every field, so there is nothing to hand-edit. If
you must edit by hand, these are the fields and what each one means:

- `tag`: the release tag, for example `gateway-v0.3.0`.
- `version`: the Gateway version that tag carries, without the tag prefix.
- `min_gateway`: the lowest Gateway release that has every operation
  Workshop calls. Workshop warns in the status bar when it attaches to an
  older Gateway. It equals `version` for the release being pinned.
- `executable`: the executable's name inside each archive, `runvil-gateway`.
  Tauri renames it per triple when the sidecar is staged.
- `spec`: empty when the release publishes no spec. An entry that is
  present holds `path` and `sha256`, and a placeholder there is refused.
- `targets.<triple>.asset`: the release asset name for that triple.
- `targets.<triple>.sha256`: that asset's sha256.
- `targets.<triple>.size`: that asset's size in bytes.

Every consumer refuses a placeholder by name. `tools/fetch-gateway-sidecar.mjs`
will not download, so a release job fails at the fetch rather than shipping
a Workshop with no Gateway.

## What reads the pin

- `tools/fetch-gateway-sidecar.mjs` downloads, verifies, and stages one
  target's binary. `--stub` writes a placeholder instead and needs no pin.
- `tools/bump-gateway.mjs` rewrites this file from a published release.
- `crates/workshop/gateway/build.rs` embeds `min_gateway` so the running
  Gateway's version can be compared with it at attach.
