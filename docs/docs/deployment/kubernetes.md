---
sidebar_position: 2
---

# Kubernetes Deployment

This guide covers deploying edgeProxy on Kubernetes, including manifests, Helm charts, and multi-cluster configurations.

## Prerequisites

- Kubernetes 1.25+
- kubectl configured
- Helm 3.x (optional)

## Basic Deployment

### Namespace

```yaml
# namespace.yaml
apiVersion: v1
kind: Namespace
metadata:
  name: edgeproxy
  labels:
    app.kubernetes.io/name: edgeproxy
```

### ConfigMap

```yaml
# configmap.yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: edgeproxy-config
  namespace: edgeproxy
data:
  routing.sql: |
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

    INSERT OR REPLACE INTO backends VALUES
        ('sa-node-1', 'myapp', 'sa', '10.50.1.1', 8080, 1, 2, 50, 100, 0),
        ('sa-node-2', 'myapp', 'sa', '10.50.1.2', 8080, 1, 1, 50, 100, 0),
        ('us-node-1', 'myapp', 'us', '10.50.2.1', 8080, 1, 2, 50, 100, 0),
        ('eu-node-1', 'myapp', 'eu', '10.50.3.1', 8080, 1, 2, 50, 100, 0);
```

### Deployment

```yaml
# deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: edgeproxy
  namespace: edgeproxy
  labels:
    app: edgeproxy
spec:
  replicas: 3
  selector:
    matchLabels:
      app: edgeproxy
  template:
    metadata:
      labels:
        app: edgeproxy
    spec:
      initContainers:
        - name: init-db
          image: alpine:3.19
          command:
            - sh
            - -c
            - |
              apk add --no-cache sqlite
              sqlite3 /data/routing.db < /config/routing.sql
          volumeMounts:
            - name: data
              mountPath: /data
            - name: config
              mountPath: /config
      containers:
        - name: edgeproxy
          image: edgeproxy:latest
          ports:
            - containerPort: 8080
              protocol: TCP
          env:
            - name: EDGEPROXY_LISTEN_ADDR
              value: "0.0.0.0:8080"
            - name: EDGEPROXY_REGION
              valueFrom:
                fieldRef:
                  fieldPath: metadata.labels['topology.kubernetes.io/region']
            - name: EDGEPROXY_DB_PATH
              value: "/data/routing.db"
            - name: EDGEPROXY_BINDING_TTL_SECS
              value: "600"
          volumeMounts:
            - name: data
              mountPath: /data
            - name: geoip
              mountPath: /geoip
          resources:
            requests:
              cpu: 100m
              memory: 128Mi
            limits:
              cpu: 1000m
              memory: 512Mi
          livenessProbe:
            tcpSocket:
              port: 8080
            initialDelaySeconds: 5
            periodSeconds: 10
          readinessProbe:
            tcpSocket:
              port: 8080
            initialDelaySeconds: 5
            periodSeconds: 5
      volumes:
        - name: data
          emptyDir: {}
        - name: config
          configMap:
            name: edgeproxy-config
        - name: geoip
          secret:
            secretName: geoip-db
            optional: true
```

### Service

```yaml
# service.yaml
apiVersion: v1
kind: Service
metadata:
  name: edgeproxy
  namespace: edgeproxy
spec:
  type: LoadBalancer
  selector:
    app: edgeproxy
  ports:
    - port: 8080
      targetPort: 8080
      protocol: TCP
```

### Deploy

```bash
kubectl apply -f namespace.yaml
kubectl apply -f configmap.yaml
kubectl apply -f deployment.yaml
kubectl apply -f service.yaml
```

## Multi-Region Deployment

### Per-Region Deployments

```yaml
# deployment-sa.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: edgeproxy-sa
  namespace: edgeproxy
spec:
  replicas: 2
  selector:
    matchLabels:
      app: edgeproxy
      region: sa
  template:
    metadata:
      labels:
        app: edgeproxy
        region: sa
    spec:
      nodeSelector:
        topology.kubernetes.io/region: sa-east-1
      containers:
        - name: edgeproxy
          image: edgeproxy:latest
          env:
            - name: EDGEPROXY_REGION
              value: "sa"
          # ... rest of container spec
---
# deployment-us.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: edgeproxy-us
  namespace: edgeproxy
spec:
  replicas: 2
  selector:
    matchLabels:
      app: edgeproxy
      region: us
  template:
    metadata:
      labels:
        app: edgeproxy
        region: us
    spec:
      nodeSelector:
        topology.kubernetes.io/region: us-east-1
      containers:
        - name: edgeproxy
          image: edgeproxy:latest
          env:
            - name: EDGEPROXY_REGION
              value: "us"
---
# deployment-eu.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: edgeproxy-eu
  namespace: edgeproxy
spec:
  replicas: 2
  selector:
    matchLabels:
      app: edgeproxy
      region: eu
  template:
    metadata:
      labels:
        app: edgeproxy
        region: eu
    spec:
      nodeSelector:
        topology.kubernetes.io/region: eu-west-1
      containers:
        - name: edgeproxy
          image: edgeproxy:latest
          env:
            - name: EDGEPROXY_REGION
              value: "eu"
```

