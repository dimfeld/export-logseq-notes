CREATE TABLE images (
  filename TEXT PRIMARY KEY,
  hash blob NOT NULL,
  data TEXT NOT NULL,
);

