-- Where a scanned frame came from: its frame number on the roll as the scanner counts it (1 to 40),
-- so a scan can resume at the next one, and how it was scanned (a small JSON record of device, dpi,
-- samples and autofocus). NULL for frames that were imported from files. Existing frames keep NULL.
ALTER TABLE frame ADD COLUMN scan_number INTEGER;
ALTER TABLE frame ADD COLUMN scan_info TEXT;