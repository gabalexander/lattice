// Sets the theme before the page paints, from the choice the app keeps (src/lib/theme.svelte.ts), so a page
// never flashes the other one.
(function () {
  var pref = 'system';
  try {
    pref = localStorage.getItem('lattice-theme') || 'system';
  } catch (e) {}
  if (pref !== 'dark' && pref !== 'light') pref = 'system';
  var dark = window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
  var root = document.documentElement;
  root.dataset.theme = pref;
  root.dataset.resolvedTheme = pref === 'system' ? (dark ? 'dark' : 'light') : pref;
})();
