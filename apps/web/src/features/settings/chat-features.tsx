import type { ChatFeatures, UpdateChatFeatures } from '@hearth/shared';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Fragment } from 'react';
import { toast } from 'sonner';
import { Card, Separator, Skeleton } from '@/components/ui/display.tsx';
import { Switch } from '@/components/ui/switch.tsx';
import type { MessageKey } from '@/i18n/index.ts';
import { api, errorMessage } from '@/lib/api.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryKeys, useProviders } from '@/lib/queries.ts';
import { Section, SettingRow } from '../shell/page.tsx';
import { DefaultModelCard } from './default-model.tsx';

type Feature = keyof ChatFeatures;

/** The switches of one card, each with its label and hint. */
const TOOLS: Feature[] = ['webFetch', 'webFetchOpen', 'calculator', 'memory', 'progress'];

/** Switches that only mean something while another one is on. */
const DEPENDS_ON: Partial<Record<Feature, Feature>> = {
  webFetchOpen: 'webFetch',
  openRouterNoFourBit: 'openRouterCheapest',
};

/** Whether the switch `name` is off-limits because the one it needs is off. */
function waitsFor(features: ChatFeatures, name: Feature): boolean {
  const needed = DEPENDS_ON[name];
  return needed !== undefined && !features[needed];
}
const ROUTING: Feature[] = ['openRouterCheapest', 'openRouterNoFourBit'];

/**
 * The user's chat features with optimistic switches that roll back when
 * saving fails.
 */
function useChatFeatures() {
  const t = useT();
  const queryClient = useQueryClient();
  const features = useQuery({ queryKey: queryKeys.features, queryFn: api.features.get });
  const update = useMutation({
    mutationFn: (patch: UpdateChatFeatures) => api.features.update(patch),
    onMutate: async (patch) => {
      await queryClient.cancelQueries({ queryKey: queryKeys.features });
      const previous = queryClient.getQueryData<ChatFeatures>(queryKeys.features);
      queryClient.setQueryData<ChatFeatures>(queryKeys.features, (current) => (current ? { ...current, ...patch } : current));
      return { previous };
    },
    onError: (error, _patch, context) => {
      queryClient.setQueryData(queryKeys.features, context?.previous);
      toast.error(errorMessage(t, error));
    },
    onSuccess: (next) => {
      queryClient.setQueryData(queryKeys.features, next);
      toast.success(t('features.saved'));
    },
  });
  return { features, update };
}

function FeatureCard({ names }: { names: Feature[] }) {
  const t = useT();
  const { features, update } = useChatFeatures();
  const data = features.data;
  return (
    <Card>
      {data ? (
        names.map((name, index) => (
          <Fragment key={name}>
            {index > 0 ? <Separator /> : null}
            <SettingRow
              label={t(`features.${name}` as MessageKey)}
              description={t(`features.${name}Hint` as MessageKey)}
              control={
                <Switch
                  aria-label={t(`features.${name}` as MessageKey)}
                  checked={data[name]}
                  // Without web fetch there is nothing to open, without the
                  // cheapest routing nothing to skip.
                  disabled={waitsFor(data, name)}
                  onCheckedChange={(checked) => update.mutate({ [name]: checked })}
                />
              }
            />
          </Fragment>
        ))
      ) : (
        <div className="flex flex-col gap-4 p-4">
          {names.map((name) => (
            <Skeleton key={name} className="h-12 w-full" />
          ))}
        </div>
      )}
    </Card>
  );
}

/**
 * Settings → Chat: the model new chats start with and the tools the user's
 * chat models and agents get.
 *
 * @returns The panel.
 */
export function ChatFeaturesPanel() {
  const t = useT();
  return (
    <Section title={t('features.title')} description={t('features.description')}>
      <div className="flex flex-col gap-4">
        <DefaultModelCard />
        <FeatureCard names={TOOLS} />
      </div>
    </Section>
  );
}

/**
 * How the user's OpenRouter requests pick a host; only shown with an
 * OpenRouter credential.
 *
 * @returns The section, or nothing.
 */
export function OpenRouterRoutingSection() {
  const t = useT();
  const providers = useProviders();
  if (!providers.data?.some((credential) => credential.kind === 'openrouter')) {
    return null;
  }
  return (
    <Section title={t('features.openRouterTitle')} description={t('features.openRouterDescription')}>
      <FeatureCard names={ROUTING} />
    </Section>
  );
}
