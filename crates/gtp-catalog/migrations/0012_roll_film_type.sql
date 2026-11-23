ALTER TABLE roll ADD COLUMN film_type TEXT NOT NULL DEFAULT 'color_negative'
    CHECK (film_type IN ('color_negative', 'bw_silver', 'bw_chromogenic'));
