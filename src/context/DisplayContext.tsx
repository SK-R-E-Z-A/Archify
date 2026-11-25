import React, {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  useCallback,
} from "react";

export type Theme = "dark" | "light";
export type FontSizePreset = "small" | "medium" | "large";
export type DensityPreset = "comfortable" | "compact";

export interface DisplayContextValue {
  theme: Theme;
  setTheme: (theme: Theme) => void;
  fontSize: FontSizePreset;
  setFontSize: (preset: FontSizePreset) => void;
  density: DensityPreset;
  setDensity: (preset: DensityPreset) => void;
  // Derived CSS values for convenience
  textSize: string;
}

const DisplayContext = createContext<DisplayContextValue | null>(null);

export function useDisplay() {
  const ctx = useContext(DisplayContext);
  if (!ctx) {
    throw new Error("useDisplay must be used within a DisplayProvider");
  }
  return ctx;
}

const THEME_KEY = "archify:theme";
const FONT_SIZE_KEY = "archify:font-size";
const DENSITY_KEY = "archify:density";

const FONT_SIZE_VALUES: Record<FontSizePreset, string> = {
  small: "12px",
  medium: "13px",
  large: "14px",
};

interface DisplayProviderProps {
  children: React.ReactNode;
}

/**
 * DisplayProvider — manages theme, font-size preset, and density preset.
 *
 * Writes [data-theme], [data-font-size], [data-density] attributes to
 * document.documentElement so the token CSS can respond.
 * Respects system color-scheme preference when no saved theme exists.
 */
export function DisplayProvider({ children }: DisplayProviderProps) {
  const [theme, setThemeState] = useState<Theme>("dark");
  const [fontSize, setFontSizeState] = useState<FontSizePreset>("medium");
  const [density, setDensityState] = useState<DensityPreset>("comfortable");

  // Read persisted preferences on mount
  useEffect(() => {
    try {
      const savedTheme = localStorage.getItem(THEME_KEY) as Theme | null;
      if (savedTheme === "light" || savedTheme === "dark") {
        setThemeState(savedTheme);
      } else {
        // Respect system preference
        setThemeState(
          window.matchMedia("(prefers-color-scheme: light)").matches
            ? "light"
            : "dark",
        );
      }
      const savedFontSize = localStorage.getItem(
        FONT_SIZE_KEY,
      ) as FontSizePreset | null;
      if (savedFontSize) setFontSizeState(savedFontSize);
      const savedDensity = localStorage.getItem(
        DENSITY_KEY,
      ) as DensityPreset | null;
      if (savedDensity) setDensityState(savedDensity);
    } catch {
      // localStorage unavailable — keep defaults
    }
  }, []);

  // Listen to system theme changes when no saved preference
  useEffect(() => {
    if (typeof window.matchMedia === "undefined") return;
    const media = window.matchMedia("(prefers-color-scheme: light)");
    const handler = (e: MediaQueryListEvent) => {
      try {
        if (!localStorage.getItem(THEME_KEY)) {
          setThemeState(e.matches ? "light" : "dark");
        }
      } catch {
        /* ignore */
      }
    };
    media.addEventListener("change", handler);
    return () => media.removeEventListener("change", handler);
  }, []);

  // Apply attributes to document element whenever values change
  useEffect(() => {
    const root = document.documentElement;
    root.setAttribute("data-theme", theme);
    root.setAttribute("data-font-size", fontSize);
    root.setAttribute("data-density", density);

    try {
      localStorage.setItem(THEME_KEY, theme);
      localStorage.setItem(FONT_SIZE_KEY, fontSize);
      localStorage.setItem(DENSITY_KEY, density);
    } catch {
      /* ignore */
    }
  }, [theme, fontSize, density]);

  const setTheme = useCallback((next: Theme) => setThemeState(next), []);
  const setFontSize = useCallback((next: FontSizePreset) => setFontSizeState(next), []);
  const setDensity = useCallback((next: DensityPreset) => setDensityState(next), []);

  const value = useMemo<DisplayContextValue>(
    () => ({
      theme,
      setTheme,
      fontSize,
      setFontSize,
      density,
      setDensity,
      textSize: FONT_SIZE_VALUES[fontSize],
    }),
    [theme, fontSize, density],
  );

  return <DisplayContext.Provider value={value}>{children}</DisplayContext.Provider>;
}

export default DisplayProvider;
