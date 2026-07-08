/**
 * Theme management shared between the inline bootstrap script in `app.html`
 * (which prevents flash-of-wrong-theme on load) and `ThemeToggle.svelte`.
 *
 * Modes:
 *  - 'light' / 'dark': explicit user choice, persisted to localStorage.
 *  - 'system': follow the OS `prefers-color-scheme`, live-updating on change.
 *
 * Default: users who have never chosen a theme get 'system'. Once a user
 * picks 'light' or 'dark' explicitly, that choice is persisted and kept.
 */

export const THEME_STORAGE_KEY = 'temperpaw-theme';

export type Theme = 'light' | 'dark' | 'system';
export type ResolvedTheme = 'light' | 'dark';

const VALID_THEMES: Theme[] = ['light', 'dark', 'system'];

export function isTheme(value: string | null): value is Theme {
	return value !== null && (VALID_THEMES as string[]).includes(value);
}

/** Reads the persisted theme choice, defaulting to 'system' when unset/invalid. */
export function getStoredTheme(): Theme {
	if (typeof localStorage === 'undefined') return 'system';
	const stored = localStorage.getItem(THEME_STORAGE_KEY);
	return isTheme(stored) ? stored : 'system';
}

export function prefersDark(): boolean {
	return typeof matchMedia !== 'undefined' && matchMedia('(prefers-color-scheme: dark)').matches;
}

/** Resolves a theme mode to the concrete 'light' | 'dark' value that should be applied. */
export function resolveTheme(theme: Theme): ResolvedTheme {
	if (theme === 'system') return prefersDark() ? 'dark' : 'light';
	return theme;
}

export function applyTheme(theme: Theme): void {
	if (typeof document === 'undefined') return;
	document.documentElement.setAttribute('data-theme', resolveTheme(theme));
}

/** Persists the mode (including 'system') and applies it immediately. */
export function setTheme(theme: Theme): void {
	if (typeof localStorage !== 'undefined') {
		localStorage.setItem(THEME_STORAGE_KEY, theme);
	}
	applyTheme(theme);
}
