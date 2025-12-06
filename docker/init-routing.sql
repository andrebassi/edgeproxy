-- Docker routing database with container IPs
CREATE TABLE IF NOT EXISTS backends (
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

DELETE FROM backends;

INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES
  -- South America backends (10.10.1.x)
  ('sa-node-1', 'myapp', 'sa', '10.10.1.1', 8080, 1, 2, 50, 100),
  ('sa-node-2', 'myapp', 'sa', '10.10.1.2', 8080, 1, 1, 50, 100),
  ('sa-node-3', 'myapp', 'sa', '10.10.1.3', 8080, 1, 1, 50, 100),
  -- US backends (10.10.2.x)
  ('us-node-1', 'myapp', 'us', '10.10.2.1', 8080, 1, 2, 50, 100),
  ('us-node-2', 'myapp', 'us', '10.10.2.2', 8080, 1, 1, 50, 100),
  ('us-node-3', 'myapp', 'us', '10.10.2.3', 8080, 1, 1, 50, 100),
  -- EU backends (10.10.3.x)
  ('eu-node-1', 'myapp', 'eu', '10.10.3.1', 8080, 1, 2, 50, 100),
  ('eu-node-2', 'myapp', 'eu', '10.10.3.2', 8080, 1, 1, 50, 100),
  ('eu-node-3', 'myapp', 'eu', '10.10.3.3', 8080, 1, 1, 50, 100);
