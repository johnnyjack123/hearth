import { type ConversationSummary, MAX_CHAT_TAGS } from '@harness/shared';
import { useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate, useParams } from '@tanstack/react-router';
import { Check, ChevronRight, Ellipsis, Folder, FolderOpen, MessagesSquare, Pencil, Plus, Tag, Trash2 } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { type FormEvent, useMemo, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { ConfirmDialog, Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog.tsx';
import { EmptyState, Skeleton } from '@/components/ui/display.tsx';
import { Input } from '@/components/ui/input.tsx';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu.tsx';
import type { MessageKey } from '@/i18n/index.ts';
import { api, errorMessage } from '@/lib/api.ts';
import { springs } from '@/lib/motion.ts';
import { usePreferences, useT } from '@/lib/preferences.tsx';
import { queryKeys, useConversations } from '@/lib/queries.ts';
import { cn } from '@/lib/utils.ts';
import { disposeChat } from '../chat/chat-store.ts';

const DAY_MS = 86_400_000;

/** Which tag folders are open, per browser. */
const OPEN_FOLDERS_KEY = 'harness.openTagFolders';

function readOpenFolders(): string[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(OPEN_FOLDERS_KEY) ?? '[]');
    return Array.isArray(parsed) ? parsed.filter((name): name is string => typeof name === 'string') : [];
  } catch {
    return [];
  }
}

function writeOpenFolders(names: string[]): void {
  try {
    localStorage.setItem(OPEN_FOLDERS_KEY, JSON.stringify(names));
  } catch {
    // Without storage the folders just start closed next time.
  }
}

/** One folder per tag, by name, with its chats most recent first. */
function groupByTag(conversations: ConversationSummary[], locale: string): { name: string; items: ConversationSummary[] }[] {
  const folders = new Map<string, ConversationSummary[]>();
  for (const conversation of conversations) {
    for (const tag of conversation.tags ?? []) {
      folders.set(tag, [...(folders.get(tag) ?? []), conversation]);
    }
  }
  return [...folders.entries()]
    .map(([name, items]) => ({ name, items }))
    .sort((a, b) => a.name.localeCompare(b.name, locale, { sensitivity: 'base' }));
}

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
 * The user's conversations: a folder per tag at the top, then all of them
 * grouped by recency, filtered by `query` (title or tag).
 *
 * @param props - Search text from the sidebar.
 * @returns The list, a skeleton while loading, or an empty state.
 */
export function ConversationList({ query }: { query: string }) {
  const { t, locale } = usePreferences();
  const { data, isPending } = useConversations();
  const [openFolders, setOpenFolders] = useState(readOpenFolders);
  const searching = query.trim() !== '';

  const { groups, folders, tags } = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const all = data ?? [];
    const filtered = all.filter(
      (conversation) =>
        !needle ||
        (conversation.title ?? t('nav.untitled')).toLowerCase().includes(needle) ||
        (conversation.tags ?? []).some((tag) => tag.toLowerCase().includes(needle)),
    );
    return {
      groups: groupByDate(filtered),
      folders: groupByTag(filtered, locale),
      // Every tag of the user, for tagging a chat.
      tags: groupByTag(all, locale).map((folder) => folder.name),
    };
  }, [data, query, t, locale]);

  function toggleFolder(name: string) {
    const next = openFolders.includes(name) ? openFolders.filter((open) => open !== name) : [...openFolders, name];
    setOpenFolders(next);
    writeOpenFolders(next);
  }

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
      {folders.length > 0 ? (
        <section aria-label={t('nav.tags.folders')}>
          <h2 className="px-2 pb-1 pt-2 text-xs font-medium text-muted-foreground">{t('nav.tags.folders')}</h2>
          <ul className="flex flex-col gap-px">
            {folders.map((folder) => (
              <li key={folder.name}>
                <TagFolder
                  name={folder.name}
                  items={folder.items}
                  tags={tags}
                  // A search shows what it found inside the folders.
                  open={searching || openFolders.includes(folder.name)}
                  onToggle={() => toggleFolder(folder.name)}
                />
              </li>
            ))}
          </ul>
        </section>
      ) : null}
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
                  <ConversationItem conversation={conversation} tags={tags} />
                </motion.li>
              ))}
            </AnimatePresence>
          </ul>
        </section>
      ))}
    </div>
  );
}

