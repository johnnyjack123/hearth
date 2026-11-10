import type { ConversationSummary, CredentialModels, ModelSelection } from '@hearth/shared';
import { useCallback, useEffect, useState } from 'react';
import { useConversations, useModels } from '@/lib/queries.ts';

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

/**
 * Tracks the model used for the next message.
 *
 * Starts from the conversation's last model, then the user's last choice in
 * this browser, then the first available model. A selection whose credential
 * no longer exists is replaced once the model list has loaded.
 *
 * @param initial - Model stored with the conversation, if any.
 * @returns Current selection, a setter that also remembers the choice, the model list query,
 *   whether the chosen model reads images and the models of the latest chats.
 */
export function useModelSelection(initial: { credentialId: string | null; modelId: string | null } | null) {
  const models = useModels();
  const conversations = useConversations();
  const [selection, setSelection] = useState<ModelSelection | null>(() =>
    initial?.credentialId && initial.modelId
      ? { credentialId: initial.credentialId, modelId: initial.modelId }
      : readLastModel(),
  );

  useEffect(() => {
    const groups = models.data;
    if (!groups) {
      return;
    }
    setSelection((current) => {
      if (current && groups.some((group) => group.credentialId === current.credentialId)) {
        return current;
      }
      const first = groups.find((group) => group.models.length > 0);
      const model = first?.models[0];
      return first && model ? { credentialId: first.credentialId, modelId: model.id } : null;
    });
  }, [models.data]);

  const select = useCallback((next: ModelSelection) => {
    rememberModel(next);
    setSelection(next);
  }, []);

  const group = models.data?.find((entry) => entry.credentialId === selection?.credentialId);
  // Whether the chosen model reads images, as the catalog says; undefined when unknown.
  const images = group?.models.find((model) => model.id === selection?.modelId)?.images;

  return { selection, select, models, images, kind: group?.kind, recent: recentModels(conversations.data, models.data) };
}
