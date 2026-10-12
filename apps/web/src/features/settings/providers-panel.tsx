import { PROVIDERS, type ProviderCredential } from '@harness/shared';
import { useQueryClient } from '@tanstack/react-query';
import { Ellipsis, KeyRound, Pencil, Plus, Server, Trash2, Zap } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { ConfirmDialog } from '@/components/ui/dialog.tsx';
import { Badge, Card, EmptyState, Separator, Skeleton } from '@/components/ui/display.tsx';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { isAdmin } from '@/lib/auth-client.ts';
import { springs } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { providerName } from '@/lib/providers.ts';
import { queryKeys, useProviders, usePublicSettings, useSession } from '@/lib/queries.ts';
import { Section } from '../shell/page.tsx';
import { ProviderDialog } from './provider-dialog.tsx';
import { testCredential } from './test-credential.ts';

type DialogState = { mode: 'create' } | { mode: 'edit'; credential: ProviderCredential } | null;

/**
 * Lists the user's provider credentials and lets them add, edit, test and
 * remove them.
 *
 * @returns The providers panel.
 */
export function ProvidersPanel() {
  const t = useT();
  const providers = useProviders();
  const settings = usePublicSettings();
  const { data: session } = useSession();
  const [dialog, setDialog] = useState<DialogState>(null);
  const customAllowed = isAdmin(session?.user) || settings.data?.allowCustomBaseUrls === true;

  return (
    <Section title={t('providers.title')} description={t('providers.description')}>
      <div className="flex justify-end">
        <Button variant="primary" onClick={() => setDialog({ mode: 'create' })}>
          <Plus />
          {t('providers.add')}
        </Button>
      </div>
      <Card>
        {providers.isPending ? (
          <div className="flex flex-col gap-3 p-4">
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
          </div>
        ) : providers.data?.length ? (
          <ul>
            <AnimatePresence initial={false}>
              {providers.data.map((credential, index) => (
                <motion.li
                  key={credential.id}
                  layout="position"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  exit={{ opacity: 0 }}
                  transition={springs.layout}
                >
                  {index > 0 ? <Separator /> : null}
                  <CredentialRow credential={credential} onEdit={() => setDialog({ mode: 'edit', credential })} />
                </motion.li>
              ))}
            </AnimatePresence>
          </ul>
        ) : (
          <EmptyState
            icon={<KeyRound />}
            title={t('providers.empty')}
            description={t('providers.emptyHint')}
            action={
              <Button variant="outline" onClick={() => setDialog({ mode: 'create' })}>
                <Plus />
                {t('providers.add')}
              </Button>
            }
          />
        )}
      </Card>
      <ProviderDialog
        open={dialog !== null}
        onOpenChange={(open) => !open && setDialog(null)}
        credential={dialog?.mode === 'edit' ? dialog.credential : undefined}
        customAllowed={customAllowed}
      />
    </Section>
  );
}

function CredentialRow({ credential, onEdit }: { credential: ProviderCredential; onEdit: () => void }) {
  const t = useT();
  const queryClient = useQueryClient();
  const [testing, setTesting] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const info = PROVIDERS[credential.kind];
  const Icon = info.userDefinedEndpoint ? Server : KeyRound;

  async function test() {
    setTesting(true);
    await testCredential(t, credential);
    await queryClient.invalidateQueries({ queryKey: queryKeys.models });
    setTesting(false);
  }

  async function remove() {
    try {
      await api.providers.remove(credential.id);
      await queryClient.invalidateQueries({ queryKey: queryKeys.providers });
      await queryClient.invalidateQueries({ queryKey: queryKeys.models });
      toast.success(t('providers.deleted'));
    } catch (error) {
      toast.error(errorMessage(t, error));
      throw error;
    }
  }

  return (
    <div className="flex items-center gap-4 p-4">
      <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
        <Icon className="size-5" aria-hidden="true" />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="truncate text-sm font-medium text-foreground">{credential.label}</span>
          <Badge>{providerName(t, credential.kind)}</Badge>
        </div>
        <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
          <span>{credential.keyHint ? t('providers.keyStored', { hint: credential.keyHint }) : t('providers.noKey')}</span>
          {credential.baseUrl ? <span className="truncate font-mono">{credential.baseUrl}</span> : null}
        </div>
      </div>
      <Button variant="outline" size="sm" loading={testing} onClick={() => void test()} className="hidden sm:inline-flex">
        {testing ? null : <Zap />}
        {t('providers.test')}
      </Button>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label={t('common.moreActions')}>
            <Ellipsis />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem className="sm:hidden" onSelect={() => void test()}>
            <Zap />
            {t('providers.test')}
          </DropdownMenuItem>
          <DropdownMenuItem onSelect={onEdit}>
            <Pencil />
            {t('common.edit')}
          </DropdownMenuItem>
          <DropdownMenuItem destructive onSelect={() => setDeleting(true)}>
            <Trash2 />
            {t('common.delete')}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <ConfirmDialog
        open={deleting}
        onOpenChange={setDeleting}
        title={t('providers.deleteTitle')}
        description={t('providers.deleteBody', { label: credential.label })}
        confirmLabel={t('common.delete')}
        destructive
        onConfirm={remove}
      />
    </div>
  );
}
