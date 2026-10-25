import { useChat } from '@ai-sdk/react';
import type { ConversationDetail } from '@harness/shared';
import { Link, useParams } from '@tanstack/react-router';
import { ArrowDown, MessageSquareOff } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { EmptyState, Skeleton } from '@/components/ui/display.tsx';
import { ApiError, type ChatMessage } from '@/lib/api.ts';
import { fade } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { api } from '@/lib/api.ts';
import { useConversation, usePublicSettings } from '@/lib/queries.ts';
import { getChat, resumeReply, stopReply } from './chat-store.ts';
import { Composer, ComposerHint } from './composer.tsx';
import { ContextMeter } from './context-meter.tsx';
import { EnvironmentButton, useLinkOnline } from './environment.tsx';
import { MessageList } from './messages.tsx';
import { ModelPicker } from './model-picker.tsx';
import { useModelSelection } from './model-selection.ts';
import { ResearchToggle } from './research-toggle.tsx';
import { useStickToBottom } from './use-stick-to-bottom.ts';

/**
 * Route component for `/c/$conversationId`: loads the conversation and shows
 * a skeleton, a not-found state, or the chat.
 *
 * @returns The chat page.
 */
export function ChatPage() {
  const t = useT();
  const { conversationId } = useParams({ strict: false }) as { conversationId: string };
  const query = useConversation(conversationId);

  if (query.isPending) {
    return <ChatSkeleton />;
  }
  if (query.isError) {
    const missing = query.error instanceof ApiError && query.error.status === 404;
    return (
      <div className="flex flex-1 items-center justify-center p-6">
        <EmptyState
          icon={<MessageSquareOff />}
          title={missing ? t('chat.notFoundTitle') : t('errors.unknown')}
          description={missing ? t('chat.notFoundBody') : undefined}
          action={
            <Button variant="primary" asChild>
              <Link to="/">{t('chat.backHome')}</Link>
            </Button>
          }
        />
      </div>
    );
  }
  // Keyed so switching conversations creates a fresh chat state.
  return <ChatView key={query.data.id} conversation={query.data} />;
}

function ChatSkeleton() {
  return (
    <div className="mx-auto flex w-full max-w-(--content-width) flex-1 flex-col gap-8 px-4 py-8" aria-hidden="true">
      <Skeleton className="ms-auto h-11 w-1/2 rounded-2xl" />
      <div className="flex flex-col gap-2">
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-5/6" />
        <Skeleton className="h-4 w-2/3" />
      </div>
      <Skeleton className="ms-auto h-11 w-2/5 rounded-2xl" />
    </div>
  );
}

function ChatView({ conversation }: { conversation: ConversationDetail<ChatMessage> }) {
  const t = useT();
  const { selection, select, models } = useModelSelection(conversation);
  const [entry] = useState(() => getChat(conversation.id, conversation.messages, selection));
  const [research, setResearch] = useState(entry.research.current);
  const { data: publicSettings } = usePublicSettings();
  // Read by the transport when the next request starts.
  entry.selection.current = selection;
  entry.research.current = research;
  const [draft, setDraft] = useState('');
  const [initialIds] = useState(() => new Set(entry.chat.messages.map((message) => message.id)));
  const { ref: scrollRef, atBottom, scrollToBottom } = useStickToBottom();

  const { messages, sendMessage, regenerate, status, error } = useChat<ChatMessage>({ chat: entry.chat });
  // After a reload the server may still be writing the reply.
  useEffect(() => {
    void resumeReply(conversation.id);
  }, [conversation.id]);

  const busy = status === 'submitted' || status === 'streaming';
  const { link } = conversation;
  const online = useLinkOnline(link);

  function submit(text: string) {
    setDraft('');
    scrollToBottom('auto');
    void sendMessage({ text });
  }

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto">
        <MessageList
          messages={messages}
          status={status}
          error={error}
          initialIds={initialIds}
          onRegenerate={() => void regenerate()}
          onApprove={(id, decision) => api.conversations.approve(conversation.id, id, decision)}
        />
      </div>

      <div className="relative shrink-0 px-4 pb-4 pt-2">
        <AnimatePresence>
          {!atBottom ? (
            <motion.div
              variants={fade}
              initial="initial"
              animate="animate"
              exit="exit"
              className="absolute inset-x-0 -top-12 flex justify-center"
            >
              <Button
                variant="outline"
                size="icon-sm"
                className="rounded-full shadow-elevation-2"
                aria-label={t('chat.scrollToBottom')}
                onClick={() => scrollToBottom()}
              >
                <ArrowDown />
              </Button>
            </motion.div>
          ) : null}
        </AnimatePresence>
        <div className="mx-auto w-full max-w-(--content-width)">
          <Composer
            value={draft}
            onValueChange={setDraft}
            onSubmit={submit}
            onStop={() => void stopReply(conversation.id)}
            busy={busy}
            blockedReason={selection ? (link && online === false ? t('agent.offlineHint') : null) : t('chat.modelRequired')}
            autoFocus
            placeholder={link ? t('agent.placeholder', { harness: link.harness, project: link.project }) : undefined}
            status={<ContextMeter messages={messages} />}
            toolbar={
              <>
                <ModelPicker selection={selection} onSelect={select} groups={models.data} loading={models.isPending} />
                <EnvironmentButton conversation={conversation} online={online} />
                {/* Agents in an environment have their own tools. */}
                {!link && publicSettings?.webSearch ? <ResearchToggle pressed={research} onPressedChange={setResearch} /> : null}
              </>
            }
          />
          <ComposerHint />
        </div>
      </div>
    </div>
  );
}
