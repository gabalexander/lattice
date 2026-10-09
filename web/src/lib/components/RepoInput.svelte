<!-- The box a wiki starts from: a folder on this machine, a git URL, or GitHub's owner/repo, the model to
     write it with, and Generate. After deepwiki-by-cc's RepoInput (MIT; see THIRD_PARTY_NOTICES.md), with
     Code Wiki's rounded search box for its look. -->
<script lang="ts">
  import { describeSource } from '$lib/format';
  import type { Model } from '$lib/types';
  import Icon from './Icon.svelte';

  let {
    model = $bindable<Model>('sonnet'),
    busy = false,
    error = '',
    onsubmit,
  }: { model?: Model; busy?: boolean; error?: string; onsubmit: (source: string, model: Model) => void } = $props();

  let source = $state('');
  const kind = $derived(describeSource(source));

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (source.trim() && !busy) onsubmit(source.trim(), model);
  }
</script>

<form class="repo-input" onsubmit={submit} aria-label="Make a wiki">
  <div class="box">
    <Icon name="search" class="lead" />
    <input
      type="text"
      bind:value={source}
      placeholder="A folder, a git URL, or owner/repo"
      aria-label="Repository: a folder on this machine, a git URL, or GitHub's owner/repo"
      aria-describedby="repo-input-hint"
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      disabled={busy}
    />
    <div class="models" role="radiogroup" aria-label="Model">
      {#each [['sonnet', 'Sonnet'], ['opus', 'Opus']] as [value, label] (value)}
        <label class:on={model === value}>
          <input type="radio" name="model" {value} bind:group={model} disabled={busy} />{label}
        </label>
      {/each}
    </div>
    <button class="go" type="submit" disabled={busy || !source.trim()}>
      {#if busy}<span class="spinner small"></span>Starting{:else}Generate{/if}
    </button>
  </div>
  <p class="hint" id="repo-input-hint">
    {#if error}
      <span class="error" role="alert">{error}</span>
    {:else if kind}
      {kind}
    {:else}
      <code>~/src/app</code>, <code>git@git.example.com:team/app.git</code>, or <code>golang/go</code>
    {/if}
  </p>
</form>

<style>
  .repo-input {
    width: 100%;
    max-width: 760px;
    margin: 0 auto;
  }
  .box {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 64px;
    padding: 0 8px 0 24px;
    border: 1px solid var(--input-line);
    border-radius: 32px;
    background: var(--bg);
    box-shadow: inset -24px -10px 22px -26px rgba(138, 180, 248, 0.55);
    transition: border-color 0.15s;
  }
  .box:focus-within {
    border-color: var(--accent);
  }
  .box :global(.lead) {
    color: var(--text-3);
  }
  input[type='text'] {
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
    border: 0;
    background: transparent;
    outline: none;
    font-size: 18px;
    color: var(--text);
  }
  input[type='text']::placeholder {
    color: var(--text-3);
  }
  .models {
    display: flex;
    padding: 3px;
    border-radius: 20px;
    background: var(--panel);
    border: 1px solid var(--panel-line);
    flex: none;
  }
  .models label {
    position: relative;
    padding: 0 12px;
    height: 32px;
    display: inline-flex;
    align-items: center;
    border-radius: 16px;
    font: 500 13px/1 var(--font-display);
    color: var(--text-3);
    cursor: pointer;
  }
  .models label.on {
    background: color-mix(in srgb, var(--text) 12%, var(--panel));
    color: var(--text);
  }
  .models input {
    position: absolute;
    opacity: 0;
    inset: 0;
    margin: 0;
    cursor: pointer;
  }
  .models label:has(input:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .go {
    height: 48px;
    padding: 0 22px;
    border-radius: 24px;
    border: 0;
    background: var(--accent);
    color: var(--accent-ink);
    font: 500 15px/1 var(--font-display);
    display: inline-flex;
    align-items: center;
    gap: 8px;
    flex: none;
  }
  .go:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 88%, var(--text));
  }
  .go:disabled {
    background: var(--hover);
    color: var(--text-3);
  }
  .hint {
    margin: 10px 24px 0;
    font-size: 13px;
    line-height: 20px;
    color: var(--text-3);
    min-height: 20px;
  }
  .hint code {
    font-size: 12px;
    color: var(--text-2);
  }
  .error {
    color: var(--error);
  }
  @media (max-width: 639px) {
    .box {
      flex-wrap: wrap;
      height: auto;
      padding: 8px 8px 8px 18px;
      border-radius: 24px;
      row-gap: 8px;
    }
    .box :global(.lead) {
      display: none;
    }
    input[type='text'] {
      flex-basis: 100%;
      height: 44px;
      font-size: 16px;
    }
    .models {
      margin-right: auto;
    }
    .hint {
      margin: 8px 12px 0;
    }
  }
</style>
