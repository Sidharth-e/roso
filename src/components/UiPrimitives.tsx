import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode } from "react";
import type { TrayTab } from "../types";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "danger";
  children: ReactNode;
}

export function Button({
  variant = "primary",
  className = "",
  type = "button",
  children,
  ...rest
}: ButtonProps) {
  const variantClass =
    variant === "primary"
      ? "ui-btn-primary"
      : variant === "danger"
        ? "ui-btn-danger"
        : "ui-btn-secondary";
  return (
    <button type={type} className={`${variantClass} ${className}`} {...rest}>
      {children}
    </button>
  );
}

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label: string;
  id: string;
}

export function Input({ label, id, className = "", ...rest }: InputProps) {
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-xs font-medium text-foreground/80">
        {label}
      </label>
      <input id={id} className={`ui-input ${className}`} {...rest} />
    </div>
  );
}

interface SwitchProps {
  id: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description?: string;
}

export function Switch({
  id,
  checked,
  onChange,
  label,
  description,
}: SwitchProps) {
  return (
    <div className="flex items-center justify-between gap-3">
      <div className="flex flex-col">
        <label
          htmlFor={id}
          className="cursor-pointer text-xs font-semibold text-foreground"
        >
          {label}
        </label>
        {description ? (
          <span className="text-[11px] text-tertiary">{description}</span>
        ) : null}
      </div>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors focus:outline-none focus:ring-2 focus:ring-primary/40 ${
          checked ? "bg-primary" : "bg-tertiary/40"
        }`}
      >
        <span
          className={`inline-block h-3.5 w-3.5 transform rounded-full bg-background transition-transform ${
            checked ? "translate-x-4" : "translate-x-1"
          }`}
        />
      </button>
    </div>
  );
}

interface StatusBadgeProps {
  tone: "success" | "warning" | "neutral" | "info";
  label: string;
}

export function StatusBadge({ tone, label }: StatusBadgeProps) {
  const toneClasses =
    tone === "success"
      ? "bg-success/15 text-success border border-success/30"
      : tone === "warning"
        ? "bg-warning/15 text-warning border border-warning/30"
        : tone === "info"
          ? "bg-info/15 text-info border border-info/30"
          : "bg-tertiary/20 text-tertiary border border-tertiary/30";
  const dotClass =
    tone === "success"
      ? "bg-success"
      : tone === "warning"
        ? "bg-warning"
        : tone === "info"
          ? "bg-info"
          : "bg-tertiary";

  return (
    <span className={`ui-badge ${toneClasses}`}>
      <span className={`h-1.5 w-1.5 rounded-full ${dotClass}`} />
      {label}
    </span>
  );
}

interface CardProps {
  children: ReactNode;
  className?: string;
}

export function Card({ children, className = "" }: CardProps) {
  return <section className={`surface-card ${className}`}>{children}</section>;
}

interface TabBarProps {
  activeTab: TrayTab;
  onSelect: (tab: TrayTab) => void;
}

const TABS: ReadonlyArray<{ id: TrayTab; label: string }> = [
  { id: "focus", label: "Focus" },
  { id: "history", label: "History" },
  { id: "settings", label: "Settings" },
];

export function TabBar({ activeTab, onSelect }: TabBarProps) {
  return (
    <nav
      aria-label="Focus Companion views"
      className="grid grid-cols-3 gap-1 rounded-lg bg-secondary/80 p-1"
    >
      {TABS.map((tab) => {
        const selected = tab.id === activeTab;
        return (
          <button
            key={tab.id}
            type="button"
            onClick={() => onSelect(tab.id)}
            aria-current={selected ? "page" : undefined}
            className={`rounded-md py-1.5 text-xs font-semibold transition focus:outline-none focus:ring-2 focus:ring-primary/40 ${
              selected
                ? "bg-background text-foreground shadow-sm"
                : "text-tertiary hover:text-foreground"
            }`}
          >
            {tab.label}
          </button>
        );
      })}
    </nav>
  );
}
