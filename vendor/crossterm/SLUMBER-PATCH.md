# Local Windows UTF-16 correction

Source: the Cargo.lock-verified crates.io crossterm 0.29.0 package. Original source and MIT license are retained.

Only src/event/sys/windows/parse.rs is functionally changed: ignore key-up records for UTF-16 surrogate code units before touching the pending high-surrogate buffer. Ordinary key-up events are unchanged. This prevents Windows key-down/key-up sequences from dropping supplementary Unicode characters.

The full Slumber binary is exercised in Windows ConPTY in Actions, including exact UTF-8 file equality with Chinese and emoji, x-menu editing/save/cancel and CLI editing. Linux PTY acceptance remains enabled. This source is statically compiled into the application, not a runtime program or DLL.
