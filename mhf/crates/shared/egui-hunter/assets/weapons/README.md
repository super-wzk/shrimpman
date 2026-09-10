# MHF weapon icons

The 14 PNGs are unchanged 64×64 crops from the MHF-ZZ client's `dat/mhf.bin`,
PNG entry 54 (zero-based). `source.json` records the archive/PNG SHA-256 values,
byte offset and crop coordinates for each weapon class. Original artwork is
from the MHF client (CAPCOM); these are not newly designed symbols.

Keep the original canvas, placement, RGB colors and alpha. Weapon icons use
white texture tint, including when their surrounding control is selected.
Selection and focus are indicated by the control instead of recoloring artwork.
The embedded PNGs are decoded once and cached per egui context.
