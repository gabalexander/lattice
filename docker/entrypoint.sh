#!/bin/sh
# What lattice's container does before it runs its command: ssh set up to
# reach private repositories, with a key mounted at /run/secrets/git_ssh_key
# when there's one (or an agent's socket in SSH_AUTH_SOCK, which ssh finds
# by itself), and the hosts it has met kept in /data, so a host's key is
# trusted the first time and checked every time after.

set -eu

mkdir -p "$HOME/.ssh" /data/ssh
chmod 700 "$HOME/.ssh"
config="$HOME/.ssh/config"
{
    echo "Host *"
    echo "  UserKnownHostsFile /data/ssh/known_hosts"
    echo "  StrictHostKeyChecking accept-new"
} > "$config"
if [ -r /run/secrets/git_ssh_key ]; then
    # ssh takes a key only when nobody else may read it.
    cp /run/secrets/git_ssh_key "$HOME/.ssh/id_lattice"
    chmod 600 "$HOME/.ssh/id_lattice"
    echo "  IdentityFile $HOME/.ssh/id_lattice" >> "$config"
elif [ -e /run/secrets/git_ssh_key ]; then
    echo "lattice: /run/secrets/git_ssh_key isn't readable by the container's user (uid $(id -u)): see docs/docker.md" >&2
fi
chmod 600 "$config"

exec "$@"
