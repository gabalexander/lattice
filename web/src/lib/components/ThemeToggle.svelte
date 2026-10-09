<!-- The theme button, as Code Wiki's: its icon says the theme in force, and its menu picks dark, light or the
     system's. -->
<script lang="ts">
  import { theme, type ThemePref } from '$lib/theme.svelte';
  import Icon, { type IconName } from './Icon.svelte';

  const CHOICES: { pref: ThemePref; label: string; icon: IconName }[] = [
    { pref: 'dark', label: 'Dark', icon: 'moon' },
    { pref: 'light', label: 'Light', icon: 'sun' },
    { pref: 'system', label: 'System', icon: 'contrast' },
  ];

  let open = $state(false);
  let button: HTMLButtonElement;
  let menu: HTMLDivElement | undefined = $state();
  const icon = $derived(CHOICES.find((c) => c.pref === theme.pref)?.icon ?? 'contrast');

  function toggle() {
    open = !open;
    if (open) requestAnimationFrame(() => menu?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus());
  }

  function choose(pref: ThemePref) {
    theme.set(pref);
    open = false;
    button.focus();
  }

  function onKey(event: KeyboardEvent) {
    const items = [...(menu?.querySelectorAll<HTMLElement>('[role="menuitemradio"]') ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      items[(at + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length]?.focus();
    } else if (event.key === 'Escape') {
      event.stopPropagation();
      open = false;
      button.focus();
    } else if (event.key === 'Tab') {
      open = false;
    }
  }
</script>

<svelte:window onclick={(e) => open && !(e.target as Element).closest('.theme-wrap') && (open = false)} />

<div class="menu-wrap theme-wrap">
  <button bind:this={button} class="round" type="button" aria-label="Theme: {theme.pref}" aria-haspopup="menu" aria-expanded={open} title="Theme: {theme.pref}" onclick={toggle}>
    <Icon name={icon} />
  </button>
  {#if open}
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div class="menu" role="menu" aria-label="Theme" bind:this={menu} onkeydown={onKey}>
      {#each CHOICES as choice (choice.pref)}
        <button type="button" role="menuitemradio" aria-checked={theme.pref === choice.pref} tabindex="-1" onclick={() => choose(choice.pref)}>
          <Icon name={choice.icon} size={20} />{choice.label}
        </button>
      {/each}
    </div>
  {/if}
</div>
