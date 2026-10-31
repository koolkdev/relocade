# Rust DOS console

`run_console(&mut dos).await` runs on the execution owner. It prints a `> ` prompt,
reads an edited line through the shared DOS console, and implements `echo` with
case-insensitive command matching. Enter, Backspace, blank lines and unknown
commands return to the next prompt. This is a small Rust command loop, not a
COMMAND.COM compatibility implementation.

The console uses `dos.console().write(bytes)` and `read_line().await`; its text,
cursor and scrolling use the same VGA state as BIOS and DOS program output.
It does not create a renderer or a worker. Existing `launch_com` and `launch_mz`
accept supplied bytes; executable lookup and storage selection are not defined here.
