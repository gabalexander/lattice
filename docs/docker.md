# lattice in Docker

The image holds lattice, git and Claude Code, and nothing else: Alpine, git, ssh and ripgrep (which Claude Code's
Grep runs on Alpine), lattice with its web app built in, Claude Code's native build, and mermaid. Claude Code runs
as it does on your machine, with your Claude subscription, logged in once inside the container.

## Start it

```sh
git clone https://github.com/gabalexander/lattice.git && cd lattice
docker compose up -d
docker compose exec lattice claude auth login      # once: follow the link it prints
docker compose exec lattice lattice doctor         # checks it all, Claude reading a file
```

Then open http://127.0.0.1:7347. The compose file publishes the port to this machine alone, `127.0.0.1:7347`. Keep
it that way: whoever reaches the port can use lattice, and spend from its Claude login.

Without compose:

```sh
docker build -t lattice .
docker run -d --name lattice -p 127.0.0.1:7347:7347 \
  -v lattice-data:/data -v lattice-claude:/home/lattice/.claude lattice
docker exec -it lattice claude auth login
```

## What's kept where

| Volume | What it holds |
| --- | --- |
| `/data` | lattice's settings (`/data/config/lattice/config.toml`), its database, the repositories it cloned and every version of their wikis, and the SSH hosts it has met (`/data/ssh/known_hosts`) |
| `/home/lattice/.claude` | Claude Code's login and settings (`CLAUDE_CONFIG_DIR`): log in once, and it stays across restarts and new images |

The image never updates Claude Code itself. A newer one comes with the image built again:
`docker compose build --pull`. `--build-arg CLAUDE_CODE=2.1.295` pins a version, and `stable`, the default, or
`latest` takes the newest of either.

## Your repositories

A repository on a git server is cloned into `/data` on its first build. On this machine, mount the directory your
code is in, read-only, and add a repository by its path in the container:

```yaml
    volumes:
      - ~/code:/repos:ro          # then add /repos/app on the home page
```

### Private repositories

Any git server, over HTTPS, with a token: set these in the environment, or in a `.env` file beside the compose
file, which compose reads.

| Variable | What it is |
| --- | --- |
| `GIT_TOKEN` | The token, which git is given as the password. It's given over HTTPS only. |
| `GIT_USERNAME` | The user it's given with: `x-access-token` unless set, which GitHub takes. GitLab takes `oauth2`, Bitbucket `x-token-auth`, and Gitea and Forgejo any name. |
| `GIT_TOKEN_HOST` | The one host the token is given to, like `git.example.com`. Without it, every host gets it. |

A token that can only read the repositories lattice builds is all it needs.

Over SSH, with your SSH agent, so no key is copied anywhere: mount its socket and say where it is. On Linux:

```yaml
    volumes:
      - ${SSH_AUTH_SOCK}:/run/ssh-agent.sock
    environment:
      SSH_AUTH_SOCK: /run/ssh-agent.sock
```

With Docker Desktop on a Mac, mount `/run/host-services/ssh-auth.sock` in its place. Or give the container a key of
its own, a deploy key say, as the secret `git_ssh_key`: uncomment the `secrets` in the compose file and put the key
in `./git_ssh_key`. It must be readable by the container's user, uid 1000; Docker Desktop's mounts are, and on
Linux, `chown 1000` the file. A host's SSH key is trusted the first time lattice meets it, and checked every time
after. To pin it from the start, put the host's line in `/data/ssh/known_hosts`.

## What it can and can't do

- Claude Code runs as lattice always runs it (see [claude.md](claude.md)): `--restricted`, reading files alone,
  confined to the repository, with no command and no web. The container adds its own walls: an unprivileged user,
  every capability dropped, and no way to gain privileges.
- Opening a file in your editor from the page needs an editor on the machine lattice runs on, which the container
  doesn't have. Its code links open the forge's page instead, where the repository has one.
- `lattice open` inside the container has no browser to open: open http://127.0.0.1:7347 on your machine.
- The other commands run inside it as they do anywhere: `docker compose exec lattice lattice status`, or `lattice
  export <repo> /data/site` and `docker compose cp lattice:/data/site ./site`.