/**
 * A tag's folder: opens to its chats; its menu renames the tag or takes it
 * off every chat.
 */
function TagFolder({
  name,
  items,
  tags,
  open,
  onToggle,
}: {
  name: string;
  items: ConversationSummary[];
  tags: string[];
  open: boolean;
  onToggle: () => void;
}) {
  const t = useT();
  const queryClient = useQueryClient();
  const [renaming, setRenaming] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const Icon = open ? FolderOpen : Folder;

  async function remove() {
    try {
      await api.tags.remove(name);
      await queryClient.invalidateQueries({ queryKey: queryKeys.conversations });
    } catch (error) {
      toast.error(errorMessage(t, error));
      throw error;
    }
  }

  return (
    <div data-tag-folder={name}>
      <div className="group relative flex items-center">
        <button
          type="button"
          aria-expanded={open}
          onClick={onToggle}
          className="flex min-w-0 flex-1 items-center gap-2 rounded-lg px-2 py-1.5 pe-9 text-start text-sm text-sidebar-foreground transition-colors duration-(--duration-instant) hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring pointer-coarse:min-h-11"
        >
          <ChevronRight className={cn('size-3.5 shrink-0 text-muted-foreground transition-transform duration-(--duration-fast)', open && 'rotate-90')} aria-hidden="true" />
          <Icon className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
          <span className="truncate">{name}</span>
          <span className="ms-auto shrink-0 text-xs text-muted-foreground">{items.length}</span>
        </button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={t('nav.tags.actions', { name })}
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
              {t('nav.tags.remove')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {open ? (
        <ul className="flex flex-col gap-px ps-5">
          {items.map((conversation) => (
            <li key={conversation.id}>
              <ConversationItem conversation={conversation} tags={tags} />
            </li>
          ))}
        </ul>
      ) : null}
      <RenameTagDialog name={name} open={renaming} onOpenChange={setRenaming} />
      <ConfirmDialog
        open={deleting}
        onOpenChange={setDeleting}
        title={t('nav.tags.removeTitle', { name })}
        description={t('nav.tags.removeBody', { count: items.length })}
        confirmLabel={t('nav.tags.remove')}
        destructive
        onConfirm={remove}
      />
    </div>
  );
}

function RenameTagDialog({ name, open, onOpenChange }: { name: string; open: boolean; onOpenChange: (open: boolean) => void }) {
  const t = useT();
  const queryClient = useQueryClient();
  const [value, setValue] = useState(name);
  const [saving, setSaving] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    const trimmed = value.trim();
    if (!trimmed || trimmed === name) {
      onOpenChange(false);
      return;
    }
    setSaving(true);
    try {
      await api.tags.rename(name, trimmed);
      await queryClient.invalidateQueries({ queryKey: queryKeys.conversations });
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
          setValue(name);
        }
        onOpenChange(next);
      }}
    >
      <DialogContent>
        <form onSubmit={submit} className="flex flex-col gap-5">
          <DialogHeader>
            <DialogTitle>{t('nav.tags.renameTitle')}</DialogTitle>
            <DialogDescription>{t('nav.tags.renameHint')}</DialogDescription>
          </DialogHeader>
          <Input value={value} onChange={(event) => setValue(event.target.value)} maxLength={40} autoFocus aria-label={t('nav.tags.renameTitle')} />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" variant="primary" loading={saving} disabled={!value.trim()}>
              {t('common.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Picks the tags of a chat from the user's tags, or adds a new one. */
function TagsDialog({
  conversation,
  tags,
  open,
  onOpenChange,
}: {
  conversation: ConversationSummary;
  tags: string[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const t = useT();
  const queryClient = useQueryClient();
  const [chosen, setChosen] = useState<string[]>(conversation.tags ?? []);
  const [added, setAdded] = useState<string[]>([]);
  const [draft, setDraft] = useState('');
  const [saving, setSaving] = useState(false);
  const title = conversation.title ?? t('nav.untitled');
  const offered = [...tags, ...added.filter((name) => !tags.some((tag) => tag.toLowerCase() === name.toLowerCase()))];
  const full = chosen.length >= MAX_CHAT_TAGS;

  function toggle(name: string) {
    setChosen((current) => (current.includes(name) ? current.filter((tag) => tag !== name) : [...current, name]));
  }

  function add() {
    const name = draft.trim();
    if (!name || full) {
      return;
    }
    const existing = offered.find((tag) => tag.toLowerCase() === name.toLowerCase());
    if (!existing) {
      setAdded((current) => [...current, name]);
    }
    const spelled = existing ?? name;
    setChosen((current) => (current.includes(spelled) ? current : [...current, spelled]));
    setDraft('');
  }

  async function save() {
    setSaving(true);
    try {
      const updated = await api.conversations.setTags(conversation.id, chosen);
      queryClient.setQueryData<ConversationSummary[]>(queryKeys.conversations, (list) =>
        list?.map((item) => (item.id === updated.id ? { ...item, tags: updated.tags } : item)),
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
          setChosen(conversation.tags ?? []);
          setAdded([]);
          setDraft('');
        }
        onOpenChange(next);
      }}
    >
      <DialogContent>
        <div className="flex flex-col gap-5">
          <DialogHeader>
            <DialogTitle>{t('nav.tags.title', { title })}</DialogTitle>
            <DialogDescription>{t('nav.tags.hint')}</DialogDescription>
          </DialogHeader>
          {offered.length > 0 ? (
            <div className="flex flex-wrap gap-2" role="group" aria-label={t('nav.tags.label')}>
              {offered.map((name) => {
                const on = chosen.includes(name);
                return (
                  <Button
                    key={name}
                    type="button"
                    size="sm"
                    variant={on ? 'primary' : 'outline'}
                    aria-pressed={on}
                    disabled={!on && full}
                    onClick={() => toggle(name)}
                  >
                    {on ? <Check /> : <Tag />}
                    {name}
                  </Button>
                );
              })}
            </div>
          ) : null}
          <form
            className="flex gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              add();
            }}
          >
            <Input
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              maxLength={40}
              placeholder={t('nav.tags.new')}
              aria-label={t('nav.tags.new')}
              autoFocus
            />
            <Button type="submit" variant="outline" disabled={!draft.trim() || full}>
              <Plus />
              {t('nav.tags.add')}
            </Button>
          </form>
          {full ? <p className="text-xs text-muted-foreground">{t('nav.tags.full', { max: MAX_CHAT_TAGS })}</p> : null}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="button" variant="primary" loading={saving} onClick={() => void save()}>
              {t('common.save')}
            </Button>
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function ConversationItem({ conversation, tags }: { conversation: ConversationSummary; tags: string[] }) {
  const t = useT();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const params = useParams({ strict: false }) as { conversationId?: string };
  const [renaming, setRenaming] = useState(false);
  const [tagging, setTagging] = useState(false);
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
          <DropdownMenuItem onSelect={() => setTagging(true)}>
            <Tag />
            {t('nav.tags.edit')}
          </DropdownMenuItem>
          <DropdownMenuItem destructive onSelect={() => setDeleting(true)}>
            <Trash2 />
            {t('common.delete')}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <RenameDialog conversation={conversation} open={renaming} onOpenChange={setRenaming} />
      <TagsDialog conversation={conversation} tags={tags} open={tagging} onOpenChange={setTagging} />
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
