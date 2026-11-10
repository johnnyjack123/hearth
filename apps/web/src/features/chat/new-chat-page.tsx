import type { ConversationDetail, ConversationSummary, EffortLevel, LinkConversationInput, Tone, Verbosity } from '@hearth/shared';
import { useQueryClient } from '@tanstack/react-query';
import type { FileUIPart } from 'ai';
import { Link, useNavigate, useSearch } from '@tanstack/react-router';
import { Bug, KeyRound, Lightbulb, ListChecks, PenLine } from 'lucide-react';
import { motion } from 'motion/react';
import { useState } from 'react';
import { toast } from 'sonner';
import { Logo } from '@/components/brand/logo.tsx';
import { Button } from '@/components/ui/button.tsx';
import { Card, EmptyState, Skeleton } from '@/components/ui/display.tsx';
import type { MessageKey } from '@/i18n/index.ts';
import { api, type ChatMessage, errorMessage } from '@/lib/api.ts';
import { durations, riseIn, transitions } from '@/lib/motion.ts';
import { usePreferences, useT } from '@/lib/preferences.tsx';
import { queryKeys, useProviders, usePublicSettings, useSession } from '@/lib/queries.ts';
import { Composer, ComposerHint } from './composer.tsx';
import { ModelPicker } from './model-picker.tsx';
import { useModelSelection } from './model-selection.ts';
import { ChatOptions } from './chat-options.tsx';
import { useOpenRouterHost } from './openrouter-host.tsx';
import { getChat } from './chat-store.ts';
import { environmentFromSearch, useLinkOnline, useNewChatEnvironment } from './environment.tsx';

const SUGGESTIONS: { icon: typeof Lightbulb; title: MessageKey; prompt: MessageKey }[] = [
  { icon: Lightbulb, title: 'chat.suggestions.explainTitle', prompt: 'chat.suggestions.explainPrompt' },
  { icon: Bug, title: 'chat.suggestions.debugTitle', prompt: 'chat.suggestions.debugPrompt' },
  { icon: PenLine, title: 'chat.suggestions.draftTitle', prompt: 'chat.suggestions.draftPrompt' },
  { icon: ListChecks, title: 'chat.suggestions.planTitle', prompt: 'chat.suggestions.planPrompt' },
];

/**
 * Start page (`/`): welcome, prompt suggestions and the composer. Sending
 * creates the conversation and hands the message to the chat view.
 *
 * @returns The new-chat page.
 */
