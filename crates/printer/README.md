# Printer peripherals

`Capture::attach` consumes a `relocade_parallel::centronics::Input` on the
execution session. The application chooses its nonzero byte capacity. A full
capture reports BUSY and waits for an explicit `clear`; it never silently
truncates a job or reports the buffer limit as an empty paper tray.

`snapshot` returns the original bytes without consuming them, including the
connection's accepted pending byte when execution handles the request. Use it
for preview, export or printing; cancelling a request leaves the job available.
`clear` starts a new job. INIT, AUTOFEED, form feed and closing a DOS handle do
not define job boundaries or alter captured bytes.

The peripheral does not bind DOS names. Applications independently install the
LPT and PRN drivers using the writer returned by the same Centronics connection.
