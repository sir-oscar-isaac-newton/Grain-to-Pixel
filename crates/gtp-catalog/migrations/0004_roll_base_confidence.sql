-- How sure the automatic measurement of a roll's film base was (0 to 1). NULL when the base was
-- set by hand, or there is none yet. Existing rolls keep NULL.
ALTER TABLE roll ADD COLUMN film_base_confidence REAL;
