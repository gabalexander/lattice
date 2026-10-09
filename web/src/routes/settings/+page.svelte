<!-- Settings: what a wiki is written with (the model, how many subsections at once, what a build may spend,
     the files left out), where a click on its code opens it, and what the chat answers with. lattice keeps them in its config file
     (~/.config/lattice/config.toml); this page reads and writes them through the API. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import AppHeader from '$lib/components/AppHeader.svelte';
  import { OPEN_IN } from '$lib/codelinks';
  import { toast } from '$lib/toast.svelte';
  import type { Settings } from '$lib/types';

  let saved = $state<Settings | null>(null);
  let form = $state<Settings | null>(null);
  let exclude = $state('');
  let loadError = $state('');
  let saveError = $state('');
  let saving = $state(false);

  const draft = $derived(form ? { ...form, exclude: exclude.split('\n').map((l) => l.trim()).filter(Boolean) } : null);
  const changed = $derived(!!draft && !!saved && JSON.stringify(draft) !== JSON.stringify(saved));

  function take(settings: Settings) {
    saved = settings;
    form = { ...settings };
    exclude = settings.exclude.join('\n');
  }

  onMount(() => {
    api.settings().then(take, (err) => (loadError = (err as Error).message));
  });

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!draft) return;
    saving = true;
    saveError = '';
    try {
      take(await api.saveSettings(draft));
      toast.show('Settings saved');
    } catch (err) {
      saveError = (err as Error).message;
    } finally {
      saving = false;
    }
  }

  const MODELS: [string, string, string][] = [
    ['sonnet', 'Sonnet', 'Quick and thorough; the usual choice.'],
    ['opus', 'Opus', 'Deeper reading of hard code; slower and dearer.'],
  ];
  /** The choices, with a model the config file names by its full name kept as one of them. */
  const choices = (current: string | undefined): [string, string, string][] =>
    current && !MODELS.some(([value]) => value === current) ? [...MODELS, [current, current, 'As the config file names it.']] : MODELS;
</script>

<svelte:head><title>Settings · lattice</title></svelte:head>

<AppHeader />

