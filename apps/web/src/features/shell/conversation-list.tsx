import type { ConversationSummary } from '@harness/shared';
import { useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate, useParams } from '@tanstack/react-router';
import { Ellipsis, MessagesSquare, Pencil, Trash2 } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { type FormEvent, useMemo, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { ConfirmDialog, Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog.tsx';
import { EmptyState, Skeleton } from '@/components/ui/display.tsx';
import { Input } from '@/components/ui/input.tsx';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu.tsx';
import type { MessageKey } from '@/i18n/index.ts';
import { api, errorMessage } from '@/lib/api.ts';
import { springs } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryKeys, useConversations } from '@/lib/queries.ts';
import { disposeChat } from '../chat/chat-store.ts';

const DAY_MS = 86_400_000;

function groupKey(updatedAt: number, startOfToday: number): MessageKey {
  if (updatedAt >= startOfToday) {
    return 'nav.today';
  }
  if (updatedAt >= startOfToday - DAY_MS) {
    return 'nav.yesterday';
  }
  if (updatedAt >= startOfToday - 7 * DAY_MS) {
    return 'nav.last7Days';
  }
  if (updatedAt >= startOfToday - 30 * DAY_MS) {
    return 'nav.last30Days';
  }
  return 'nav.older';
}

function groupByDate(conversations: ConversationSummary[]): { key: MessageKey; items: ConversationSummary[] }[] {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const groups: { key: MessageKey; items: ConversationSummary[] }[] = [];
  for (const conversation of conversations) {
    const key = groupKey(conversation.updatedAt, today.getTime());
    const last = groups.at(-1);
    if (last?.key === key) {
      last.items.push(conversation);
    } else {
      groups.push({ key, items: [conversation] });
    }
  }
  return groups;
}

/**
 * The user's conversations grouped by recency, filtered by `query`.
 *
 * @param props - Search text from the sidebar.
 * @returns The list, a skeleton while loading, or an empty state.
 */
export function ConversationList({ query }: { query: string }) {
  const t = useT();
  const { data, isPending } = useConversations();

  const groups = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const filtered = (data ?? []).filter(
      (conversation) => !needle || (conversation.title ?? t('nav.untitled')).toLowerCase().includes(needle),
    );
    return groupByDate(filtered);
  }, [data, query, t]);

  if (isPending) {
    return (
      <div className="flex flex-col gap-2 px-2 pt-6">
        {['w-3/4', 'w-1/2', 'w-5/6', 'w-2/3', 'w-2/5'].map((width) => (
          <Skeleton key={width} className={`h-5 ${width}`} />
        ))}
      </div>
    );
  }

  if ((data ?? []).length === 0) {
    return <EmptyState icon={<MessagesSquare />} title={t('nav.noChats')} description={t('nav.noChatsHint')} />;
  }

  if (groups.length === 0) {
    return <p className="px-3 py-6 text-center text-sm text-muted-foreground">{t('nav.noResults', { query })}</p>;
  }

  return (
    <div className="flex flex-col gap-4">
      {groups.map((group) => (
        <section key={group.key} aria-label={t(group.key)}>
          <h2 className="px-2 pb-1 pt-2 text-xs font-medium text-muted-foreground">{t(group.key)}</h2>
          <ul className="flex flex-col gap-px">
            <AnimatePresence initial={false}>
              {group.items.map((conversation) => (
                <motion.li
                  key={conversation.id}
                  layout="position"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={springs.layout}
                >
                  <ConversationItem conversation={conversation} />
                </motion.li>
              ))}
            </AnimatePresence>
          </ul>
        </section>
      ))}
    </div>
  );
}

function ConversationItem({ conversation }: { conversation: ConversationSummary }) {
  const t = useT();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const params = useParams({ strict: false }) as { conversationId?: string };
  const [renaming, setRenaming] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const title = conversation.title ?? t('nav.untitled');

  async function remove() {
    try {
      await api.conversations.remove(conversation.id);
      disposeChat(conversation.id);
      queryClient.setQueryData<ConversationSummary[]>(queryKeys.conversations, (list) =>
        list?.filter((item) => item.id !== conversation.id),
      );
      queryClient.removeQueries({ queryKey: queryKeys.conversation(conversation.id) });
      toast.success(t('nav.chatDeleted'));
      if (params.conversationId === conversation.id) {
        await navigate({ to: '/' });
      }
    } catch (error) {
      toast.error(errorMessage(t, error));
      throw error;
    }
  }

  return (
    <div className="group relative flex items-center">
      <Link
        to="/c/$conversationId"
        params={{ conversationId: conversation.id }}
        className="flex min-w-0 flex-1 items-center rounded-lg px-2 py-1.5 pe-9 text-sm text-sidebar-foreground transition-colors duration-(--duration-instant) hover:bg-sidebar-accent hover:text-sidebar-accent-foreground data-[status=active]:bg-sidebar-accent data-[status=active]:font-medium data-[status=active]:text-sidebar-accent-foreground pointer-coarse:min-h-11"
      >
        <span className="truncate">{title}</span>
      </Link>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t('nav.chatActions', { title })}
            className="absolute end-1 size-7 opacity-0 transition-opacity focus-visible:opacity-100 group-hover:opacity-100 data-[state=open]:opacity-100 pointer-coarse:opacity-100"
          >
            <Ellipsis />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuItem onSelect={() => setRenaming(true)}>
            <Pencil />
            {t('common.rename')}
          </DropdownMenuItem>
          <DropdownMenuItem destructive onSelect={() => setDeleting(true)}>
            <Trash2 />
            {t('common.delete')}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <RenameDialog conversation={conversation} open={renaming} onOpenChange={setRenaming} />
      <ConfirmDialog
        open={deleting}
        onOpenChange={setDeleting}
        title={t('nav.deleteChatTitle')}
        description={t('nav.deleteChatBody', { title })}
        confirmLabel={t('common.delete')}
        destructive
        onConfirm={remove}
      />
    </div>
  );
}

function RenameDialog({
  conversation,
  open,
  onOpenChange,
}: {
  conversation: ConversationSummary;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const t = useT();
  const queryClient = useQueryClient();
  const [title, setTitle] = useState(conversation.title ?? '');
  const [saving, setSaving] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const trimmed = title.trim();
    if (!trimmed) {
      return;
    }
    setSaving(true);
    try {
      const updated = await api.conversations.rename(conversation.id, trimmed);
      queryClient.setQueryData<ConversationSummary[]>(queryKeys.conversations, (list) =>
        list?.map((item) => (item.id === updated.id ? updated : item)),
      );
      onOpenChange(false);
    } catch (error) {
      toast.error(errorMessage(t, error));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (next) {
          setTitle(conversation.title ?? '');
        }
        onOpenChange(next);
      }}
    >
      <DialogContent aria-describedby={undefined}>
        <form onSubmit={submit} className="flex flex-col gap-5">
          <DialogHeader>
            <DialogTitle>{t('nav.renameChatTitle')}</DialogTitle>
          </DialogHeader>
          <Input value={title} onChange={(event) => setTitle(event.target.value)} maxLength={200} autoFocus aria-label={t('nav.renameChatTitle')} />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" variant="primary" loading={saving} disabled={!title.trim()}>
              {t('common.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
