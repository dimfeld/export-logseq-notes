CREATE TABLE pages (
  filename TEXT PRIMARY KEY,
  hash blob NOT NULL,
  created_at bigint NOT NULL,
  edited_at bigint NOT NULL
);

CREATE INDEX pages_hash ON pages (hash);
