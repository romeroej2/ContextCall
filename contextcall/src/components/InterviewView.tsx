import { memo, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { InterviewAnswer, InterviewStatus } from "../types";
import { colors, radius, spacing, typography } from "../theme";

interface Props {
  answers: InterviewAnswer[];
  status: InterviewStatus | null;
  isRecording: boolean;
}

const statusColor: Record<InterviewStatus["state"], string> = {
  starting: colors.warning,
  ready: colors.success,
  error: colors.error,
  stopped: colors.textMuted,
};

/** Minimal markdown: bullets, fenced code, **bold** and `code` — enough for streamed answers. */
function renderMarkdown(text: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const lines = text.split("\n");
  let code: string[] | null = null;
  let key = 0;

  const inline = (line: string): ReactNode[] =>
    line.split(/(\*\*[^*]+\*\*|`[^`]+`)/g).map((part, i) => {
      if (part.startsWith("**") && part.endsWith("**") && part.length > 4) {
        return <strong key={i}>{part.slice(2, -2)}</strong>;
      }
      if (part.startsWith("`") && part.endsWith("`") && part.length > 2) {
        return (
          <code key={i} style={{ fontFamily: "monospace", background: colors.surfaceElevated, padding: "0 3px", borderRadius: radius.sm }}>
            {part.slice(1, -1)}
          </code>
        );
      }
      return part;
    });

  for (const line of lines) {
    if (line.trim().startsWith("```")) {
      if (code) {
        nodes.push(
          <pre key={key++} style={{ fontFamily: "monospace", fontSize: typography.sm, background: colors.surfaceElevated, padding: spacing[2], borderRadius: radius.md, overflowX: "auto", margin: `${spacing[1]}px 0` }}>
            {code.join("\n")}
          </pre>,
        );
        code = null;
      } else {
        code = [];
      }
      continue;
    }
    if (code) {
      code.push(line);
      continue;
    }
    const trimmed = line.trim();
    if (!trimmed) continue;
    const bullet = trimmed.match(/^([-*•]|\d+\.)\s+(.*)$/);
    const heading = trimmed.match(/^#{1,4}\s+(.*)$/);
    if (bullet) {
      nodes.push(
        <div key={key++} style={{ display: "flex", gap: spacing[2], margin: "2px 0" }}>
          <span style={{ color: colors.accent }}>•</span>
          <span>{inline(bullet[2])}</span>
        </div>,
      );
    } else if (heading) {
      nodes.push(
        <div key={key++} style={{ fontWeight: 600, marginTop: spacing[1] }}>
          {inline(heading[1])}
        </div>,
      );
    } else {
      nodes.push(
        <div key={key++} style={{ margin: "2px 0" }}>
          {inline(trimmed)}
        </div>,
      );
    }
  }
  if (code) {
    nodes.push(
      <pre key={key++} style={{ fontFamily: "monospace", fontSize: typography.sm, background: colors.surfaceElevated, padding: spacing[2], borderRadius: radius.md, overflowX: "auto" }}>
        {code.join("\n")}
      </pre>,
    );
  }
  return nodes;
}

function seconds(ms: number | null): string {
  return ms == null ? "" : `${(ms / 1000).toFixed(1)}s`;
}

const AnswerCard = memo(function AnswerCard({
  answer,
  latest,
  deepModel,
}: {
  answer: InterviewAnswer;
  latest: boolean;
  deepModel: string | null;
}) {
  const [showDeep, setShowDeep] = useState(true);
  const waitingFast = !answer.fast && !answer.fastDone;
  const hasDeep = deepModel != null && (answer.deep || !answer.deepDone);

  return (
    <div
      style={{
        background: colors.surface,
        border: `1px solid ${latest ? colors.accent : colors.border}`,
        borderRadius: radius.lg,
        padding: spacing[3],
        marginBottom: spacing[3],
        opacity: latest ? 1 : 0.8,
      }}
    >
      <div style={{ fontSize: typography.sm, color: colors.textSecondary, fontStyle: "italic", marginBottom: spacing[2] }}>
        {answer.manual ? "You asked: " : "Interviewer: "}
        {answer.question}
      </div>

      <div style={{ fontSize: typography.lg, color: colors.text, lineHeight: 1.45 }}>
        {waitingFast ? <span style={{ color: colors.textMuted }}>Thinking…</span> : renderMarkdown(answer.fast)}
      </div>
      {answer.fastFirstMs != null && (
        <div style={{ fontSize: typography.xs, color: colors.textMuted, marginTop: spacing[1] }}>
          ⚡ {seconds(answer.fastFirstMs)}
        </div>
      )}

      {hasDeep && (
        <div style={{ marginTop: spacing[3], borderTop: `1px solid ${colors.border}`, paddingTop: spacing[2] }}>
          <button
            onClick={() => setShowDeep((v) => !v)}
            style={{ background: "none", border: "none", padding: 0, cursor: "pointer", fontSize: typography.sm, color: colors.accent, fontWeight: 600 }}
          >
            {showDeep ? "▾" : "▸"} Depth · {deepModel}
            {answer.deepFirstMs != null ? ` · ${seconds(answer.deepFirstMs)}` : ""}
            {!answer.deepDone && answer.deep ? " · streaming" : ""}
          </button>
          {showDeep && (
            <div style={{ fontSize: typography.md, color: colors.text, lineHeight: 1.45, marginTop: spacing[1] }}>
              {answer.deep ? renderMarkdown(answer.deep) : <span style={{ color: colors.textMuted }}>Researching…</span>}
            </div>
          )}
        </div>
      )}

      {answer.error && (
        <div style={{ fontSize: typography.sm, color: colors.error, marginTop: spacing[2] }}>{answer.error}</div>
      )}
    </div>
  );
});

export function InterviewView({ answers, status, isRecording }: Props) {
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<string | null>(null);

  // Typed questions work any time (the copilot starts on demand); the empty
  // "Answer" needs a live session to know what the other side just said.
  const canAsk = isRecording || typed.trim().length > 0;

  const askNow = async () => {
    if (!canAsk) return;
    try {
      await invoke("interview_answer_now", { question: typed.trim() || null });
      setTyped("");
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };

  const newestFirst = [...answers].reverse();

  return (
    <div style={{ padding: spacing[3], display: "flex", flexDirection: "column", gap: spacing[3], height: "100%", overflowY: "auto" }}>
      <div style={{ display: "flex", alignItems: "center", gap: spacing[2], fontSize: typography.sm, color: colors.textSecondary }}>
        <span
          style={{
            width: 8,
            height: 8,
            borderRadius: "50%",
            background: status ? statusColor[status.state] : colors.textMuted,
            flexShrink: 0,
          }}
        />
        <span>
          {status
            ? `${status.message} · ${status.fastModel}${status.deepModel ? ` + ${status.deepModel}` : ""}`
            : isRecording
              ? "Copilot is not running"
              : "Type any question to start the copilot, or start a session for live answers"}
        </span>
      </div>

      <div style={{ display: "flex", gap: spacing[2] }}>
        <input
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.nativeEvent.isComposing) {
              e.preventDefault();
              void askNow();
            }
          }}
          placeholder={
            isRecording
              ? "Ask anything (e.g. what's the ORM in Ruby?), or press Answer for what they just said"
              : "Ask anything — e.g. what's the ORM in Ruby?"
          }
          style={{
            flex: 1,
            fontSize: typography.md,
            padding: `${spacing[2]}px ${spacing[3]}px`,
            border: `1px solid ${colors.border}`,
            borderRadius: radius.md,
            background: colors.surface,
            color: colors.text,
          }}
        />
        <button
          onClick={() => void askNow()}
          disabled={!canAsk}
          style={{
            fontSize: typography.md,
            padding: `${spacing[2]}px ${spacing[3]}px`,
            borderRadius: radius.md,
            border: "none",
            background: canAsk ? colors.accent : colors.border,
            color: colors.textInverse,
            cursor: canAsk ? "pointer" : "default",
          }}
        >
          {typed.trim() ? "Ask" : "Answer"}
        </button>
      </div>
      {error && <div style={{ fontSize: typography.sm, color: colors.error }}>{error}</div>}

      {newestFirst.length === 0 ? (
        <div style={{ fontSize: typography.md, color: colors.textMuted, textAlign: "center", marginTop: spacing[6] }}>
          Answers appear here as soon as someone asks a question — or type your own above.
        </div>
      ) : (
        <div>
          {newestFirst.map((answer, i) => (
            <AnswerCard key={answer.id} answer={answer} latest={i === 0} deepModel={status?.deepModel ?? null} />
          ))}
        </div>
      )}
    </div>
  );
}
