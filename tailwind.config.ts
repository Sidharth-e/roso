import type { Config } from "tailwindcss";

const config: Config = {
  darkMode: "class",
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    colors: {
      transparent: "transparent",
      current: "currentColor",
      primary: "rgb(var(--color-primary) / <alpha-value>)",
      secondary: "rgb(var(--color-secondary) / <alpha-value>)",
      tertiary: "rgb(var(--color-tertiary) / <alpha-value>)",
      background: "rgb(var(--color-background) / <alpha-value>)",
      foreground: "rgb(var(--color-foreground) / <alpha-value>)",
      success: "rgb(var(--color-success) / <alpha-value>)",
      warning: "rgb(var(--color-warning) / <alpha-value>)",
      error: "rgb(var(--color-error) / <alpha-value>)",
      info: "rgb(var(--color-info) / <alpha-value>)",
    },
    extend: {
      borderRadius: {
        popover: "1rem",
        card: "0.75rem",
      },
    },
  },
  plugins: [],
};

export default config;
