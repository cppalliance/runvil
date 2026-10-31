# The Gateway pin

`pin.json` here records the one Gateway release Workshop obtains its
sidecar from, and the checksums that prove the bytes. `openapi.yaml` and
`asyncapi.yaml` are the committed spec copies the same release published.

## What a person must fill in

Every field in `pin.json` is a placeholder right now, because no Gateway
release exists yet. Before any release or nightly job can run:

1. Publish a Gateway release. It must attach, for all five target triples,
   the binary archive cargo-dist builds; `sidecar.json`; `openapi.yaml`; and
   `asyncapi.yaml`.
2. Run `node tools/bump-gateway.mjs <tag>`. It downloads that release's
   manifest and specs, checks the specs against the manifest's hashes, and
   rewrites `pin.json` and the two spec copies in one step. Nothing is
   written if any download or check fails.

`bump-gateway.mjs` fills every field, so there is nothing to hand-edit. If
you must edit by hand, these are the fields and what each one means:

- `tag`: the release tag, for example `gateway-v0.4.0`.
- `version`: the Gateway version that tag carries, without the tag prefix.
- `min_gateway`: the lowest Gateway release that has every operation
  Workshop calls. Workshop warns in the status bar when it attaches to an
  older Gateway.
- `executable`: the executable's name inside each archive, `runvil-gateway`.
  Tauri renames it per triple when the sidecar is staged.
- `spec.openapi.sha256` and `spec.asyncapi.sha256`: the sha256 of the two
  committed spec files beside this file.
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
