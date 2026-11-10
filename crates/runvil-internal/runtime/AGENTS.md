# runvil-runtime

- Only the executor decides store access. It derives every store view from the chain's access: at dispatch for a `Vfs` effect, and once at section-VM install for the section's `store` table. The caller uses the view it was given and never derives, widens, or keeps store scope from it.
