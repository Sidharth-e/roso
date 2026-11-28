import { useEffect, useState } from "react";
import type { PendingConfirmation } from "../types";
import { Button, Card } from "./UiPrimitives";

interface ConfirmationCardProps {
  confirmation: PendingConfirmation;
  isPending: boolean;
  onResolve: (approved: boolean) => void;
  /** Off in the HUD, which never takes keyboard focus. */
  keyboardShortcuts?: boolean;
}

export function ConfirmationCard({
  confirmation,
  isPending,
  onResolve,
  keyboardShortcuts = true,
}: ConfirmationCardProps) {
  const [secondsLeft, setSecondsLeft] = useState<number | null>(null);

  useEffect(() => {
    if (!confirmation.expiresAt) {
      setSecondsLeft(null);
      return;
    }
    const expiresMs = Date.parse(confirmation.expiresAt);
    if (Number.isNaN(expiresMs)) {
      setSecondsLeft(null);
      return;
    }

    function updateCountdown() {
      const remaining = Math.max(0, Math.ceil((expiresMs - Date.now()) / 1000));
      setSecondsLeft(remaining);
    }

    updateCountdown();
    const timer = window.setInterval(updateCountdown, 1000);
    return () => window.clearInterval(timer);
  }, [confirmation.expiresAt, confirmation.id]);

  useEffect(() => {
    if (!keyboardShortcuts) {
      return;
    }
    function onKeyDown(event: KeyboardEvent) {
      if (isPending) {
        return;
      }
      if (event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
        onResolve(true);
      } else if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onResolve(false);
      }
    }

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [isPending, onResolve, keyboardShortcuts]);

  return (
    <Card className="space-y-2.5 border-warning/50 bg-warning/10">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs font-bold text-warning">
          Approve Step #{confirmation.stepNumber}
        </span>
        <div className="flex items-center gap-1.5">
          {secondsLeft !== null ? (
            <span className="rounded bg-background/80 px-1.5 py-0.5 text-[10px] font-medium text-tertiary">
              Auto-deny in {secondsLeft}s
            </span>
          ) : null}
          <span className="rounded bg-warning/20 px-1.5 py-0.5 text-[10px] font-semibold text-warning">
            {confirmation.actionType}
          </span>
        </div>
      </div>

      <p className="text-xs font-semibold text-foreground break-words">
        {confirmation.description}
      </p>

      {confirmation.targetApp ? (
        <p className="text-xs text-foreground/85">
          Target app: <span className="font-semibold">{confirmation.targetApp}</span>
        </p>
      ) : null}

      {confirmation.targetUrl ? (
        <p className="break-all font-mono text-xs text-foreground/85">
          URL: {confirmation.targetUrl}
        </p>
      ) : null}

      {confirmation.fullPayload ? (
        <div className="max-h-32 overflow-y-auto rounded-lg border border-tertiary/25 bg-background/85 p-2 font-mono text-xs text-foreground break-all whitespace-pre-wrap">
          {confirmation.fullPayload}
        </div>
      ) : null}

      <p className="text-xs text-foreground/80">{confirmation.reason}</p>

      <div className="grid grid-cols-2 gap-2">
        <Button
          variant="primary"
          disabled={isPending}
          onClick={() => onResolve(true)}
        >
          {keyboardShortcuts ? "Approve (⏎)" : "Approve"}
        </Button>
        <Button
          variant="danger"
          disabled={isPending}
          onClick={() => onResolve(false)}
        >
          {keyboardShortcuts ? "Deny (Esc)" : "Deny"}
        </Button>
      </div>
    </Card>
  );
}
