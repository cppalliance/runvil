# Engine internals

This directory holds the Engine's private crates, which together parse a prompt program and step its run. Code outside the Engine reaches them only through the Engine's two public crates, `runvil` and `runvil-plugin`, the only crates outside this directory allowed to depend on it.
