-- Fly.io backends for edgeProxy
-- 10 regions: gru, iad, ord, lax, lhr, fra, cdg, nrt, sin, syd

DELETE FROM backends;

INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES
  -- South America
  ('fly-gru-1', 'edgeproxy-backend', 'sa', 'fdaa:9:3ca6:a7b:5c5:84eb:a8dd:2', 8080, 1, 1, 100, 200),

  -- North America
  ('fly-iad-1', 'edgeproxy-backend', 'us', 'fdaa:9:3ca6:a7b:9d36:68ea:a86c:2', 8080, 1, 1, 100, 200),
  ('fly-ord-1', 'edgeproxy-backend', 'us', 'fdaa:9:3ca6:a7b:569:9513:f0fc:2', 8080, 1, 1, 100, 200),
  ('fly-lax-1', 'edgeproxy-backend', 'us', 'fdaa:9:3ca6:a7b:f8:9246:574f:2', 8080, 1, 1, 100, 200),

  -- Europe
  ('fly-lhr-1', 'edgeproxy-backend', 'eu', 'fdaa:9:3ca6:a7b:4a0:bb4d:8303:2', 8080, 1, 1, 100, 200),
  ('fly-fra-1', 'edgeproxy-backend', 'eu', 'fdaa:9:3ca6:a7b:47a:6312:f024:2', 8080, 1, 1, 100, 200),
  ('fly-cdg-1', 'edgeproxy-backend', 'eu', 'fdaa:9:3ca6:a7b:5b5:412c:501:2', 8080, 1, 1, 100, 200),

  -- Asia-Pacific
  ('fly-nrt-1', 'edgeproxy-backend', 'ap', 'fdaa:9:3ca6:a7b:2e1:cf04:568f:2', 8080, 1, 1, 100, 200),
  ('fly-sin-1', 'edgeproxy-backend', 'ap', 'fdaa:9:3ca6:a7b:581:ef9d:80e7:2', 8080, 1, 1, 100, 200),
  ('fly-syd-1', 'edgeproxy-backend', 'ap', 'fdaa:9:3ca6:a7b:f5:1f25:ab4e:2', 8080, 1, 1, 100, 200);
