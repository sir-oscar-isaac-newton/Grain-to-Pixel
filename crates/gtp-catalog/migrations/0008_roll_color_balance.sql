-- The automatic colour balance of a roll of scanner frames (P-22): the median over its frames of
-- the colour axes (red-cyan, green-magenta, blue-yellow) that make their mid-tones neutral. All
-- three are NULL until a scan or import has judged a frame of the roll; the first one stored stays.
-- Existing rolls keep NULL.
ALTER TABLE roll ADD COLUMN balance_red_cyan REAL;
ALTER TABLE roll ADD COLUMN balance_green_magenta REAL;
ALTER TABLE roll ADD COLUMN balance_blue_yellow REAL;
