import { useSyncExternalStore } from "react";
import type {
  InterviewAnswer,
  InterviewStatus,
  InterviewTier,
  KBResult,
  Suggestion,
  TranscriptionProgress,
  Utterance,
} from "../types";

type SpeakerKey = "you" | "them";

export interface AudioLevelsState {
  raw: number;
  mic: number;
  them: number;
  micTransmitActive: boolean;
}

export interface SuggestionStatusState {
  isGenerating: boolean;
  lastCheckedAt: string | null;
  lastCheckSurfaced: boolean | null;
}

export interface LiveSessionState {
  utterances: Utterance[];
  suggestions: Suggestion[];
  volatileYouText: string;
  volatileThemText: string;
  audioLevels: AudioLevelsState;
  transcriptionProgress: TranscriptionProgress;
  suggestionStatus: SuggestionStatusState;
  /** Newest last. Interview mode only. */
  interviewAnswers: InterviewAnswer[];
  interviewStatus: InterviewStatus | null;
}

export interface InterviewQuestionPayload {
  id: string;
  question: string;
  manual: boolean;
  askedAt: string;
}

export interface InterviewAnswerPayload {
  id: string;
  tier: InterviewTier;
  kind: "delta" | "done" | "dismissed" | "error";
  text: string;
  elapsedMs: number;
}

const MAX_INTERVIEW_ANSWERS = 30;

interface TranscriptPayload {
  text: string;
  speaker: string;
  participantId?: string;
  participantLabel?: string;
}

interface SuggestionPayload {
  id: string;
  kind?: Suggestion["kind"];
  text: string;
  kbHits?: KBResult[];
}

type Listener = () => void;

function looksLikeSourceReferenceList(text: string): boolean {
  const parts = text
    .split(" . ")
    .map((part) => part.trim())
    .filter(Boolean);

  if (parts.length === 0) {
    return false;
  }

  return parts.every((part) => {
    const normalized = part.replace(/\\/g, "/");
    const hasMarkdownTarget = normalized.includes(".md");
    const hasHeadingAnchor = normalized.includes("#");
    const pathLike =
      normalized.startsWith("ContextCall/") || normalized.includes("/") || normalized.includes("\\");
    return hasMarkdownTarget && hasHeadingAnchor && pathLike;
  });
}

const EMPTY_AUDIO_LEVELS: AudioLevelsState = {
  raw: 0,
  mic: 0,
  them: 0,
  micTransmitActive: false,
};

const EMPTY_TRANSCRIPTION_PROGRESS: TranscriptionProgress = {
  capturedSegments: 0,
  processedSegments: 0,
};

const EMPTY_SUGGESTION_STATUS: SuggestionStatusState = {
  isGenerating: false,
  lastCheckedAt: null,
  lastCheckSurfaced: null,
};

const EMPTY_STATE: LiveSessionState = {
  utterances: [],
  suggestions: [],
  volatileYouText: "",
  volatileThemText: "",
  audioLevels: EMPTY_AUDIO_LEVELS,
  transcriptionProgress: EMPTY_TRANSCRIPTION_PROGRESS,
  suggestionStatus: EMPTY_SUGGESTION_STATUS,
  interviewAnswers: [],
  interviewStatus: null,
};

class LiveSessionStore {
  private state: LiveSessionState = EMPTY_STATE;
  private listeners = new Set<Listener>();

  subscribe = (listener: Listener) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  getState = () => this.state;

  private commit(nextState: LiveSessionState) {
    if (Object.is(nextState, this.state)) {
      return;
    }
    this.state = nextState;
    this.listeners.forEach((listener) => listener());
  }

  private update(updater: (current: LiveSessionState) => LiveSessionState) {
    this.commit(updater(this.state));
  }

  resetSession = () => {
    this.commit({
      ...EMPTY_STATE,
      audioLevels: this.state.audioLevels.micTransmitActive
        ? { ...EMPTY_AUDIO_LEVELS, micTransmitActive: false }
        : EMPTY_AUDIO_LEVELS,
      // Status comes from the backend lifecycle; it can arrive before the reset.
      interviewStatus: this.state.interviewStatus,
    });
  };

  replaceTranscript = (utterances: Utterance[]) => {
    this.update((current) => ({
      ...current,
      utterances,
      suggestions: [],
      volatileYouText: "",
      volatileThemText: "",
      transcriptionProgress: EMPTY_TRANSCRIPTION_PROGRESS,
      suggestionStatus: EMPTY_SUGGESTION_STATUS,
    }));
  };

  addTranscriptUtterance = (payload: TranscriptPayload) => {
    const speaker = payload.speaker === "you" ? "you" : "them";
    this.update((current) => ({
      ...current,
      utterances: [
        ...current.utterances,
        {
          id: crypto.randomUUID(),
          text: payload.text,
          speaker,
          participantId: payload.participantId || null,
          participantLabel: payload.participantLabel || null,
          timestamp: new Date().toISOString(),
        },
      ],
      volatileYouText: speaker === "you" ? "" : current.volatileYouText,
      volatileThemText: speaker === "them" ? "" : current.volatileThemText,
    }));
  };

