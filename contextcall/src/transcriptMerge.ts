import type { Utterance } from "./types";

// STT engines emit a final segment every few seconds, which reads as a stack of
// tiny bubbles. This after-pass folds consecutive segments from the same speaker
// into one bubble until there is a real pause or the bubble ends a sentence and
// is already long enough to stand on its own.

/** A silence longer than this starts a new bubble. */
const MAX_GAP_MS = 8_000;
/** Once a bubble is this long, break at the next sentence end. */
const SOFT_MAX_CHARS = 280;
/** Never grow a bubble past this, even mid-sentence. */
const HARD_MAX_CHARS = 700;

const SENTENCE_END = /[.!?…。！？]["'”’)\]]*$/;

function sameSpeaker(a: Utterance, b: Utterance): boolean {
  return a.speaker === b.speaker && (a.participantId || null) === (b.participantId || null);
}

function lastActivityMs(utterance: Utterance): number {
  return Date.parse(utterance.endTimestamp || utterance.timestamp);
}

export function shouldMerge(previous: Utterance, next: Utterance): boolean {
  if (!sameSpeaker(previous, next)) return false;

  const gap = Date.parse(next.timestamp) - lastActivityMs(previous);
  if (Number.isNaN(gap) || gap > MAX_GAP_MS) return false;

  const text = previous.text.trim();
  if (text.length + next.text.length > HARD_MAX_CHARS) return false;
  if (text.length >= SOFT_MAX_CHARS && SENTENCE_END.test(text)) return false;

  return true;
}

function joinText(a: string, b: string): string {
  const left = a.trimEnd();
  const right = b.trimStart();
  if (!left) return right;
  if (!right) return left;
  return `${left} ${right}`;
}

function merge(previous: Utterance, next: Utterance): Utterance {
  return {
    ...previous,
    text: joinText(previous.text, next.text),
    endTimestamp: next.endTimestamp || next.timestamp,
  };
}

/** Appends `next` to the transcript, merging it into the last bubble when it continues it. */
export function appendMerged(utterances: Utterance[], next: Utterance): Utterance[] {
  const previous = utterances[utterances.length - 1];
  if (!previous || !shouldMerge(previous, next)) {
    return [...utterances, next];
  }
  return [...utterances.slice(0, -1), merge(previous, next)];
}

/** Runs the merge pass over a whole transcript, e.g. one loaded from history. */
export function mergeUtterances(utterances: Utterance[]): Utterance[] {
  const result: Utterance[] = [];
  for (const next of utterances) {
    const previous = result[result.length - 1];
    if (previous && shouldMerge(previous, next)) {
      result[result.length - 1] = merge(previous, next);
    } else {
      result.push(next);
    }
  }
  return result;
}
