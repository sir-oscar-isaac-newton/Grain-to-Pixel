# 0010. A flat field per roll, applied when a camera frame is read

- Status: Accepted (the author approved the design on 2026-11-02, issue P-21 / GitHub #194)
- Date: 2026-11-02

## Context

The light of the author's lightbox is not even across a Nikon D600 frame: a bare-light triplet falls to
0.38 to 0.61 of the centre at the edges, red most. A negative shot on it carries the same falloff as a
red edge cast. Frames shot with the same camera settings share it, so it can be measured once.

## Decision

1. **A flat field belongs to a roll.** It is made from three bare-light exposures (red, green, blue,
   told apart by colour like any triplet) and stored in the catalog table `flat_field` (migration
   0006), keyed by `roll_id`, deleted with the roll. A roll has none, or one: attaching replaces it.
2. **It is a smoothed gain grid, not the capture.** Block means over 96 columns of cells, normalised
   to 1 at the centre (the mean of the centre 3 by 3 cells), floored at 0.05 so a dead corner cannot
   explode the gain. Dust and noise do not survive the averaging; the light does.
3. **It divides the frame when the frame is read**, in `read_scan` (Develop's load and export) and in
   the import's analysis, for camera-triplet frames only. The stored NEF planes stay raw, so removing
   or replacing the flat field changes the roll without re-importing, and nothing is lost.
4. **Size must match.** A frame of another size than the flat field is not corrected. Attaching a flat
   field of another size than the roll's existing frames is refused; a later frame of another size is
   imported uncorrected with the warning `FlatFieldSkipped`.
5. **Attaching or removing it** drops Develop's cached proxies, makes the roll's thumbnails again, and
   measures an automatic film base again from the corrected first frame (a hand-picked base stays).
6. **The benchmark applies it with the app's own code** (`flat_field` in the manifest, three files) and
   reports an edge cast per frame.

## Consequences

- **The edge cast goes.** The red-minus-green excess of the left and right edges over the centre,
  ours minus Trichroma Lab's, was +6 to +47 on 12 of the 15 frames and is within ±8 on all 12
  afterwards. Frames 13 and 14, which were -17/+14 and -12/+20, are now -28/-16 and -23/-9
  (over-corrected on the left), and frame 15 is +10/+10 (was +12/+25).
- **The film base changes** (0.148 0.098 0.065 to 0.190 0.105 0.075): it was measured under the
  vignette before. Existing rolls keep their base until a flat field is attached.
- **One field per roll** is all the author needs; a library of fields per lightbox setup is not built.
- **It corrects the light and the lens together,** and only for the settings it was shot with: a
  flat field made at another aperture or distance corrects wrongly. The app does not check this.
- **Cost:** a gain lookup per pixel on every load, parallel over rows; not measured.