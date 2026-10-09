#!/bin/sh
# Checks an image of lattice works, without logging Claude Code in: what's
# in it runs, the server answers on the port it's published to, with the
# web app built in and mermaid served from the image, and git takes a token
# from the environment. CI runs it on each image it builds:
#
#   docker/smoke.sh lattice:dev

set -eu

image=${1:?say which image, like lattice:dev}
port=17347
name="lattice-smoke-$$"

fail() {
    printf 'smoke: %s\n' "$*" >&2
    docker logs "$name" >&2 2>&1 || true
    exit 1
}

run() {
    docker run --rm --entrypoint "" "$image" "$@"
}

run lattice --version
run claude --version
run git --version

said=$(printf 'protocol=https\nhost=git.example.com\n\n' |
    docker run --rm -i -e GIT_TOKEN=t0ken --entrypoint "" "$image" git credential fill)
case "$said" in
    *password=t0ken*) echo "ok: git takes the token" ;;
    *) fail "git didn't take the token: $said" ;;
esac

docker run -d --name "$name" -p "127.0.0.1:$port:7347" --cap-drop ALL \
    --security-opt no-new-privileges:true "$image" > /dev/null
trap 'docker rm -f "$name" > /dev/null 2>&1 || true' EXIT

answered=
for _ in $(seq 1 50); do
    if curl -fsS "http://127.0.0.1:$port/api/server" > /dev/null 2>&1; then
        answered=1
        break
    fi
    sleep 0.2
done
[ -n "$answered" ] || fail "the server didn't answer on port $port"
curl -fsS "http://127.0.0.1:$port/api/server" | grep -q '"lattice"' || fail "/api/server"
curl -fsS "http://127.0.0.1:$port/" | grep -q '/_app/' || fail "the web app isn't built in"
curl -fsS -o /dev/null "http://127.0.0.1:$port/assets/mermaid.min.js" || fail "mermaid isn't served"
curl -fsS "http://127.0.0.1:$port/api/repos" | grep -q '^\[' || fail "/api/repos"
# A page of this machine's, through the published port, may change things.
status=$(curl -sS -o /dev/null -w '%{http_code}' -X POST \
    -H "Origin: http://127.0.0.1:$port" -H 'Content-Type: application/json' \
    --data '{"source":"golang/go"}' "http://127.0.0.1:$port/api/repos")
[ "$status" = 200 ] || fail "adding a repository answered $status"
status=$(curl -sS -o /dev/null -w '%{http_code}' -X POST \
    -H 'Origin: https://evil.example' --data '{}' "http://127.0.0.1:$port/api/repos")
[ "$status" = 403 ] || fail "another site's page was answered $status"
docker exec "$name" lattice status | grep -q '^go  golang/go' || fail "lattice status"
echo "ok: $image"
