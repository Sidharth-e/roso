import { memo, useMemo, useState } from "react";
import type { TaskStepRecord } from "../types";
import { Card, ModelChip, stepStatusClass } from "./UiPrimitives";

interface StepRowProps {
  step: TaskStepRecord;
  compact: boolean;
}

const StepRow = memo(function StepRow({ step, compact }: StepRowProps) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="trace-card space-y-1">
      <button
        type="button"
        aria-expanded={expanded}
        onClick={() => setExpanded((prev) => !prev)}
        className="flex w-full items-center justify-between gap-2 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 rounded"
      >
        <span className="text-xs font-bold uppercase tracking-wider text-primary">
          #{step.stepNumber} · {step.actionType}
        </span>
        <span className={`text-xs font-medium ${stepStatusClass(step.status)}`}>
          {step.status}
        </span>
      </button>
      <p className="text-xs font-medium text-foreground">{step.description}</p>
      {!compact ? (
        <p className="text-xs text-tertiary">{step.narration}</p>
      ) : null}
      {expanded ? (
        <div className="space-y-1 border-t border-tertiary/20 pt-1.5">
          {compact ? (
            <p className="text-xs text-tertiary">{step.narration}</p>
          ) : null}
          {step.thought ? (
            <p className="text-xs text-foreground/85">{step.thought}</p>
          ) : null}
          {step.verificationNote ? (
            <p className="text-xs text-foreground/80">
              Check: {step.verificationNote}
            </p>
          ) : null}
          {step.model ? (
            <ModelChip role="actor" model={step.model} />
          ) : null}
        </div>
      ) : step.verificationNote ? (
        <p className="text-xs text-foreground/75">
          Check: {step.verificationNote}
        </p>
      ) : null}
    </div>
  );
});

interface StepFeedProps {
  steps: TaskStepRecord[];
  title: string;
  compact?: boolean;
  showEmpty?: boolean;
}

export function StepFeed({
  steps,
  title,
  compact = false,
  showEmpty = false,
}: StepFeedProps) {
  const reversed = useMemo(() => [...steps].reverse(), [steps]);

  if (steps.length === 0 && !showEmpty) {
    return null;
  }

  return (
    <Card className="space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-xs font-semibold text-foreground">{title}</span>
        <span className="text-xs text-tertiary">{steps.length} actions</span>
      </div>
      {steps.length === 0 ? (
        <p className="text-xs text-tertiary">
          Actions taken on screen will stream here in real time.
        </p>
      ) : (
        <div className="max-h-52 space-y-1.5 overflow-y-auto pr-0.5">
          {reversed.map((step) => (
            <StepRow
              key={`${step.stepNumber}-${step.actionType}-${step.timestamp}`}
              step={step}
              compact={compact}
            />
          ))}
        </div>
      )}
    </Card>
  );
}
