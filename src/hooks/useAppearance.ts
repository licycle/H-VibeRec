import { useCallback, useEffect, useState } from 'react';

export type AppFontSize = 'small' | 'standard' | 'large';

const SETTINGS_STORAGE_KEY = 'recorder-settings';
const DEFAULT_FONT_SIZE: AppFontSize = 'small';
const FONT_SIZE_VALUES: Record<AppFontSize, string> = {
  small: '15px',
  standard: '16px',
  large: '17px',
};

function isAppFontSize(value: unknown): value is AppFontSize {
  return value === 'small' || value === 'standard' || value === 'large';
}

function readStoredSettings(): Record<string, unknown> {
  const savedSettings = localStorage.getItem(SETTINGS_STORAGE_KEY);
  if (!savedSettings) return {};

  const parsed = JSON.parse(savedSettings);
  return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : {};
}

function saveStoredSettings(patch: Record<string, unknown>) {
  try {
    localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify({
      ...readStoredSettings(),
      ...patch,
    }));
  } catch (error) {
    console.error('Failed to save appearance settings:', error);
  }
}

/**
 * useAppearance Hook
 * 管理单机版应用设置（暗黑模式等）
 */
export function useAppearance() {
  const [darkMode, setDarkMode] = useState<boolean>(false);
  const [fontSize, setFontSizeState] = useState<AppFontSize>(DEFAULT_FONT_SIZE);

  // Load settings on mount
  useEffect(() => {
    loadSettings();
  }, []);

  // Apply dark mode
  useEffect(() => {
    document.body.classList.toggle('theme-dark', darkMode);
  }, [darkMode]);

  useEffect(() => {
    document.documentElement.style.setProperty('--app-font-size', FONT_SIZE_VALUES[fontSize]);
  }, [fontSize]);

  const loadSettings = () => {
    try {
      const hasSavedSettings = localStorage.getItem(SETTINGS_STORAGE_KEY) !== null;
      const settings = readStoredSettings();
      if (typeof settings.darkMode === 'boolean') {
        setDarkMode(settings.darkMode);
      } else if (hasSavedSettings) {
        setDarkMode(window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches);
      }
      if (isAppFontSize(settings.fontSize)) {
        setFontSizeState(settings.fontSize);
      }
    } catch (error) {
      console.error('Failed to load settings:', error);
    }
  };

  const updateDarkMode = useCallback((value: boolean) => {
    setDarkMode(value);
    saveStoredSettings({ darkMode: value });
  }, []);

  const updateFontSize = useCallback((value: AppFontSize) => {
    setFontSizeState(value);
    saveStoredSettings({ fontSize: value });
  }, []);

  return {
    darkMode,
    setDarkMode: updateDarkMode,
    fontSize,
    setFontSize: updateFontSize,
  };
}
