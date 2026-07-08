<script lang="ts">
  import { getStoredTheme, applyTheme, setTheme, type Theme } from '$lib/theme';

  let mode = $state<Theme>('system');

  const ORDER: Theme[] = ['light', 'dark', 'system'];
  const LABEL: Record<Theme, string> = { light: 'Light', dark: 'Dark', system: 'System' };

  function cycle() {
    const next = ORDER[(ORDER.indexOf(mode) + 1) % ORDER.length];
    mode = next;
    setTheme(next);
  }

  $effect(() => {
    mode = getStoredTheme();
    applyTheme(mode);

    const mql = matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => {
      // Only the resolved (light/dark) theme changes; the stored mode stays 'system'.
      if (mode === 'system') applyTheme('system');
    };
    mql.addEventListener('change', onChange);
    return () => mql.removeEventListener('change', onChange);
  });
</script>

<button
  onclick={cycle}
  aria-label={`Theme: ${LABEL[mode]}. Click to switch.`}
  class="toggle"
  title={`Theme: ${LABEL[mode]} (click to cycle)`}
>
  <!-- The icon shows the CURRENT mode (sun = light, moon = dark, monitor =
       system), not the mode a click switches to; the title spells out the
       cycle for anyone unsure. -->
  {#if mode === 'system'}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="2" y="4" width="20" height="13" rx="1.5"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/></svg>
  {:else if mode === 'light'}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></svg>
  {:else}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>
  {/if}
  <span class="dot" class:active={mode === 'system'}></span>
</button>

<style>
  .toggle {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px; height: 28px;
    border-radius: var(--radius, 4px);
    color: var(--text-3, #666);
  }

  .toggle:hover { color: var(--text-1, #eee); }

  .dot {
    position: absolute;
    bottom: 2px;
    right: 2px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: transparent;
  }

  .dot.active {
    background: var(--accent, #34d399);
  }
</style>