  setVolatileTranscript = (speaker: SpeakerKey, text: string) => {
    this.update((current) =>
      speaker === "you"
        ? { ...current, volatileYouText: text }
        : { ...current, volatileThemText: text },
    );
  };

  setAudioLevels = (audioLevels: AudioLevelsState) => {
    this.update((current) => ({ ...current, audioLevels }));
  };

  setTranscriptionProgress = (transcriptionProgress: TranscriptionProgress) => {
    this.update((current) => ({ ...current, transcriptionProgress }));
  };

  addSuggestion = (payload: SuggestionPayload) => {
    const text = payload.text.trim();
    if (!text || looksLikeSourceReferenceList(text)) {
      this.update((current) => ({
        ...current,
        suggestionStatus: {
          ...current.suggestionStatus,
          isGenerating: false,
        },
      }));
      return;
    }

    this.update((current) => ({
      ...current,
      suggestions: [
        ...current.suggestions,
        {
          id: payload.id,
          kind: payload.kind || "knowledge_base",
          text,
          timestamp: new Date().toISOString(),
          kbHits: payload.kbHits || [],
        },
      ],
      suggestionStatus: {
        ...current.suggestionStatus,
        isGenerating: false,
      },
    }));
  };

  injectSuggestion = (payload: Omit<Suggestion, "timestamp">) => {
    this.update((current) => ({
      ...current,
      suggestions: [
        ...current.suggestions,
        {
          ...payload,
          timestamp: new Date().toISOString(),
        },
      ],
    }));
  };

  dismissSuggestion = (id: string) => {
    this.update((current) => ({
      ...current,
      suggestions: current.suggestions.filter((suggestion) => suggestion.id !== id),
    }));
  };

  setSuggestionGenerating = (isGenerating: boolean) => {
    this.update((current) => ({
      ...current,
      suggestionStatus: {
        ...current.suggestionStatus,
        isGenerating,
      },
    }));
  };

  addInterviewQuestion = (payload: InterviewQuestionPayload) => {
    this.update((current) => ({
      ...current,
      interviewAnswers: [
        ...current.interviewAnswers,
        {
          id: payload.id,
          question: payload.question,
          manual: payload.manual,
          askedAt: payload.askedAt,
          fast: "",
          deep: "",
          fastDone: false,
          deepDone: false,
          fastFirstMs: null,
          deepFirstMs: null,
          error: null,
        },
      ].slice(-MAX_INTERVIEW_ANSWERS),
    }));
  };

  applyInterviewAnswer = (payload: InterviewAnswerPayload) => {
    this.update((current) => {
      const index = current.interviewAnswers.findIndex((answer) => answer.id === payload.id);
      if (index === -1) {
        return current;
      }
      if (payload.kind === "dismissed" && payload.tier === "fast") {
        return {
          ...current,
          interviewAnswers: current.interviewAnswers.filter((answer) => answer.id !== payload.id),
        };
      }
      const answer = { ...current.interviewAnswers[index] };
      const textKey = payload.tier === "fast" ? "fast" : "deep";
      const doneKey = payload.tier === "fast" ? "fastDone" : "deepDone";
      const firstKey = payload.tier === "fast" ? "fastFirstMs" : "deepFirstMs";
      switch (payload.kind) {
        case "delta":
          answer[textKey] += payload.text;
          answer[firstKey] ??= payload.elapsedMs;
          break;
        case "done":
          if (payload.text.trim()) answer[textKey] = payload.text;
          answer[doneKey] = true;
          break;
        case "dismissed":
          answer[doneKey] = true;
          break;
        case "error":
          answer[doneKey] = true;
          answer.error = payload.text;
          break;
      }
      const interviewAnswers = [...current.interviewAnswers];
      interviewAnswers[index] = answer;
      return { ...current, interviewAnswers };
    });
  };

  setInterviewStatus = (interviewStatus: InterviewStatus) => {
    this.update((current) => ({ ...current, interviewStatus }));
  };

  setSuggestionCheck = (checkedAt: string, surfaced: boolean | null) => {
    this.update((current) => ({
      ...current,
      suggestionStatus: {
        ...current.suggestionStatus,
        lastCheckedAt: checkedAt,
        lastCheckSurfaced: surfaced,
      },
    }));
  };
}

export const liveSessionStore = new LiveSessionStore();

export function useLiveSessionStore<T>(selector: (state: LiveSessionState) => T): T {
  return useSyncExternalStore(
    liveSessionStore.subscribe,
    () => selector(liveSessionStore.getState()),
    () => selector(EMPTY_STATE),
  );
}