export function NewChatPage() {
  const t = useT();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { data: session } = useSession();
  const providers = useProviders();
  const { selection, select, models, images, kind, recent } = useModelSelection(null);
  const host = useOpenRouterHost(kind === 'openrouter' ? (selection ?? null) : null);
  const [draft, setDraft] = useState('');
  const [creating, setCreating] = useState(false);
  const {
    research: lastResearch,
    setResearch: setLastResearch,
    effort: lastEffort,
    setEffort: setLastEffort,
    verbosity: lastVerbosity,
    setVerbosity: setLastVerbosity,
    tone: lastTone,
    setTone: setLastTone,
  } = usePreferences();
  // Starts as in the chat used last; changes here count as using it.
  const [research, setChatResearch] = useState(lastResearch);
  function setResearch(on: boolean) {
    setChatResearch(on);
    setLastResearch(on);
  }
  const [effort, setChatEffort] = useState(lastEffort);
  function setEffort(next: EffortLevel | null) {
    setChatEffort(next);
    setLastEffort(next);
  }
  const [verbosity, setChatVerbosity] = useState(lastVerbosity);
  function setVerbosity(next: Verbosity) {
    setChatVerbosity(next);
    setLastVerbosity(next);
  }
  const [tone, setChatTone] = useState(lastTone);
  function setTone(next: Tone) {
    setChatTone(next);
    setLastTone(next);
  }
  const { data: publicSettings } = usePublicSettings();
  const search = useSearch({ from: '/app/' });
  const [environment, setEnvironment] = useState<LinkConversationInput | null>(() => environmentFromSearch(search));
  // Another environment in the address ("New chat in this environment" from
  // another chat) replaces the choice; compared by value, not identity.
  const searchKey = `${search.device ?? ''}/${search.project ?? ''}/${search.harness ?? ''}`;
  const [seenSearch, setSeenSearch] = useState(searchKey);
  if (searchKey !== seenSearch) {
    setSeenSearch(searchKey);
    setEnvironment(environmentFromSearch(search));
  }
  const online = useLinkOnline(environment);
  const environmentOption = useNewChatEnvironment(environment, setEnvironment);

  async function submit(text: string, files: FileUIPart[]) {
    if (!selection) {
      return;
    }
    setCreating(true);
    try {
      // Created already linked, so the first message goes to the agent.
      const conversation = await api.conversations.create({ ...selection, ...(environment ? { link: environment } : {}) });
      queryClient.setQueryData<ConversationSummary[]>(queryKeys.conversations, (list) => [conversation, ...(list ?? [])]);
      queryClient.setQueryData<ConversationDetail<ChatMessage>>(queryKeys.conversation(conversation.id), {
        ...conversation,
        messages: [],
      });
      // Send before navigating: the chat session outlives the page switch.
      void getChat(conversation.id, [], selection, research, effort, verbosity, tone).chat.sendMessage({ text, files });
      await navigate({ to: '/c/$conversationId', params: { conversationId: conversation.id } });
    } catch (error) {
      toast.error(errorMessage(t, error));
      setCreating(false);
    }
  }

  const firstName = session?.user.name?.split(' ')[0];
  const noProviders = providers.data?.length === 0;

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="mx-auto flex w-full max-w-(--content-width) flex-1 flex-col justify-center gap-10 px-4 py-10">
        <motion.div variants={riseIn} initial="initial" animate="animate" className="flex flex-col items-center gap-4 text-center">
          <Logo className="[&_svg]:size-12" />
          <div className="flex flex-col gap-2">
            <h1 className="text-balance text-3xl font-semibold tracking-tight text-foreground">{t('chat.welcomeTitle')}</h1>
            <p className="text-muted-foreground">
              {firstName ? t('chat.welcomeSubtitleNamed', { name: firstName }) : t('chat.welcomeSubtitle')}
            </p>
          </div>
        </motion.div>

        {providers.isPending ? (
          <Skeleton className="h-32 w-full rounded-2xl" />
        ) : noProviders ? (
          <Card className="p-2">
            <EmptyState
              icon={<KeyRound />}
              title={t('chat.noProviderTitle')}
              description={t('chat.noProviderBody')}
              action={
                <Button variant="primary" asChild>
                  <Link to="/settings" search={{ tab: 'providers' }}>
                    {t('chat.addProvider')}
                  </Link>
                </Button>
              }
            />
          </Card>
        ) : (
          <div className="flex flex-col gap-6">
            <ul className="grid gap-2 sm:grid-cols-2">
              {SUGGESTIONS.map((suggestion, index) => (
                <motion.li
                  key={suggestion.title}
                  initial={{ opacity: 0, y: 6 }}
                  animate={{ opacity: 1, y: 0, transition: { ...transitions.enter, delay: durations.stagger * (index + 1) } }}
                >
                  <button
                    type="button"
                    onClick={() => setDraft(t(suggestion.prompt))}
                    className="group flex h-full w-full items-start gap-3 rounded-xl border border-border bg-card p-3 text-start transition-[background-color,box-shadow] duration-(--duration-fast) ease-standard hover:bg-accent hover:shadow-elevation-1"
                  >
                    <suggestion.icon className="mt-0.5 size-4 shrink-0 text-primary" aria-hidden="true" />
                    <span className="flex min-w-0 flex-col gap-0.5">
                      <span className="text-sm font-medium text-foreground">{t(suggestion.title)}</span>
                      <span className="line-clamp-1 text-xs text-muted-foreground">{t(suggestion.prompt)}</span>
                    </span>
                  </button>
                </motion.li>
              ))}
            </ul>
            <div>
              <Composer
                value={draft}
                onValueChange={setDraft}
                onSubmit={(text, files) => void submit(text, files)}
                files={{ images }}
                blockedReason={
                  creating
                    ? t('common.loading')
                    : !selection
                      ? t('chat.modelRequired')
                      : environment && online === false
                        ? t('agent.offlineHint')
                        : null
                }
                placeholder={environment ? t('agent.placeholder', { harness: environment.harness, project: environment.project }) : undefined}
                autoFocus
                toolbar={
                  <>
                    <ChatOptions
                      environment={environmentOption}
                      research={publicSettings?.webSearch ? { on: research, onChange: setResearch } : null}
                      effort={effort}
                      onEffortChange={setEffort}
                      verbosity={verbosity}
                      onVerbosityChange={setVerbosity}
                      tone={tone}
                      onToneChange={setTone}
                      host={host}
                    />
                    <ModelPicker selection={selection} onSelect={select} groups={models.data} loading={models.isPending} recent={recent} />
                  </>
                }
              />
              {environmentOption.dialogs}
              <ComposerHint />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
