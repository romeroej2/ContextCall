// Pure helpers for the settings UI.
// Extracted from SettingsView.tsx (EP0007 / US0029) to shrink the monolith.

import type { ApiKeys, AppSettings } from "../../types";

export function resolveWhisperModel(
  locale: string,
  whisperModel: string,
): "tiny" | "tiny-en" | "base" | "base-en" | "small" | "small-en" | "medium" | "medium-en" | "large-v3-turbo" {
  const normalized = locale.trim().toLowerCase();
  const isEnglish = normalized.startsWith("en");

  switch (whisperModel) {
    case "tiny":
      return isEnglish ? "tiny-en" : "tiny";
    case "base":
      return isEnglish ? "base-en" : "base";
    case "small":
      return isEnglish ? "small-en" : "small";
    case "tiny-en":
      return "tiny-en";
    case "base-en":
      return "base-en";
    case "small-en":
      return "small-en";
    case "medium":
      return isEnglish ? "medium-en" : "medium";
    case "medium-en":
      return "medium-en";
    case "large-v3-turbo":
      return "large-v3-turbo";
    default:
      return "base";
  }
}

export function arraysEqual(left: string[], right: string[]) {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

export function appSettingsEqual(left: AppSettings | null, right: AppSettings | null) {
  if (!left || !right) {
    return left === right;
  }
  return JSON.stringify(left) === JSON.stringify(right);
}

export function apiKeysEqual(left: ApiKeys | null, right: ApiKeys | null) {
  if (!left || !right) {
    return left === right;
  }
  return JSON.stringify(left) === JSON.stringify(right);
}

export function shouldReindexKnowledgeBase(previous: AppSettings, next: AppSettings) {
  const ollamaEmbeddingConfigChanged =
    (previous.embeddingProvider === "ollama" || next.embeddingProvider === "ollama") &&
    (previous.ollamaBaseUrl !== next.ollamaBaseUrl ||
      previous.ollamaEmbedModel !== next.ollamaEmbedModel);
  const openAiEmbeddingConfigChanged =
    (previous.embeddingProvider === "openai" || next.embeddingProvider === "openai") &&
    (previous.openAiEmbedBaseUrl !== next.openAiEmbedBaseUrl ||
      previous.openAiEmbedModel !== next.openAiEmbedModel);

  return (
    previous.kbFolderPath !== next.kbFolderPath ||
    previous.obsidianVaultPath !== next.obsidianVaultPath ||
    !arraysEqual(previous.obsidianKbIncludePaths, next.obsidianKbIncludePaths) ||
    previous.obsidianNotesFolder !== next.obsidianNotesFolder ||
    previous.embeddingProvider !== next.embeddingProvider ||
    ollamaEmbeddingConfigChanged ||
    openAiEmbeddingConfigChanged
  );
}

export function shouldReindexAfterApiKeySave(
  settings: AppSettings | null,
  previous: ApiKeys,
  next: ApiKeys,
) {
  if (!settings) {
    return false;
  }

  switch (settings.embeddingProvider) {
    case "voyage":
      return previous.voyageApiKey !== next.voyageApiKey;
    case "openai":
      return previous.openAiEmbedApiKey !== next.openAiEmbedApiKey;
    default:
      return false;
  }
}
