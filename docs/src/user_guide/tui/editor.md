# In-App Editing & File Viewing

## Editing

Open the actions menu (`x`) and select Edit Recipe, Edit Profile or Edit Collection. This fork opens an in-process UTF-8 editor by default on every platform. Request body editing and CLI collection/config editing use the same editor. No Vim installation is required.

Ctrl+S saves; F2 saves and closes; Esc or Ctrl+Q closes. Unsaved changes prompt for Y (discard), N (continue editing) or S (save and close). Cancellation never writes unsaved contents. Already saved changes remain saved. Files are replaced atomically after checking for another writer; conflicts preserve the editing buffer instead of overwriting the file.

Use arrows, Home/End and PageUp/PageDown to navigate. Shift+arrows select; Ctrl+A selects all; Ctrl+Z/Y undo/redo; Ctrl+F searches and F3 finds the next occurrence. Tab inserts two spaces. Ctrl+C/X/V use an internal clipboard. Use the terminal's paste shortcut to paste from the system clipboard; bracketed paste is supported. UTF-8, Chinese paths, spaces, CRLF and BOM are preserved.

The default does not use EDITOR or VISUAL. An explicit `editor: builtin` also selects the built-in editor. Outside portable mode, an explicitly configured external command such as `editor: code --wait` remains available. Portable mode always uses the built-in editor, even with an old `editor: vim` configuration.

## Paging

View Body (`v`, or the actions menu) opens a read-only in-process viewer. Use arrows, PageUp/PageDown and Ctrl+F/F3; Esc or Q closes it. Non-UTF-8 binary files are shown as hexadecimal. This does not start less, more, bat or another program.

The default ignores PAGER. `pager: builtin` explicitly selects the built-in viewer. An explicitly configured external pager remains an optional normal-mode integration; MIME-specific pager maps retain their existing format. Portable mode always uses the built-in viewer.

## Native filtering and export

The response query box accepts native JaQ filters such as `.data.list` or `jq '.items | map(.name)'`. The -r and -c flags are supported; the complete external jq CLI is not emulated. The selected raw or transformed response is the query input.

Export with `save PATH` or `tee PATH`. Paths may contain spaces and Windows backslashes, and may be surrounded by double quotes. New-file export refuses to overwrite an existing file. The regular Save Body dialog can be used for confirmed overwrites. Native cat and head -c/-n are also available.

An explicit `!command` retains custom shell integration in normal mode only. Portable mode rejects external commands before starting any program. Arbitrary user-installed scripts are not bundled language runtimes.
