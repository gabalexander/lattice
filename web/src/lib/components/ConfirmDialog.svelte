<!-- A question that needs a yes before something is done, in a modal dialog: Esc or Cancel says no. -->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';

  let {
    title,
    confirm,
    danger = false,
    onconfirm,
    oncancel,
    children,
  }: { title: string; confirm: string; danger?: boolean; onconfirm: () => void; oncancel: () => void; children: Snippet } = $props();

  let dialog: HTMLDialogElement;

  onMount(() => dialog.showModal());
</script>

<dialog bind:this={dialog} class="confirm" aria-labelledby="confirm-title" onclose={oncancel}>
  <h2 id="confirm-title">{title}</h2>
  <p>{@render children()}</p>
  <div class="row">
    <button class="pill small" type="button" onclick={() => dialog.close()}>Cancel</button>
    <button class="pill small {danger ? 'danger' : 'primary'}" type="button" onclick={onconfirm}>{confirm}</button>
  </div>
</dialog>

<style>
  .confirm {
    width: min(440px, calc(100vw - 32px));
    border: 1px solid var(--panel-line);
    border-radius: 16px;
    background: var(--panel);
    padding: 22px 24px 18px;
    box-shadow: var(--shadow);
  }
  h2 {
    font: 500 20px/28px var(--font-display);
    margin: 0 0 8px;
  }
  p {
    margin: 0 0 20px;
    color: var(--text-2);
    line-height: 22px;
  }
  .row {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
