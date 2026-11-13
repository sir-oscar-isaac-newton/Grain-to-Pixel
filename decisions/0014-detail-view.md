# 0014. The detail view renders the whole picture at full size and cuts regions from one cached copy

- Status: Accepted (the owner approved the design, issue U-29 / GitHub #319)
- Date: 2026-11-11

## Context

The Develop view draws a 2048 px preview, so zooming in only enlarges pixels. Noise reduction, output
sharpening and dust removal are judged on the pixels, and the first two behave differently on a
preview (noise reduction scales its radii by the proxy's size, sharpening is never applied to a
preview). Issue U-29 asks for a 1:1 view that shows what a full-size export would write. The open
questions are how much of the picture to render per request, where the result is kept, how it is
tied to the export so the two cannot drift apart, and when the memory is given back.

## Decision

1. **Render the whole picture, once, through the export's code path.** `export::render_full` (read
   the scan, the roll's flat field and infrared plane through `export::read_full`; `render` at
   `RenderTarget::Full`; optional resize; output sharpening at the final size) is the one function
   the export and the detail view both call. The export passes its size option, the detail view
   none, so a full-size export and the detail are the same picture. The detail uses the recipe it is
   given and not the saved one.
2. **Keep one picture, as RGBA8 sRGB.** The Develop service holds at most one: the frame, its clamped
   recipe, the roll's film base and the dust flag it was made with (a change of the base or of the
   flag makes it stale), its size and the pixels (the same conversion the previews use), 4 bytes a
   pixel: 96 MB for 24 megapixels. The float images are dropped as soon as it is made. A request for
   the same frame and context cuts a region from it; any other replaces it. A mutex around "render if
   the cache does not hold this" makes two requests for the same recipe render once.
3. **A region is a rectangle in fractions of the picture**, rounded outwards to whole pixels and at
   most `MAX_DETAIL_EDGE` = 4096 pixels a side. The buffer starts with a 32-byte header
   (`DETAIL_HEADER_BYTES`: `seq` u64, region size, region origin, full picture size, little-endian) so
   the UI learns the size of the rendered picture (which depends on the crop and the straightening)
   from the first answer, and the interface of `DevelopInfo` does not change.
4. **Sequence numbers are the detail view's own**, per frame, so a detail request never cancels a
   preview and the reverse. A request superseded by a newer one for the frame is `Cancelled`. A
   render that is already running finishes and keeps its picture unless the newer request brought
   another recipe, so a pan (same recipe, new region) wastes nothing.
5. **Release.** The picture is cleared with the proxy cache when another library opens, replaced when
   another frame is asked for, dropped when the Develop view is left (the command
   `develop_release_detail`, approved with this ADR's follow-up; the proxy cache stays), and dropped
   after `DETAIL_IDLE_SECS` = 300 s without a request. The idle check runs lazily on the next detail
   request and on `develop_open`, with the time passed in so a test does not wait; no timer thread
   is added. A release does not wait for a render in flight: it bumps a release generation, and a
   render that began before it discards its picture when it ends (the caller gets `Cancelled`), so
   nothing is stored after the view has gone.
6. **The UI** asks for the visible part grown by half its size on each side once the recipe has
   settled, hides the layer while a drag runs or the picture on hand belongs to another recipe, and
   keeps the pickers and the crop on the preview, since their regions are in preview coordinates.

## Rejected

- **Render only the visible region with a margin.** Noise reduction, dust removal, straightening and
  the vignette each need neighbours, or the position in the whole picture, so each would need
  region-aware logic, and each could then differ from the export. One full render through the
  export's function cannot.
- **Keep the full scan in memory** and render regions from it: about 290 MB more for a 24 megapixel
  frame (3 channels of `f32`, plus the infrared plane), for a render that still needs the whole
  picture for the stages above.

## Consequences

- After each change at 1:1 the layer waits for a full render: the budget is 3 s for a 24 megapixel
  scan with every tool on, on 6 cores (measured 2.8 s without the decode, see the specification's
  section 11; after P-31 phase 2, decode included, 2.98 s on 6 threads, 2.36 s on 12). A region from the cached picture takes a few milliseconds. The enlarged preview shows
  in the meantime, with a status line.
- The service holds up to 96 MB more (24 megapixels) while the Develop view is open, and during a render
  the float working images are alive as well (a peak like an export's). The old picture is dropped
  before the new one is made, so two are never held at once.
- A resized export sharpens at its smaller size, so it differs from the detail, which is always at
  the full size. The notice under the picture says so.
- The preview converts with a matrix and a clip, a file with the colour profiles, as before; the
  detail is converted like a preview.
