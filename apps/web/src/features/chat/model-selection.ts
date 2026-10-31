import type { ModelSelection } from '@harness/shared';
import { useCallback, useEffect, useState } from 'react';
import { useModels } from '@/lib/queries.ts';

const LAST_MODEL_KEY = 'harness.lastModel';

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
 * @returns Current selection, a setter that also remembers the choice, the model list query and
 *   whether the chosen model reads images.
 */
export function useModelSelection(initial: { credentialId: string | null; modelId: string | null } | null) {
  const models = useModels();
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

  // Whether the chosen model reads images, as the catalog says; undefined when unknown.
  const images = models.data
    ?.find((group) => group.credentialId === selection?.credentialId)
    ?.models.find((model) => model.id === selection?.modelId)?.images;

  return { selection, select, models, images };
}
