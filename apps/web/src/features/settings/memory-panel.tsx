import { MAX_MEMORIES, MAX_MEMORY_CHARS, type Memory } from '@harness/shared';
import { useQueryClient } from '@tanstack/react-query';
import { BookMarked, Check, Pencil, Trash2, X } from 'lucide-react';
import { type FormEvent, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Card, EmptyState, Skeleton } from '@/components/ui/display.tsx';
import { Input } from '@/components/ui/input.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { usePreferences } from '@/lib/preferences.tsx';
import { queryKeys, useMemories } from '@/lib/queries.ts';
import { Section } from '../shell/page.tsx';

/**
 * The user's memories: what every chat and agent is told. Models add to it
 * when the user asks them to; here the user adds, changes and removes.
 *
 * @returns The memory panel.
 */
export function MemoryPanel() {
  const { t, locale } = usePreferences();
  const queryClient = useQueryClient();
  const memories = useMemories();
  const [draft, setDraft] = useState('');
  const [saving, setSaving] = useState(false);
  const refresh = () => queryClient.invalidateQueries({ queryKey: queryKeys.memories });

  async function add(event: FormEvent) {
    event.preventDefault();
    if (!draft.trim()) {
      return;
    }
    setSaving(true);
    try {
      await api.memories.create({ text: draft.trim() });
      setDraft('');
      await refresh();
    } catch (error) {
      toast.error(errorMessage(t, error));
    } finally {
      setSaving(false);
    }
  }

  const list = memories.data ?? [];
  const date = new Intl.DateTimeFormat(locale, { dateStyle: 'medium' });
  return (
    <Section title={t('memory.title')} description={t('memory.description')}>
      <div className="flex flex-col gap-4">
        <form onSubmit={(event) => void add(event)} className="flex gap-2">
          <Input
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            maxLength={MAX_MEMORY_CHARS}
            placeholder={t('memory.placeholder')}
            aria-label={t('memory.placeholder')}
            className="min-w-0 flex-1"
          />
          <Button type="submit" variant="primary" loading={saving} disabled={!draft.trim() || list.length >= MAX_MEMORIES}>
            {t('memory.add')}
          </Button>
        </form>
        {memories.isPending ? (
          <Skeleton className="h-32 w-full rounded-xl" />
        ) : list.length === 0 ? (
          <Card className="p-2">
            <EmptyState icon={<BookMarked />} title={t('memory.emptyTitle')} description={t('memory.emptyBody')} />
          </Card>
        ) : (
          <Card className="divide-y divide-border">
            {list.map((memory) => (
              <MemoryRow key={memory.id} memory={memory} date={date.format(memory.createdAt)} onChanged={refresh} />
            ))}
          </Card>
        )}
        <p className="text-xs text-muted-foreground">{t('memory.count', { count: list.length, max: MAX_MEMORIES })}</p>
      </div>
    </Section>
  );
}

function MemoryRow({ memory, date, onChanged }: { memory: Memory; date: string; onChanged: () => Promise<unknown> }) {
  const { t } = usePreferences();
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(memory.text);
  const [busy, setBusy] = useState(false);

  async function run(action: () => Promise<unknown>) {
    setBusy(true);
    try {
      await action();
      await onChanged();
      setEditing(false);
    } catch (error) {
      toast.error(errorMessage(t, error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex min-w-0 items-start gap-3 px-4 py-3">
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        {editing ? (
          <form
            className="flex gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              void run(() => api.memories.update(memory.id, { text: text.trim() }));
            }}
          >
            <Input
              value={text}
              onChange={(event) => setText(event.target.value)}
              maxLength={MAX_MEMORY_CHARS}
              aria-label={t('memory.edit')}
              className="min-w-0 flex-1"
              autoFocus
            />
            <Button type="submit" variant="ghost" size="icon-sm" aria-label={t('common.save')} loading={busy} disabled={!text.trim()}>
              <Check />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={t('common.cancel')}
              onClick={() => {
                setText(memory.text);
                setEditing(false);
              }}
            >
              <X />
            </Button>
          </form>
        ) : (
          <p className="break-words text-sm text-foreground">{memory.text}</p>
        )}
        <p className="text-xs text-muted-foreground">{t(`memory.source.${memory.source}`, { date })}</p>
      </div>
      {editing ? null : (
        <div className="flex shrink-0 gap-1">
          <Button variant="ghost" size="icon-sm" aria-label={t('memory.edit')} onClick={() => setEditing(true)}>
            <Pencil />
          </Button>
          <Button variant="ghost" size="icon-sm" aria-label={t('memory.remove')} loading={busy} onClick={() => void run(() => api.memories.remove(memory.id))}>
            <Trash2 />
          </Button>
        </div>
      )}
    </div>
  );
}
