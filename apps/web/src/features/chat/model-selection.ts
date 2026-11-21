import type { ConversationSummary, CredentialModels, ModelSelection } from '@hearth/shared';
import { useCallback, useState } from 'react';
import { useConversations, useDefaultModel, useModels } from '@/lib/queries.ts';

/** Models the picker offers as recently used. */
const RECENT_MODELS = 5;

/**
 * The models of the latest chats, newest first and each once; only models
 * the credentials still offer. Taken from the chats, so every device of the
 * user shows the same.
 *
 * @param conversations - The user's chats, newest activity first.
 * @param groups - Models per credential.
 * @param limit - How many at most.
 * @returns The selections.
 */
export function recentModels(
  conversations: ConversationSummary[] | undefined,
  groups: CredentialModels[] | undefined,
  limit = RECENT_MODELS,
): ModelSelection[] {
  const recent: ModelSelection[] = [];
  for (const { credentialId, modelId } of conversations ?? []) {
    if (recent.length >= limit) {
      break;
    }
    const offered = groups?.some((group) => group.credentialId === credentialId && group.models.some((model) => model.id === modelId));
    if (!credentialId || !modelId || !offered || recent.some((seen) => seen.credentialId === credentialId && seen.modelId === modelId)) {
      continue;
    }
    recent.push({ credentialId, modelId });
  }
  return recent;
}

const LAST_MODEL_KEY = 'hearth.lastModel';

function readLastModel(): ModelSelection | null {
  try {
    const parsed = JSON.parse(localStorage.getItem(LAST_MODEL_KEY) ?? 'null') as ModelSelection | null;
    return parsed?.credentialId && parsed.modelId ? parsed : null;
  } catch {
    return null;
  }
}

function rememberModel(selection: ModelSelection): void {
  try {
    localStorage.setItem(LAST_MODEL_KEY, JSON.stringify(selection));
  } catch {
    // Not persisting the choice is acceptable.
  }
}

/** Whether a credential of the list offers the model. */
function offers(groups: CredentialModels[], selection: ModelSelection): boolean {
  return groups.some((group) => group.credentialId === selection.credentialId && group.models.some((model) => model.id === selection.modelId));
}

/** Whether the list still has the selection's credential. */
function hasCredential(groups: CredentialModels[], selection: ModelSelection): boolean {
  return groups.some((group) => group.credentialId === selection.credentialId);
}

/**
 * The model for the next message, from the best source that is still
 * usable. Before the model list has loaded every source counts as usable.
 *
 * @param chosen - The chat's model, or what the user picked on this page.
 * @param fallback - The user's default model, else the last choice in this browser.
 * @param groups - Models per credential, undefined while loading.
 * @returns The selection, or null when no credential offers a model.
 */
function pickModel(
  chosen: ModelSelection | null,
  fallback: { defaultModel: ModelSelection | null; last: ModelSelection | null },
  groups: CredentialModels[] | undefined,
): ModelSelection | null {
  // A chat keeps its model even when the catalog no longer lists it; only a
  // removed credential replaces it.
  if (chosen && (!groups || hasCredential(groups, chosen))) {
    return chosen;
  }
  const { defaultModel, last } = fallback;
  if (defaultModel && (!groups || offers(groups, defaultModel))) {
    return defaultModel;
  }
  if (last && (!groups || hasCredential(groups, last))) {
    return last;
  }
  const first = groups?.find((group) => group.models.length > 0);
  const model = first?.models[0];
  return first && model ? { credentialId: first.credentialId, modelId: model.id } : null;
}

/**
 * Tracks the model used for the next message.
 *
 * Starts from the conversation's model, then the user's default model for
 * new chats (Settings → Chat, the same on every device) while its credential
 * still offers it, then the user's last choice in this browser, then the
 * first available model. A selection whose credential no longer exists is
 * replaced once the model list has loaded.
 *
 * @param initial - Model stored with the conversation, if any.
 * @returns Current selection, a setter that also remembers the choice, the model list query,
 *   whether the chosen model reads images and the models of the latest chats.
 */
export function useModelSelection(initial: { credentialId: string | null; modelId: string | null } | null) {
  const models = useModels();
  const conversations = useConversations();
  const defaults = useDefaultModel();
  const [chosen, setChosen] = useState<ModelSelection | null>(() =>
    initial?.credentialId && initial.modelId ? { credentialId: initial.credentialId, modelId: initial.modelId } : null,
  );
  const [last] = useState(readLastModel);
  const selection = pickModel(chosen, { defaultModel: defaults.data ?? null, last }, models.data);

  const select = useCallback((next: ModelSelection) => {
    rememberModel(next);
    setChosen(next);
  }, []);

  const group = models.data?.find((entry) => entry.credentialId === selection?.credentialId);
  // Whether the chosen model reads images, as the catalog says; undefined when unknown.
  const images = group?.models.find((model) => model.id === selection?.modelId)?.images;

  return { selection, select, models, images, kind: group?.kind, recent: recentModels(conversations.data, models.data) };
}
