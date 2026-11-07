# runvil-vfs

- The machinery modules hold no run concepts. The declared store is generic machinery (`VfsRefBuilder::store` names any mount as the store), so it lives with the handle; the mode gate stays at the crate root.
- Origin labels are the most specific available: a section name for a chain, a tool id for a tool, a fixture name for a test.
