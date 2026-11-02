# Prompt loader

This crate loads a Markdown prompt into a ready-to-run tree of frontmatter, sections, and compiled Lua code, and it reports each failure with a classified error. Every Lua fence in the H1 body and the sections compiles here, so a loaded prompt cannot fail on a syntax error partway through a run. It does no execution.
