# Definition edits

Each file in this directory is Cursor's definition of the tool named by its
`wire_name`, snapshot on 2026-10-27. Everything in a file is Cursor's text
except the edits below, each made because the line describes behavior this
Pack does not have.

## read.json

- `Reads a file from the local filesystem.` became `Reads a file from the
  run's filesystem.`, because every read goes through the run's virtual
  filesystem, not the real one.
- `Assume this tool is able to read all files on the machine.` became
  `Assume this tool is able to read every file the run can see.`, for the
  same reason.
- `If the User provides a path to a file` became `If the prompt provides a
  path to a file`, because a Runvil run has a prompt rather than a chat
  user.
- Removed: `Image Support`, `PDF Support`, and the paragraph explaining how
  images and PDFs are returned. This build reads text only, so a model told
  otherwise would send a PDF and get an error.

## write.json

- Cursor's `Write` description, which is about creating or overwriting a
  file. Reworded to name `StrReplace` for targeted changes and to require a
  read before overwriting, matching Cursor's own guidance. No behavior was
  added; only the wording is ours.

## replace.json

- Cursor's `StrReplace` definition. The wording is ours; the parameters,
  their types, and the `required` list are Cursor's.

## rm.json

- Verbatim. Cursor's `Delete` description and schema are kept exactly.

## glob.json

- The `target_directory` description's default, `Cursor workspace root`,
  became `the run's filesystem root`.

## grep.json

- Removed the sentence pointing at `rg`, since the tool searches the run's
  filesystem itself.
- Removed the sentence pointing at the `Shell` tool for piping, because this
  Pack offers no `Shell`.
- The `path` parameter's default, `Cursor workspace root`, became `the
  run's filesystem root`.
- Every parameter, type, description, enum, and the `required` list is
  otherwise Cursor's.
