<!-- The header of the app's own pages (the wikis, a job, the settings): the mark leading home, then where else
     to go and the theme. A wiki's page has Code Wiki's header instead (wiki/WikiHeader.svelte). -->
<script lang="ts">
  import { page } from '$app/state';
  import Icon from './Icon.svelte';
  import Logo from './Logo.svelte';
  import ThemeToggle from './ThemeToggle.svelte';
</script>

<header class="app-header">
  <Logo />
  <nav aria-label="lattice">
    <a class="pill small nav" href="/" aria-current={page.url.pathname === '/' ? 'page' : undefined}>Wikis</a>
    <a class="pill small nav" href="/settings" aria-current={page.url.pathname === '/settings' ? 'page' : undefined}>
      <Icon name="settings" size={18} />Settings
    </a>
    <ThemeToggle />
  </nav>
</header>

<style>
  .app-header {
    position: sticky;
    top: 0;
    z-index: 20;
    height: 84px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 0 25px;
    background: linear-gradient(to bottom, var(--bg) 0, var(--bg) 75%, color-mix(in srgb, var(--bg) 0%, transparent) 100%);
  }
  nav {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .nav {
    border-color: transparent;
    padding: 0 14px;
  }
  .nav[aria-current='page'] {
    color: var(--text);
    background: var(--hover);
  }
  @media (max-width: 839px) {
    .app-header {
      height: 64px;
      padding: 0 12px;
    }
    .nav {
      padding: 0 10px;
    }
  }
</style>
