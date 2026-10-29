# runvil-engine

- Concrete providers stay in their provider crates. The Engine may re-export them for crate-internal use but never reacquires provider implementation.
- Store access is decided only by the executor: every store view is derived from the chain's access inside the Engine at dispatch for a `Vfs` effect and once at section-VM install for the section's `store` table; the caller, performing a `Vfs` effect, uses the view it was given and never derives, widens, or retains store scope from it.
