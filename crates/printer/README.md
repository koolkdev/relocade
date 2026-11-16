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

`text::render_pdf` turns a snapshot into PDF bytes. The text profile uses CP437, 80 columns, 60 lines, CR, LF, eight-column
tabs, backspace overprinting and form feed. LF alone preserves the column; use
CRLF for a new line at the left margin. A final form feed adds no blank page,
while repeated form feeds explicitly produce blank pages. Unsupported control
commands fail the whole render, retaining the raw capture for another renderer.
The profile is limited to 64 pages and does not interpret ESC/P, PCL or PostScript.

The PDF embeds compressed monochrome pages with the emulator's bundled Modern DOS
8×8 glyphs (CC0). Each page uses US Letter with half-inch margins.
