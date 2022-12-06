CREATE TABLE images (
  filename TEXT PRIMARY KEY,
  hash blob NOT NULL,
  pic_store_id text,
  html TEXT NOT NULL,
);

