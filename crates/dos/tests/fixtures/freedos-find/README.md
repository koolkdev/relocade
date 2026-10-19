# FreeDOS FIND fixture

Unmodified FIND 3.0b LFN (rev A) from the FreeDOS 1.3 repository package dated
2022-02-17.0. The integration test executes this binary as a guest program, with
command-line arguments and a mounted text file, on Wasmtime and Chrome.

Package: <https://www.ibiblio.org/pub/micro/pc-stuff/freedos/files/repositories/1.3/base/find.zip>

Run `python3 tools/fetch-test-assets.py` from the repository root before compiling
tests. [test-assets.toml](../../../../../test-assets.toml) pins the package by SHA-256
and lists the archive members to extract. `FIND.COM` and `SOURCES.ZIP` are generated
files, ignored by Git. The complete upstream package is cached in
`target/test-assets`, including its source and license notices.

The fetched files are byte-for-byte copies. The checked-in `COPYING` comes from
`DOC/FIND/COPYING`, with normalized line endings and trailing whitespace.

FIND is copyright Jim Hall, Eric Auer and Imre Leber, distributed under GPL-2.0-or-later
as stated in `FIND.C`. Its license is in `COPYING`. `SOURCES.ZIP` contains the upstream
source, build files and bundled library sources with their original notices.
The fixture retains those licenses independently of the emulator's MIT license.
