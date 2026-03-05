# Bundled Third-Party Tools

DVD2CHD ships pre-built binaries of the following tools inside
`tools/` (tarball / AppImage) or `/usr/lib/dvd2chd/tools/` (.deb / .rpm).
These binaries are **not** modified and are provided solely for convenience.
You can replace them at any time with system-installed versions – DVD2CHD
will prefer a system binary found via PATH over the bundled one.

---

## chdman

- **Project**: MAME (Multiple Arcade Machine Emulator)
- **License**: BSD 3-Clause ("New BSD License")
- **Source**: <https://github.com/mamedev/mame>
- **Copyright**: Copyright (c) Nicola Salmoria and the MAME team

Full license text: <https://raw.githubusercontent.com/mamedev/mame/master/COPYING>

---

## cdrdao

- **Project**: cdrdao – Disc-At-Once Recording of Audio and Data CD-Rs
- **License**: GNU General Public License v2.0 (GPL-2.0-only)
- **Source**: <https://cdrdao.sourceforge.net> / <https://github.com/cdrdao/cdrdao>
- **Copyright**: Copyright (c) Andreas Mueller and contributors

In compliance with the GPL v2, the complete corresponding source code is
available at the project homepage linked above.

---

## ddrescue (GNU ddrescue)

- **Project**: GNU ddrescue
- **License**: GNU General Public License v2.0 or later (GPL-2.0-or-later)
- **Source**: <https://www.gnu.org/software/ddrescue/>
- **Copyright**: Copyright (c) Antonio Diaz Diaz

In compliance with the GPL v2+, the complete corresponding source code is
available at the project homepage linked above.

---

## Note on GPL and DVD2CHD

DVD2CHD's own source code (dvd2chd-core and dvd2chd-gui) is licensed under
the **MIT License** (see `LICENSE`). The GPL applies exclusively to the
bundled cdrdao and ddrescue binaries. DVD2CHD communicates with these tools
by spawning them as separate processes and does not link against them, so
there is no combined-work implication for the DVD2CHD source code.
