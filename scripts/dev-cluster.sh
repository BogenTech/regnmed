#!/bin/sh
# Local production-like cluster: k3d (k3s) inside a small colima VM,
# sized for an 8 GB Mac (2 CPUs / 2 GB). One command each way:
#
#   scripts/dev-cluster.sh up       build images, start VM+cluster, deploy, seed
#   scripts/dev-cluster.sh stop     free ALL RAM (state survives; up resumes)
#   scripts/dev-cluster.sh down     delete the cluster (VM stays; up recreates)
#   scripts/dev-cluster.sh status   what is running, pods, URLs
#   scripts/dev-cluster.sh deploy   rebuild images + roll them out (after code changes)
#
# Debugging once it runs:
#   kubectl -n regnmed get pods
#   kubectl -n regnmed logs deploy/regnid -f          (or any deployment)
#   kubectl -n regnmed exec deploy/regnid -- /app/regnid verify-... etc.
set -e
cd "$(dirname "$0")/.."

CLUSTER=regnmed
VM_CPU=2
VM_MEM=2

urls() {
    echo
    echo "  Portal: http://regnmed.localhost      (what a human opens)"
    echo "  API:    http://api.regnmed.localhost  (same binary; /health, /me)"
    echo "  IdP:    http://id.regnmed.localhost   (login/admin UI, OIDC issuer)"
    echo "  (browsers resolve *.localhost; for curl add:"
    echo "   --resolve id.regnmed.localhost:80:127.0.0.1)"
    echo
    echo "  admin login: admin-demo@example.test / admin-demo-passord"
    echo "  stop everything (free RAM):  scripts/dev-cluster.sh stop"
}

colima_up() {
    if ! colima status >/dev/null 2>&1; then
        echo "==> starting colima VM (${VM_CPU} cpu / ${VM_MEM} GB)"
        colima start --cpu "$VM_CPU" --memory "$VM_MEM" --disk 20
    fi
}

cluster_up() {
    if ! k3d cluster list | grep -q "^$CLUSTER"; then
        echo "==> creating k3d cluster"
        k3d cluster create "$CLUSTER" --servers 1 --agents 0 \
            -p "80:80@loadbalancer" \
            --k3s-arg "--disable=metrics-server@server:0"
    elif ! kubectl get nodes >/dev/null 2>&1; then
        echo "==> starting k3d cluster"
        k3d cluster start "$CLUSTER"
    fi
}

# Roll out the freshly imported images.
#
# The images always carry the same `:dev` tag, so re-importing one does
# NOT change the Deployment spec — and with imagePullPolicy: IfNotPresent
# Kubernetes then has no reason to recreate the pod. Without an explicit
# restart, `up` on an existing cluster reports success while every pod
# keeps running the previous build: a deploy that silently did nothing.
# (Found the hard way — a four-day-old regnid pod "successfully rolled
# out" without the new migration.)
roll_apps() {
    echo "==> rolling out regnid, regnmed-api and the mail worker"
    kubectl -n regnmed rollout restart deploy/regnid deploy/regnmed-api \
        deploy/regnid-mail-worker
    kubectl -n regnmed rollout status deploy/regnid deploy/regnmed-api \
        deploy/regnid-mail-worker --timeout=180s
}

seed() {
    # Idempotent: duplicates just fail quietly.
    echo "==> seeding demo admin + OIDC client"
    kubectl -n regnmed exec deploy/regnid -- /app/regnid add-user \
        --email admin-demo@example.test --password admin-demo-passord \
        --name "Demo Admin" --admin >/dev/null 2>&1 || true
    # Both origins: the portal answers on regnmed.localhost (what a human
    # opens) and on api.regnmed.localhost (the API host serves the same
    # binary). The app builds its redirect_uri from location.origin, so
    # each origin it can be opened from must be registered here.
    kubectl -n regnmed exec deploy/regnid -- /app/regnid add-client \
        --client-id regnmed-portal --name "regnmed portal" \
        --redirect-uri http://regnmed.localhost/callback \
        --redirect-uri http://api.regnmed.localhost/callback \
        --post-logout-redirect-uri http://regnmed.localhost/ \
        --post-logout-redirect-uri http://api.regnmed.localhost/ \
        --audience regnmed >/dev/null 2>&1 || true
}

case "${1:-}" in
up)
    # The VM first: build-images.sh ends in `docker build`, and the
    # Docker daemon lives in colima. Building before starting it works
    # only when the VM happens to be running already, so a cold start
    # failed after paying for two release cross-compiles.
    colima_up
    ./scripts/build-images.sh
    cluster_up
    echo "==> importing images"
    k3d image import regnid:dev regnmed:dev -c "$CLUSTER"
    echo "==> applying manifests"
    kubectl apply -k deploy/local
    echo "==> restarting coredns to pick up host rewrites"
    kubectl -n kube-system rollout restart deploy/coredns >/dev/null
    echo "==> waiting for postgres and nats"
    kubectl -n regnmed rollout status deploy/postgres deploy/nats --timeout=180s
    roll_apps
    seed
    echo "==> cluster is up"
    urls
    ;;
deploy)
    colima_up
    ./scripts/build-images.sh
    k3d image import regnid:dev regnmed:dev -c "$CLUSTER"
    roll_apps
    ;;
stop)
    colima stop
    echo "VM stopped — all RAM freed. 'up' resumes where you left off."
    ;;
down)
    k3d cluster delete "$CLUSTER"
    echo "cluster deleted (colima VM still running; 'stop' to free RAM too)"
    ;;
status)
    colima status 2>&1 || true
    kubectl -n regnmed get pods 2>/dev/null || echo "cluster not running"
    urls
    ;;
*)
    echo "usage: $0 up|deploy|stop|down|status"
    exit 1
    ;;
esac
