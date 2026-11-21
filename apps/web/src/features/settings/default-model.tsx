import type { DefaultModel } from '@hearth/shared';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { CircleAlert, X } from 'lucide-react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Card, Skeleton } from '@/components/ui/display.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryKeys, useDefaultModel, useModels } from '@/lib/queries.ts';
import { ModelPicker } from '../chat/model-picker.tsx';
import { SettingRow } from '../shell/page.tsx';

/**
 * Settings → Chat: the model every new chat starts with, on every device.
 * Saving updates the cached choice, so the next new chat in this tab uses
 * it at once.
 *
 * @returns The card.
 */
export function DefaultModelCard() {
  const t = useT();
  const queryClient = useQueryClient();
  const models = useModels();
  const current = useDefaultModel();
  const save = useMutation({
    mutationFn: (next: DefaultModel) => api.features.setDefaultModel(next),
    onSuccess: (next) => {
      queryClient.setQueryData(queryKeys.defaultModel, next);
      toast.success(t('features.saved'));
    },
    onError: (error) => toast.error(errorMessage(t, error)),
  });
  const choice = current.data ?? null;
  // Its credential may no longer list the model; new chats then fall back.
  const unavailable =
    choice && models.data
      ? !models.data.some((group) => group.credentialId === choice.credentialId && group.models.some((model) => model.id === choice.modelId))
      : false;

  return (
    <Card>
      <SettingRow
        label={t('features.defaultModel')}
        description={
          <>
            {t('features.defaultModelHint')}
            {unavailable && choice ? (
              <span className="mt-2 flex items-start gap-1.5 text-warning" role="status">
                <CircleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
                {t('features.defaultModelUnavailable', { model: choice.modelId })}
              </span>
            ) : null}
          </>
        }
        control={
          current.isPending ? (
            <Skeleton className="h-8 w-48" />
          ) : (
            <div className="flex items-center gap-1" data-default-model>
              <ModelPicker
                selection={choice}
                onSelect={(next) => save.mutate(next)}
                groups={models.data}
                loading={models.isPending}
                side="bottom"
                variant="outline"
                placeholder={t('features.defaultModelNone')}
              />
              {choice ? (
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={t('features.defaultModelClear')}
                  title={t('features.defaultModelClear')}
                  disabled={save.isPending}
                  onClick={() => save.mutate(null)}
                >
                  <X />
                </Button>
              ) : null}
            </div>
          )
        }
      />
    </Card>
  );
}
