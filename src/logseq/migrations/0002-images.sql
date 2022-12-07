CREATE TABLE images (
  filename TEXT PRIMARY KEY,
  version INTEGER NOT NULL,
  hash blob NOT NULL,
  data TEXT NOT NULL
);

