---
sidebar_position: 2
---

# Deploy com Kubernetes

Este guia cobre o deployment do edgeProxy no Kubernetes, incluindo manifests, Helm charts e configurações multi-cluster.

## Pré-requisitos

- Kubernetes 1.25+
- kubectl configurado
- Helm 3.x (opcional)

## Deployment Básico

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

## Deploy Multi-Região

### Deployments por Região

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
          # ... resto da spec do container
```

### Load Balancer Global

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
  externalTrafficPolicy: Local  # Preservar IP do cliente
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
            cidr: 10.50.0.0/16  # Overlay WireGuard
      ports:
        - protocol: TCP
          port: 8080
```

## Troubleshooting

### Verificar Status dos Pods

```bash
kubectl get pods -n edgeproxy
kubectl describe pod -n edgeproxy edgeproxy-xxx
kubectl logs -n edgeproxy edgeproxy-xxx -f
```

### Testar Conectividade

```bash
# Port-forward para teste local
kubectl port-forward -n edgeproxy svc/edgeproxy 8080:8080

# Testar conexão
echo "test" | nc localhost 8080
```

### Debug do Banco de Dados

```bash
kubectl exec -n edgeproxy deploy/edgeproxy -- \
  sqlite3 /data/routing.db "SELECT * FROM backends"
```

## Próximos Passos

- [Configuração](../configuration) - Variáveis de ambiente
- [Arquitetura](../architecture) - Design do sistema
- [Deploy com Docker](./docker) - Básicos de containers