### Global Load Balancer

```yaml
# global-service.yaml
apiVersion: v1
kind: Service
metadata:
  name: edgeproxy-global
  namespace: edgeproxy
  annotations:
    # AWS Global Accelerator
    service.beta.kubernetes.io/aws-load-balancer-type: "nlb"
    # GCP NEG
    cloud.google.com/neg: '{"ingress": true}'
spec:
  type: LoadBalancer
  externalTrafficPolicy: Local  # Preserve client IP
  selector:
    app: edgeproxy
  ports:
    - port: 8080
      targetPort: 8080
```

## HorizontalPodAutoscaler

```yaml
# hpa.yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: edgeproxy-hpa
  namespace: edgeproxy
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: edgeproxy
  minReplicas: 2
  maxReplicas: 10
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
    - type: Resource
      resource:
        name: memory
        target:
          type: Utilization
          averageUtilization: 80
```

## PodDisruptionBudget

```yaml
# pdb.yaml
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: edgeproxy-pdb
  namespace: edgeproxy
spec:
  minAvailable: 1
  selector:
    matchLabels:
      app: edgeproxy
```

## Network Policies

```yaml
# network-policy.yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: edgeproxy-netpol
  namespace: edgeproxy
spec:
  podSelector:
    matchLabels:
      app: edgeproxy
  policyTypes:
    - Ingress
    - Egress
  ingress:
    - from:
        - ipBlock:
            cidr: 0.0.0.0/0
      ports:
        - protocol: TCP
          port: 8080
  egress:
    - to:
        - ipBlock:
            cidr: 10.50.0.0/16  # WireGuard overlay
      ports:
        - protocol: TCP
          port: 8080
```

## ServiceMonitor (Prometheus)

```yaml
# servicemonitor.yaml
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: edgeproxy
  namespace: edgeproxy
spec:
  selector:
    matchLabels:
      app: edgeproxy
  endpoints:
    - port: metrics
      interval: 30s
      path: /metrics
```

## Helm Chart (Basic)

```yaml
# Chart.yaml
apiVersion: v2
name: edgeproxy
description: Distributed TCP Proxy for Geo-Aware Load Balancing
version: 0.1.0
appVersion: "1.0.0"
```

```yaml
# values.yaml
replicaCount: 2

image:
  repository: edgeproxy
  tag: latest
  pullPolicy: IfNotPresent

region: sa

service:
  type: LoadBalancer
  port: 8080

resources:
  limits:
    cpu: 1000m
    memory: 512Mi
  requests:
    cpu: 100m
    memory: 128Mi

autoscaling:
  enabled: true
  minReplicas: 2
  maxReplicas: 10
  targetCPUUtilizationPercentage: 70

nodeSelector: {}

tolerations: []

affinity:
  podAntiAffinity:
    preferredDuringSchedulingIgnoredDuringExecution:
      - weight: 100
        podAffinityTerm:
          labelSelector:
            matchExpressions:
              - key: app
                operator: In
                values:
                  - edgeproxy
          topologyKey: kubernetes.io/hostname
```

### Install

```bash
helm install edgeproxy ./charts/edgeproxy \
  --namespace edgeproxy \
  --create-namespace \
  --set region=sa \
  --set replicaCount=3
```

## WireGuard Integration

For production, backends communicate over WireGuard overlay:

```yaml
# wireguard-daemonset.yaml
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: wireguard
  namespace: edgeproxy
spec:
  selector:
    matchLabels:
      app: wireguard
  template:
    metadata:
      labels:
        app: wireguard
    spec:
      hostNetwork: true
      containers:
        - name: wireguard
          image: linuxserver/wireguard:latest
          securityContext:
            capabilities:
              add:
                - NET_ADMIN
            privileged: true
          volumeMounts:
            - name: config
              mountPath: /config
      volumes:
        - name: config
          secret:
            secretName: wireguard-config
```

## Troubleshooting

### Check Pod Status

```bash
kubectl get pods -n edgeproxy
kubectl describe pod -n edgeproxy edgeproxy-xxx
kubectl logs -n edgeproxy edgeproxy-xxx -f
```

### Test Connectivity

```bash
# Port-forward for local testing
kubectl port-forward -n edgeproxy svc/edgeproxy 8080:8080

# Test connection
echo "test" | nc localhost 8080
```

### Debug Database

```bash
kubectl exec -n edgeproxy deploy/edgeproxy -- \
  sqlite3 /data/routing.db "SELECT * FROM backends"
```

## Next Steps

- [Configuration](../configuration) - Environment variables
- [Architecture](../architecture) - System design
- [Docker Deployment](./docker) - Container basics
