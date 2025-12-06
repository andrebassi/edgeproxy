CREATE TABLE backends (
    id TEXT PRIMARY KEY,
    app TEXT,
    region TEXT,
    wg_ip TEXT,
    port INTEGER,
    healthy INTEGER,
    weight INTEGER,
    soft_limit INTEGER,
    hard_limit INTEGER,
    deleted INTEGER DEFAULT 0
);

INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES
  ('sa-node-1', 'myapp', 'sa', '10.50.1.1', 8080, 1, 1, 100, 200),
  ('us-node-1', 'myapp', 'us', '10.50.2.1', 8080, 1, 1, 100, 200),
  ('eu-node-1', 'myapp', 'eu', '10.50.3.1', 8080, 1, 1, 100, 200);
