import { useState } from "react";
import type { TaskHistoryRecord } from "../types";
import {
  Button,
  Card,
  ModelChip,
  StatusBadge,
  categoryDisplayName,
  outcomeTone,
  stepStatusClass,
} from "./UiPrimitives";

interface HistoryViewProps {
  records: TaskHistoryRecord[];
  isError: boolean;
  onRunAgain: (goal: string) => void;
}

export function HistoryView({
  records,
  isError,
  onRunAgain,
}: HistoryViewProps) {
  const [expandedId, setExpandedId] = useState<string | null>(null);

  if (isError) {
    return (
      <Card className="border-error/40 bg-error/10">
        <p className="text-xs font-medium text-error">
          Unable to load task history.
        </p>
      </Card>
    );
  }

  if (records.length === 0) {
    return (
      <Card>
        <p className="text-center text-xs text-tertiary">
          Completed task runs will appear here. Screenshots and OCR text are never stored.
        </p>
      </Card>
    );
  }

  return (
    <div className="space-y-2">
      {records.map((item) => {
        const isExpanded = expandedId === item.taskId;
        return (
          <Card key={item.taskId} className="space-y-2">
            <div className="flex items-start justify-between gap-2">
              <p className="text-xs font-semibold text-foreground">
                {item.goal}
              </p>
              <StatusBadge
                tone={outcomeTone(item.outcome)}
                label={item.outcome}
              />
            </div>

            {item.category || item.selectedModel ? (
              <div className="flex flex-wrap items-center gap-1.5">
                {item.category ? (
                  <StatusBadge
                    tone="info"
                    label={categoryDisplayName(item.category)}
                  />
                ) : null}
                {item.selectedModel ? (
                  <ModelChip role="actor" model={item.selectedModel} />
                ) : null}
              </div>
            ) : null}

            <p className="text-xs text-foreground/80">{item.summary}</p>

            <div className="flex items-center justify-between text-xs text-tertiary">
              <span>Steps: {item.stepCount}</span>
              <span>
                {new Date(item.startedAt).toLocaleTimeString([], {
                  hour: "2-digit",
                  minute: "2-digit",
                })}
              </span>
            </div>

            <div className="flex items-center gap-2 pt-0.5">
              {item.steps.length > 0 ? (
                <Button
                  variant="secondary"
                  className="flex-1"
                  onClick={() =>
                    setExpandedId(isExpanded ? null : item.taskId)
                  }
                >
                  {isExpanded ? "Hide Steps" : `View Steps (${item.steps.length})`}
                </Button>
              ) : null}
              <Button
                variant="primary"
                className="flex-1"
                onClick={() => onRunAgain(item.goal)}
              >
                Run Again
              </Button>
            </div>

            {isExpanded ? (
              <div className="max-h-44 space-y-1.5 overflow-y-auto border-t border-tertiary/20 pt-2">
                {item.steps.map((step) => (
                  <div
                    key={`${item.taskId}-${step.stepNumber}-${step.timestamp}`}
                    className="rounded-lg border border-tertiary/20 bg-background/70 px-2 py-1.5"
                  >
                    <div className="flex items-center justify-between gap-2">
                      <span className="text-xs font-semibold text-primary">
                        #{step.stepNumber} · {step.actionType}
                      </span>
                      <span className={`text-xs ${stepStatusClass(step.status)}`}>
                        {step.status}
                      </span>
                    </div>
                    <p className="text-xs text-foreground">{step.description}</p>
                    {step.verificationNote ? (
                      <p className="text-xs text-tertiary">
                        {step.verificationNote}
                      </p>
                    ) : null}
                  </div>
                ))}
              </div>
            ) : null}
          </Card>
        );
      })}
    </div>
  );
}
