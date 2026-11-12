# Markdown blocks

This crate reads Markdown text into a tree of plain blocks. Each block holds its source, the lines it was read from with the blank lines after them, and the sources of a parse are the whole text, so writing them back gives the text that was read. The crate also holds the two grammars that name things in that tree: the section path and the block selector. `pulldown-cmark` reads the Markdown with the options the prompt loader uses, so a heading or a fence means the same thing in both. It holds no Lua and does no execution.
