import { useEffect, useState } from 'react';

type Theme = 'light' | 'dark';
const storageKey = 'interview:workspace-theme';

export function useWorkspaceTheme() {
  const [theme, setTheme] = useState<Theme>(() => {
    try {
      const saved = localStorage.getItem(storageKey);
      if (saved === 'dark' || saved === 'light') return saved;
    } catch {
      /* The toggle still works when browser storage is unavailable. */
    }
    return window.matchMedia('(prefers-color-scheme: dark)').matches
      ? 'dark'
      : 'light';
  });
  useEffect(() => {
    try {
      localStorage.setItem(storageKey, theme);
    } catch {
      /* Keep the in-memory preference. */
    }
  }, [theme]);
  return {
    theme,
    toggleTheme: () =>
      setTheme((value) => (value === 'dark' ? 'light' : 'dark')),
  };
}
