import type {
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  KeyboardEvent,
  ReactNode,
  TextareaHTMLAttributes,
} from "react";
import { useTaskUiStore } from "../store";
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

interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  children: ReactNode;
}

export function IconButton({
  label,
  className = "",
  type = "button",
  children,
  ...rest
}: IconButtonProps) {
  return (
    <button
      type={type}
      aria-label={label}
      title={label}
      className={`icon-btn ${className}`}
      {...rest}
    >
      {children}
    </button>
  );
}

export function HudIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <rect x="2" y="2.5" width="12" height="11" rx="2" stroke="currentColor" strokeWidth="1.5" />
      <path d="M9.5 2.5V13.5" stroke="currentColor" strokeWidth="1.5" />
    </svg>
  );
}

export function SunIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <circle cx="8" cy="8" r="3" stroke="currentColor" strokeWidth="1.5" />
      <path
        d="M8 1.5V3M8 13V14.5M1.5 8H3M13 8H14.5M3.4 3.4L4.5 4.5M11.5 11.5L12.6 12.6M12.6 3.4L11.5 4.5M4.5 11.5L3.4 12.6"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
    </svg>
  );
}

export function MoonIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="M12.8 10.2A5.5 5.5 0 0 1 5.8 3.2a5.5 5.5 0 1 0 7 7Z"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function CloseIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="M4 4L12 12M12 4L4 12"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
    </svg>
  );
}

export function MinimizeIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path d="M3.5 10.5H12.5" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
    </svg>
  );
}

export function ExpandIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="M6 3.5H12.5V10M12.5 3.5L7.5 8.5M10 12.5H3.5V6"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label: string;
  id: string;
}

export function Input({ label, id, className = "", ...rest }: InputProps) {
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-xs font-medium text-foreground/85">
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
      <label htmlFor={id} className="text-xs font-medium text-foreground/85">
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
          <span className="text-xs text-tertiary">{description}</span>
        ) : null}
      </div>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 ${
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

export function ErrorToast() {
  const toastError = useTaskUiStore((s) => s.toastError);
  const clearToastError = useTaskUiStore((s) => s.clearToastError);

  if (!toastError) {
    return null;
  }

  return (
    <div
      role="alert"
      className="mb-2.5 flex items-start justify-between gap-2 rounded-lg border border-error/40 bg-error/15 px-3 py-2 text-xs text-error"
    >
      <span className="min-w-0 break-words font-medium">{toastError}</span>
      <button
        type="button"
        onClick={clearToastError}
        aria-label="Dismiss error"
        className="shrink-0 rounded px-1 py-0.5 text-xs font-bold text-error hover:bg-error/20 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-error/40"
      >
        Dismiss
      </button>
    </div>
  );
}

interface TabBarProps {
  activeTab: TrayTab;
  onSelect: (tab: TrayTab) => void;
}

const TABS: ReadonlyArray<{ id: TrayTab; label: string }> = [
  { id: "task", label: "Command" },
  { id: "history", label: "History" },
  { id: "settings", label: "Settings" },
];

export function TabBar({ activeTab, onSelect }: TabBarProps) {
  function handleKeyDown(event: KeyboardEvent<HTMLButtonElement>, idx: number) {
    let nextIdx: number | null = null;
    if (event.key === "ArrowRight") {
      nextIdx = (idx + 1) % TABS.length;
    } else if (event.key === "ArrowLeft") {
      nextIdx = (idx - 1 + TABS.length) % TABS.length;
    } else if (event.key === "Home") {
      nextIdx = 0;
    } else if (event.key === "End") {
      nextIdx = TABS.length - 1;
    }
    if (nextIdx !== null) {
      event.preventDefault();
      const targetTab = TABS[nextIdx];
      onSelect(targetTab.id);
      document.getElementById(`tab-${targetTab.id}`)?.focus();
    }
  }

  return (
    <div role="tablist" aria-label="Task Companion views" className="nav-dock">
      {TABS.map((tab, idx) => {
        const selected = tab.id === activeTab;
        return (
          <button
            key={tab.id}
            id={`tab-${tab.id}`}
            type="button"
            role="tab"
            aria-selected={selected}
            aria-controls={`panel-${tab.id}`}
            tabIndex={selected ? 0 : -1}
            onClick={() => onSelect(tab.id)}
            onKeyDown={(e) => handleKeyDown(e, idx)}
            className={selected ? "nav-dock-item-active" : "nav-dock-item"}
          >
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}

export function SendIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path
        d="M8 13V3M8 3L3.5 7.5M8 3L12.5 7.5"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
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
  if (outcome === "failed" || outcome === "max_steps" || outcome === "unverified" || outcome === "recovery_exhausted") {
    return "error";
  }
  if (outcome === "stopped") {
    return "warning";
  }
  return "neutral";
}

export function stepStatusClass(status: string): string {
  if (status === "verified" || status === "done") {
    return "text-success";
  }
  if (status === "failed" || status === "unverified_failed" || status === "denied") {
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
      className="h-1 w-full overflow-hidden rounded-full bg-primary/10"
      style={{ filter: "drop-shadow(0 0 4px rgb(var(--color-primary) / 0.8))" }}
    >
      <rect x={0} y={0} width={pct} height={6} className="fill-primary" />
    </svg>
  );
}
