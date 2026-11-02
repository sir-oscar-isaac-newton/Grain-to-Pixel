-- The size of one frame on a roll's captures, as fractions of the image (frame detection places a
-- window of this size), and how many measured frames it averages (0 when set by hand). NULL until
-- measured or set. Existing rolls keep NULL.
ALTER TABLE roll ADD COLUMN frame_w REAL;
ALTER TABLE roll ADD COLUMN frame_h REAL;
ALTER TABLE roll ADD COLUMN frame_samples INTEGER;
