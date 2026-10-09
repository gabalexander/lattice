// The theme: dark, light, or the system's, remembered in this browser. app.html sets it before the first paint
// from the same key, so a page never flashes the wrong one.

import { themeChanged } from './mermaid';

export type ThemePref = 'dark' | 'light' | 'system';
const THEME_KEY = 'lattice-theme';

function stored(): ThemePref {
  try {
    const pref = localStorage.getItem(THEME_KEY);
    return pref === 'dark' || pref === 'light' ? pref : 'system';
  } catch {
    return 'system';
  }
}

const darkQuery = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null;

class Theme {
  pref = $state<ThemePref>(stored());
  system = $state<'dark' | 'light'>(darkQuery && !darkQuery.matches ? 'light' : 'dark');
  resolved = $derived<'dark' | 'light'>(this.pref === 'system' ? this.system : this.pref);

  constructor() {
    darkQuery?.addEventListener('change', (e) => {
      this.system = e.matches ? 'dark' : 'light';
      this.apply();
    });
  }

  set(pref: ThemePref) {
    this.pref = pref;
    try {
      if (pref === 'system') localStorage.removeItem(THEME_KEY);
      else localStorage.setItem(THEME_KEY, pref);
    } catch {
      // A private window, or storage turned off: the choice lasts as long as the page.
    }
    this.apply();
  }

  /** The next one of dark, light and system, for a button that cycles through them. */
  cycle() {
    const order: ThemePref[] = ['dark', 'light', 'system'];
    this.set(order[(order.indexOf(this.pref) + 1) % order.length]);
  }

  apply() {
    const root = document.documentElement;
    root.dataset.theme = this.pref;
    root.dataset.resolvedTheme = this.resolved;
    themeChanged();
  }
}

export const theme = new Theme();
