---
name: Move the Workshop pin and document gateway-client
overview: Move the Workshop commit that the installers are built from to the Workshop commit that pins the Gateway client adapter, and add gateway-client as a fifth rustdoc site, so the adapter's documentation page links to a page that exists. The workspace version and the toolchain already match Workshop's.
todos:
  - id: runvil-pr
    content: "Move the Workshop rev, add gateway-client to the rustdoc sites; pull request from the fork, CI, fast-forward merge"
    status: pending
isProject: false
---

# Move the Workshop pin and document gateway-client

This is the part of the extraction plan that concerns this repository. `harness-gateway-client` is now an adapter over `gateway-client`, a repository of its own. Workshop pins the Harness commit that holds the adapter, and this repository builds its installers from that Workshop commit. This change moves the Workshop `rev` and adds the rustdoc site for `gateway-client`.

### runvil (step D2)

- **`Cargo.toml`.** `rev` in `[workspace.metadata.workshop]` moves to W. The workspace version and the toolchain channel change only if Workshop's did, because `cargo installer` refuses a checkout whose version or toolchain differs. Compare them with Workshop's at W.
- **`crates/build-xtask/src/site.rs` and `site-tests.rs`.** Add `gateway-client` as a fifth rustdoc site (the array type goes from 4 to 5 entries) with a few words for the landing row, such as "Chat completions for any server, and finding a Gateway". The adapter's docs page links the client's types, and without a site of its own those links lead nowhere. `cargo doc -p gateway-client` must select a git dependency from the Harness checkout. I expect it does, and the `site` workflow in CI is the check. Confidence: medium. If CI shows otherwise, this edit comes out and the pull request keeps only the pin.
- **The local commit.** `runvil` has one local commit, "Require the web Plugin in the install cells", that is not on upstream. It changes the two install scripts and not `Cargo.toml`, and it rides along in this pull request.
