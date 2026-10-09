<!-- What the wiki's page does and its keys, behind the header's help button. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '$lib/components/Icon.svelte';
  import type { Mode } from '$lib/codelinks';

  let { made, mode, onclose }: { made: string; mode: Mode; onclose: () => void } = $props();
  let dialog: HTMLDialogElement;
  const mod = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘' : 'Ctrl';

  onMount(() => dialog.showModal());

  function backdrop(event: MouseEvent) {
    if (event.target !== dialog) return;
    const r = dialog.getBoundingClientRect();
    if (event.clientX < r.left || event.clientX > r.right || event.clientY < r.top || event.clientY > r.bottom) dialog.close();
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<dialog class="help" aria-labelledby="lw-help-title" bind:this={dialog} onclose={onclose} onclick={backdrop}>
  <button class="round close" type="button" aria-label="Close" title="Close (Esc)" onclick={() => dialog.close()}><Icon name="close" size={26} /></button>
  <h2 id="lw-help-title">About this wiki</h2>
  <p>{made}</p>
  <h3>Reading it</h3>
  <ul>
    <li>The outline on the left follows you down the page; click an entry to go there.</li>
    {#if mode === 'serve'}
      <li>Click a name in <code>code</code> to open the file at that line in your editor; <kbd>{mod}</kbd>-click opens it on the forge, at the commit the page was written from.</li>
    {:else}
      <li>Click a name in <code>code</code> to see the file at that line on the forge, at the commit the page was written from.</li>
    {/if}
    <li>The magnifier on a diagram opens it large: drag to move it, <kbd>{mod}</kbd> and scroll (or pinch) to zoom.</li>
    <li>Share copies a link to the section you're reading; the link beside each heading copies its own.</li>
    <li>Chat answers questions about the code, reading it as it goes, with the section in view as context.</li>
  </ul>
  <h3>Keys</h3>
  <dl class="keys">
    <div><dt><kbd>/</kbd></dt><dd>Find a section or a wiki</dd></div>
    <div><dt><kbd>c</kbd></dt><dd>Show or hide the chat</dd></div>
    <div><dt><kbd>j</kbd> <kbd>k</kbd></dt><dd>Next or previous section</dd></div>
    <div><dt><kbd>Esc</kbd></dt><dd>Close what's open</dd></div>
  </dl>
  {#if mode === 'serve'}
    <h3>Keeping it fresh</h3>
    <p>Sync rewrites what the code has changed since this version; every build is kept, and the version menu switches between them.</p>
  {/if}
</dialog>
