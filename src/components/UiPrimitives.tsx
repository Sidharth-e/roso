import type {
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  ReactNode,
  TextareaHTMLAttributes,
} from "react";
import type { TaskPhase, TrayTab } from "../types";

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

interface TextAreaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  label: string;
  id: string;
}

export function TextArea({ label, id, className = "", ...rest }: TextAreaProps) {
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-xs font-medium text-foreground/80">
        {label}
      </label>
      <textarea id={id} className={`ui-input resize-none ${className}`} {...rest} />
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
  tone: "success" | "warning" | "error" | "neutral" | "info";
  label: string;
}

export function StatusBadge({ tone, label }: StatusBadgeProps) {
  const toneClasses =
    tone === "success"
      ? "bg-success/15 text-success border border-success/30"
      : tone === "warning"
        ? "bg-warning/15 text-warning border border-warning/30"
        : tone === "error"
          ? "bg-error/15 text-error border border-error/30"
          : tone === "info"
            ? "bg-info/15 text-info border border-info/30"
            : "bg-tertiary/20 text-tertiary border border-tertiary/30";
  const dotClass =
    tone === "success"
      ? "bg-success"
      : tone === "warning"
        ? "bg-warning"
        : tone === "error"
          ? "bg-error"
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
  { id: "task", label: "Task" },
  { id: "live", label: "Live" },
  { id: "history", label: "History" },
  { id: "settings", label: "Settings" },
];

export function TabBar({ activeTab, onSelect }: TabBarProps) {
  return (
    <nav
      aria-label="Task Companion views"
      className="grid grid-cols-4 gap-1 rounded-lg bg-secondary/80 p-1"
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

export function roleDisplayName(role: string): string {
  switch (role) {
    case "judge":
      return "Judge";
    case "planner":
      return "Planner";
    case "coding_actor":
      return "Coding";
    case "writing_actor":
      return "Writing";
    case "vision_actor":
      return "Vision";
    case "recovery":
      return "Recovery";
    default:
      return role || "System";
  }
}

export function categoryDisplayName(category: string): string {
  switch (category) {
    case "browser_research":
      return "Browser & Research";
    case "coding":
      return "Coding & Dev";
    case "writing":
      return "Writing & Notes";
    case "system_admin":
      return "System & Files";
    case "general_ui":
      return "General UI";
    default:
      return category;
  }
}

export function phaseBadgeProps(phase: TaskPhase | undefined): {
  tone: "success" | "warning" | "error" | "neutral" | "info";
  label: string;
} {
  switch (phase) {
    case "classifying":
      return { tone: "info", label: "Classifying" };
    case "planning":
      return { tone: "info", label: "Planning" };
    case "running":
      return { tone: "info", label: "Acting" };
    case "verifying":
      return { tone: "info", label: "Verifying" };
    case "recovering":
      return { tone: "warning", label: "Recovering" };
    case "awaiting_confirmation":
      return { tone: "warning", label: "Approve?" };
    case "paused":
      return { tone: "neutral", label: "Paused" };
    case "completed":
      return { tone: "success", label: "Done" };
    case "failed":
      return { tone: "error", label: "Failed" };
    case "stopped":
      return { tone: "warning", label: "Stopped" };
    default:
      return { tone: "neutral", label: "Ready" };
  }
}

export function outcomeTone(
  outcome: string
): "success" | "warning" | "error" | "neutral" | "info" {
  if (outcome === "completed") {
    return "success";
  }
  if (outcome === "failed") {
    return "error";
  }
  if (outcome === "stopped" || outcome === "max_steps") {
    return "warning";
  }
  return "neutral";
}

export function stepStatusClass(status: string): string {
  if (status === "verified" || status === "done") {
    return "text-success";
  }
  if (status === "failed" || status === "unverified_failed") {
    return "text-error";
  }
  if (status === "recovery" || status === "retrying") {
    return "text-warning";
  }
  return "text-tertiary";
}

interface ModelChipProps {
  role: string;
  model: string;
  active?: boolean;
}

export function ModelChip({ role, model, active = false }: ModelChipProps) {
  return (
    <span className="model-chip max-w-full">
      {active ? (
        <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-primary animate-pulse" />
      ) : null}
      <span className="shrink-0 text-foreground/75">{roleDisplayName(role)}:</span>
      <span className="truncate font-mono">{model}</span>
    </span>
  );
}

export function StepMeterBar({ value, max }: { value: number; max: number }) {
  const pct = max > 0 ? Math.min(100, Math.max(0, (value / max) * 100)) : 0;

  return (
    <svg
      viewBox="0 0 100 6"
      preserveAspectRatio="none"
      role="progressbar"
      aria-label="Task step progress"
      aria-valuenow={Math.round(pct)}
      aria-valuemin={0}
      aria-valuemax={100}
      className="h-1.5 w-full overflow-hidden rounded-full bg-background/80"
    >
      <rect x={0} y={0} width={pct} height={6} className="fill-primary" />
    </svg>
  );
}
