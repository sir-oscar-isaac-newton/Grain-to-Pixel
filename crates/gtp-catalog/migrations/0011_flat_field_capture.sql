-- A flat field remembers how its pictures were exposed and which files were kept (H-24).
-- `exposure` is a JSON ExposureInfo (shutter time, ISO, aperture) read from the pictures' EXIF; NULL
-- when unknown. `files` is a JSON list of the library-relative paths of the three NEF files the
-- capture view kept; NULL for a flat field made from files that were only read. Existing rows get NULL.
ALTER TABLE flat_field ADD COLUMN exposure TEXT;
ALTER TABLE flat_field ADD COLUMN files TEXT;