<main class="settings">
  <h1>Settings</h1>
  <p class="lede">What lattice writes wikis with, and what its chat answers with. Every wiki is written by Claude Code on this machine, with your subscription.</p>

  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {:else if form}
    <form onsubmit={save}>
      <section class="panel" aria-labelledby="writing">
        <h2 id="writing">Writing a wiki</h2>
        <fieldset class="field">
          <legend>Model</legend>
          <div class="choices">
            {#each choices(saved?.model) as [value, label, hint] (value)}
              <label class="choice" class:on={form.model === value}>
                <input type="radio" name="model" {value} bind:group={form.model} />
                <span class="name">{label}</span><span class="hint">{hint}</span>
              </label>
            {/each}
          </div>
        </fieldset>
        <div class="field">
          <label for="concurrency">Subsections written at once</label>
          <div class="range">
            <input id="concurrency" type="range" min="1" max="16" step="1" bind:value={form.concurrency} aria-describedby="concurrency-hint" />
            <output for="concurrency">{form.concurrency}</output>
          </div>
          <p class="hint" id="concurrency-hint">More is faster, and spends your plan's limits faster.</p>
        </div>
        <div class="field">
          <label for="budget">Most a build may spend</label>
          <div class="money"><span aria-hidden="true">$</span><input id="budget" type="number" min="0" step="1" bind:value={form.budget_usd} aria-describedby="budget-hint" /></div>
          <p class="hint" id="budget-hint">In dollars, as Claude Code counts it; 0 is no limit. A build stops when it gets there, and Resume carries on.</p>
        </div>
        <div class="field">
          <label for="exclude">Left out</label>
          <textarea id="exclude" rows="5" bind:value={exclude} spellcheck="false" placeholder={'vendor/**\n*.min.js'} aria-describedby="exclude-hint"></textarea>
          <p class="hint" id="exclude-hint">A glob a line, beyond what .gitignore leaves out already.</p>
        </div>
      </section>

      <section class="panel" aria-labelledby="reading">
        <h2 id="reading">Reading a wiki</h2>
        <div class="field">
          <label for="open-in">Open code in</label>
          <select id="open-in" bind:value={form.open_code_in} aria-describedby="open-in-hint">
            {#each OPEN_IN as { group, places } (group)}
              <optgroup label={group}>
                {#each places as place (place.value)}<option value={place.value}>{place.name}</option>{/each}
              </optgroup>
            {/each}
          </select>
          <p class="hint" id="open-in-hint">
            Where a click on a name in the code opens its file, at its line; ⌘ or Ctrl-click opens it on the forge. VS Code, Cursor, Zed and the JetBrains IDEs open on the machine your browser is on, from where the code is on lattice's; the JetBrains IDEs need the Toolbox App, and the project open or opened lately.
          </p>
        </div>
      </section>

      <section class="panel" aria-labelledby="chat">
        <h2 id="chat">The chat</h2>
        <fieldset class="field">
          <legend>Model</legend>
          <div class="choices">
            {#each choices(saved?.ask_model) as [value, label] (value)}
              <label class="choice small" class:on={form.ask_model === value}>
                <input type="radio" name="ask_model" {value} bind:group={form.ask_model} />
                <span class="name">{label}</span>
              </label>
            {/each}
          </div>
        </fieldset>
        <div class="field">
          <label for="ask-budget">Most a question may spend</label>
          <div class="money"><span aria-hidden="true">$</span><input id="ask-budget" type="number" min="0" step="0.05" bind:value={form.ask_budget_usd} /></div>
        </div>
      </section>

      <div class="bar">
        {#if saveError}<p class="error" role="alert">{saveError}</p>{/if}
        <button class="pill" type="button" disabled={!changed || saving} onclick={() => saved && take(saved)}>Undo changes</button>
        <button class="pill primary" type="submit" disabled={!changed || saving}>{saving ? 'Saving' : 'Save'}</button>
      </div>
    </form>
  {:else}
    <div class="spinner" aria-label="Loading"></div>
  {/if}
</main>

<style>
  .settings {
    max-width: 760px;
    margin: 0 auto;
    padding: 12px 25px 80px;
  }
  h1 {
    font: 400 32px/40px var(--font-display);
    margin: 0 0 6px;
  }
  .lede {
    color: var(--text-2);
    margin: 0 0 24px;
    line-height: 24px;
  }
  .panel {
    background: var(--panel);
    border: 1px solid var(--panel-line);
    border-radius: 16px;
    padding: 22px 25px 8px;
    margin-bottom: 14px;
  }
  h2 {
    font: 500 18px/26px var(--font-display);
    margin: 0 0 14px;
  }
  .field {
    border: 0;
    margin: 0 0 22px;
    padding: 0;
    min-width: 0;
  }
  .field > label,
  legend {
    display: block;
    font: 500 14px/20px var(--font-display);
    margin: 0 0 8px;
    padding: 0;
  }
  .hint {
    margin: 6px 0 0;
    font-size: 13px;
    line-height: 19px;
    color: var(--text-3);
  }
  .choices {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 10px;
  }
  .choice {
    position: relative;
    display: grid;
    gap: 2px;
    padding: 12px 14px;
    border-radius: 12px;
    border: 1px solid var(--button-line);
    cursor: pointer;
  }
  .choice.small {
    padding: 9px 14px;
  }
  .choice.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .choice:has(input:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .choice input {
    position: absolute;
    opacity: 0;
    inset: 0;
    margin: 0;
    cursor: pointer;
  }
  .choice .name {
    font: 500 15px/22px var(--font-display);
  }
  .choice .hint {
    margin: 0;
  }
  .range {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .range input {
    flex: 1;
    accent-color: var(--accent);
  }
  output {
    min-width: 2ch;
    font: 500 18px/1 var(--font-display);
    font-variant-numeric: tabular-nums;
  }
  .money {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 44px;
    padding: 0 14px;
    border-radius: 10px;
    border: 1px solid var(--input-line);
    background: var(--bg);
    color: var(--text-3);
  }
  select {
    height: 44px;
    min-width: min(100%, 320px);
    padding: 0 12px;
    border-radius: 10px;
    border: 1px solid var(--input-line);
    background: var(--bg);
    color: var(--text);
    outline: none;
  }
  .money:focus-within,
  select:focus,
  textarea:focus {
    border-color: var(--accent);
  }
  .money input {
    width: 110px;
    border: 0;
    background: transparent;
    outline: none;
    color: var(--text);
    font-size: 15px;
  }
  textarea {
    width: 100%;
    padding: 10px 14px;
    border-radius: 10px;
    border: 1px solid var(--input-line);
    background: var(--bg);
    font: 400 13px/21px var(--font-code);
    resize: vertical;
    outline: none;
  }
  .bar {
    position: sticky;
    bottom: 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 10px;
    padding: 14px 0;
    background: var(--bg);
  }
  .error {
    color: var(--error);
    margin: 0 auto 0 0;
  }
  @media (max-width: 839px) {
    .settings {
      padding: 4px 12px 56px;
    }
    .panel {
      padding: 18px 16px 4px;
    }
  }
</style>
