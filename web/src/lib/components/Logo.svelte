<!-- lattice's mark, a diamond split into four, and its name: a link home, or just the mark where there's no
     home to go to, as in an exported wiki. -->
<script lang="ts">
  let { href = '/', compact = false }: { href?: string | null; compact?: boolean } = $props();
</script>

<svelte:element this={href === null ? 'span' : 'a'} class="logo" href={href ?? undefined} aria-label={href === null ? 'lattice' : 'lattice: every wiki'} role={href === null ? 'img' : undefined}>
  <svg class="mark" viewBox="0 0 32 32" aria-hidden="true">
    <path class="a" d="M16 3.6 21.8 9.4 16 15.2 10.2 9.4z" />
    <path class="b" d="M22.6 10.2 28.4 16l-5.8 5.8-5.8-5.8z" />
    <path class="c" d="M16 16.8 21.8 22.6 16 28.4l-5.8-5.8z" />
    <path class="d" d="M9.4 10.2 15.2 16l-5.8 5.8L3.6 16z" />
  </svg>
  {#if !compact}<span class="name">lattice</span>{/if}
</svelte:element>

<style>
  .logo {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    color: var(--text);
    text-decoration: none;
    white-space: nowrap;
    border-radius: 8px;
  }
  .mark {
    width: 32px;
    height: 32px;
    flex: none;
  }
  .a { fill: #f2b48f; }
  .b { fill: #d97757; }
  .c { fill: #b9583a; }
  .d { fill: #e8a17a; }
  .name {
    font: 500 24px/1 var(--font-display);
    letter-spacing: -0.01em;
  }
  @media (max-width: 839px) {
    .mark { width: 26px; height: 26px; }
    .name { font-size: 20px; }
  }
</style>
