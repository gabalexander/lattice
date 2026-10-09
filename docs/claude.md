# How lattice runs Claude Code

Everything lattice asks of a model, a build's plan, each of its pages, a question in the chat, is one run of
Claude Code, `claude -p`, on your machine, with your login. lattice never calls a model's API itself and needs no
other provider or service.

## What a run may do

Each run is locked down:

- `--restricted`: no tool that runs a command or fetches from the web, the file tools confined to the
  repository it runs in, and none of your, the project's or anyone's settings files read, so neither their
  permissions nor their hooks reach it.
- `--tools Read,Grep,Glob` (or no tool at all, when it only answers from what it's told), the rest refused
  besides with `--disallowedTools`, and `--permission-mode dontAsk`, which refuses whatever wasn't allowed
  rather than asking.
- `--strict-mcp-config`: no MCP server. `--settings '{"disableAllHooks":true}'`: no hook.
- `--max-budget-usd`, from the settings (see [configuration.md](configuration.md)), and a turn cap and a time
  limit where lattice sets them. A run that's cancelled or takes too long is stopped, with whatever it
  started.
- `--no-session-persistence`, but for the chat, which follows up in its conversation.
- It isn't part of whichever Claude Code or crystal session started lattice: their session variables are
  taken out of its environment.

The message goes on its standard input, and what Claude does comes back as it does it, with
`--output-format stream-json`: the text as it's written, each file it reads, and at the end what it answered
and what it cost.

A run that fails for a reason that passes, Claude overloaded or rate limited, or its process dying, can be
tried again, waiting longer each time; one that failed for any other reason, like a flag this Claude Code
doesn't know, isn't.

## Checking it works

```sh
lattice doctor
```

checks the settings, the data directory, git and Claude Code, then asks Claude to read a file holding a word
made up for the check, without telling it the word: only reading the file gives it back. It's the run a build
makes, with the same flags, so a Claude Code too old for them, or not logged in, is found before a build
spends anything. It costs a cent or two; `--model` checks another model than the settings'.
