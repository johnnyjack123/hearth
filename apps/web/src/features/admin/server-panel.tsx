import type { ServerSettings } from '@harness/shared';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';
import { Card, Separator, Skeleton } from '@/components/ui/display.tsx';
import { Switch } from '@/components/ui/switch.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryKeys } from '@/lib/queries.ts';
import { Section, SettingRow } from '../shell/page.tsx';

/**
 * Server-wide settings: registration mode and custom endpoints.
 *
 * @returns The server panel.
 */
export function ServerPanel() {
  const t = useT();
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: queryKeys.serverSettings, queryFn: api.settings.get });
  const update = useMutation({
    mutationFn: api.settings.update,
    onMutate: async (patch) => {
      // Optimistic: switches react instantly and roll back on failure.
      await queryClient.cancelQueries({ queryKey: queryKeys.serverSettings });
      const previous = queryClient.getQueryData<ServerSettings>(queryKeys.serverSettings);
      queryClient.setQueryData<ServerSettings>(queryKeys.serverSettings, (current) => (current ? { ...current, ...patch } : current));
      return { previous };
    },
    onError: (error, _patch, context) => {
      queryClient.setQueryData(queryKeys.serverSettings, context?.previous);
      toast.error(errorMessage(t, error));
    },
    onSuccess: (next) => {
      queryClient.setQueryData(queryKeys.serverSettings, next);
      void queryClient.invalidateQueries({ queryKey: queryKeys.publicSettings });
      toast.success(t('admin.saved'));
    },
  });

  return (
    <Section title={t('admin.tabServer')} description={t('admin.serverDescription')}>
      <Card>
        {settings.data ? (
          <>
            <SettingRow
              label={t('admin.registration')}
              description={t('admin.registrationHint')}
              control={
                <Switch
                  aria-label={t('admin.registration')}
                  checked={settings.data.registration === 'open'}
                  onCheckedChange={(checked) => update.mutate({ registration: checked ? 'open' : 'closed' })}
                />
              }
            />
            <Separator />
            <SettingRow
              label={t('admin.customBaseUrls')}
              description={t('admin.customBaseUrlsHint')}
              control={
                <Switch
                  aria-label={t('admin.customBaseUrls')}
                  checked={settings.data.allowCustomBaseUrls}
                  onCheckedChange={(checked) => update.mutate({ allowCustomBaseUrls: checked })}
                />
              }
            />
          </>
        ) : (
          <div className="flex flex-col gap-4 p-4">
            <Skeleton className="h-12 w-full" />
            <Skeleton className="h-12 w-full" />
          </div>
        )}
      </Card>
    </Section>
  );
}
