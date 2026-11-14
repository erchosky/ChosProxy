import { useEffect } from "react";
import type { Theme } from "../bindings/Theme";

/** Applies the theme to <html data-theme>, following the OS for "system". */
export function useTheme(theme: Theme | undefined) {
  useEffect(() => {
    const root = document.documentElement;
    if (theme !== "system") {
      root.dataset.theme = theme ?? "dark";
      return;
    }
    const media = window.matchMedia("(prefers-color-scheme: light)");
    const apply = () => {
      root.dataset.theme = media.matches ? "light" : "dark";
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
}
