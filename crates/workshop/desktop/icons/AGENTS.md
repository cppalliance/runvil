# Workshop icons

- Keep the Gateway app, Gateway configuration UI, and Workshop UI icon copies synchronized with the corresponding master icons in this directory.
- `installer-header.png`, `installer-header.bmp`, `installer-sidebar.png`, and `installer-sidebar.bmp` are hand-crafted installer assets. Do not regenerate, resize, overwrite, or modify them as part of ordinary icon generation.
- The Qt IFW installer's `installer-logo.png`, `installer-logo@2x.png`, `installer-watermark.png`, and `installer-watermark@2x.png` in `crates/build-workshop/ifw/config/` are scaled copies of the two PNG masters. When a master changes, regenerate them from this directory:
  - `sips -z 65 171 installer-header.png --out ../../../build-workshop/ifw/config/installer-logo.png`
  - `sips -z 130 342 installer-header.png --out ../../../build-workshop/ifw/config/installer-logo@2x.png`
  - `sips -z 310 162 installer-sidebar.png --out ../../../build-workshop/ifw/config/installer-watermark.png`
  - `sips -z 620 324 installer-sidebar.png --out ../../../build-workshop/ifw/config/installer-watermark@2x.png`
