-- The pick or reject mark of a frame (U-20): 'none' (not judged yet), 'pick' (worth editing and
-- exporting) or 'reject' (to be deleted). It is the first culling step after a scan. Existing frames
-- start as 'none'.
ALTER TABLE frame ADD COLUMN flag TEXT NOT NULL DEFAULT 'none' CHECK (flag IN ('none', 'pick', 'reject'));
