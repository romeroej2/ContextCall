import { useLiveSessionStore } from "../hooks/useLiveSessionStore";
import { colors } from "../theme";
import { InterviewView } from "./InterviewView";
import { TranscriptWorkspace } from "./TranscriptWorkspace";

interface InterviewWorkspaceProps {
  currentSessionId?: string;
  isRecording: boolean;
}

/** Interview mode main pane: answers take the primary (left) column, live transcript sits on the right. */
export function InterviewWorkspace({ currentSessionId, isRecording }: InterviewWorkspaceProps) {
  const interviewAnswers = useLiveSessionStore((state) => state.interviewAnswers);
  const interviewStatus = useLiveSessionStore((state) => state.interviewStatus);

  return (
    <div style={{ flex: 1, minHeight: 0, display: "flex" }}>
      <div style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column" }}>
        <InterviewView answers={interviewAnswers} status={interviewStatus} isRecording={isRecording} />
      </div>
      <div
        style={{
          flex: "0 0 clamp(260px, 36%, 420px)",
          minWidth: 0,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
          borderLeft: `1px solid ${colors.border}`,
        }}
      >
        <TranscriptWorkspace currentSessionId={currentSessionId} />
      </div>
    </div>
  );
}
