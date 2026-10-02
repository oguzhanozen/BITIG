ALTER TABLE folders ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0);

-- Keep the previous alphabetical order as the initial user-visible order.
WITH ranked AS (
    SELECT id,
           ROW_NUMBER() OVER (
               PARTITION BY parent_id
               ORDER BY lower(name), created_at, id
           ) - 1 AS position
    FROM folders
)
UPDATE folders
SET sort_order = (SELECT position FROM ranked WHERE ranked.id = folders.id);

CREATE INDEX folders_parent_sort_idx ON folders(parent_id, sort_order);
