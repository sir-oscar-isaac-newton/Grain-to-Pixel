-- How a captured frame was lit (H-15): a small versioned JSON record of the light levels used, where
-- they came from (the roll's film stock, the global default or set by hand) and the camera settings
-- of the three exposures. NULL for frames that were imported from files or scanned, and for
-- existing frames.
ALTER TABLE frame ADD COLUMN capture_info TEXT;
