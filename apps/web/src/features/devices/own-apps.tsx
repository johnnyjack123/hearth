import {
  APP_KINDS,
  type AppKind,
  type AppTemplate,
  appNameSchema,
  appPathSchema,
  BUILTIN_APP_NAMES,
  type DeviceSummary,
  domainPatternSchema,
  type UpdateAppTemplateInput,
  validPackage,
} from '@harness/shared';
import { useQueryClient } from '@tanstack/react-query';
import { Ellipsis, Package, Pencil, Plus, Trash2 } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { type FormEvent, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { ConfirmDialog, Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog.tsx';
import { Badge, Card, EmptyState, Separator, Skeleton } from '@/components/ui/display.tsx';
import { Field } from '@/components/ui/field.tsx';
import { Input, Textarea } from '@/components/ui/input.tsx';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/menu.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { springs } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryKeys, useAppTemplates, useDevices } from '@/lib/queries.ts';
import { cn } from '@/lib/utils.ts';
import { Section } from '../shell/page.tsx';

/** Element id of the section, for links from the switches of a project. */
export const OWN_APPS_ID = 'own-programs';

/**
 * The account's own programs (app catalog, step 3): packages from Debian,
 * npm or pip, or an install script. The server hands them to every device
 * of the account; projects switch them on like catalog apps.
 *
 * @returns The section for Settings → Devices.
 */
export function OwnAppsSection() {
  const t = useT();
  const templates = useAppTemplates();
  const [dialog, setDialog] = useState<{ mode: 'create' } | { mode: 'edit'; template: AppTemplate } | null>(null);

  return (
    <div id={OWN_APPS_ID} className="scroll-mt-6">
      <Section title={t('ownApps.title')} description={t('ownApps.description')}>
        <div className="flex justify-end">
          <Button variant="primary" onClick={() => setDialog({ mode: 'create' })}>
            <Plus />
            {t('ownApps.add')}
          </Button>
        </div>
        <Card>
          {templates.isPending ? (
            <div className="flex flex-col gap-3 p-4">
              <Skeleton className="h-10 w-full" />
            </div>
          ) : templates.data?.length ? (
            <ul>
              <AnimatePresence initial={false}>
                {templates.data.map((template, index) => (
                  <motion.li
                    key={template.id}
                    layout="position"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    transition={springs.layout}
                  >
                    {index > 0 ? <Separator /> : null}
                    <TemplateRow template={template} onEdit={() => setDialog({ mode: 'edit', template })} />
                  </motion.li>
                ))}
              </AnimatePresence>
            </ul>
          ) : (
            <EmptyState
              icon={<Package />}
              title={t('ownApps.empty')}
              description={t('ownApps.emptyHint')}
              action={
                <Button variant="outline" onClick={() => setDialog({ mode: 'create' })}>
                  <Plus />
                  {t('ownApps.add')}
                </Button>
              }
            />
          )}
        </Card>
        <OwnAppDialog
          open={dialog !== null}
          onOpenChange={(open) => !open && setDialog(null)}
          template={dialog?.mode === 'edit' ? dialog.template : undefined}
          others={templates.data ?? []}
        />
      </Section>
    </div>
  );
}

function TemplateRow({ template, onEdit }: { template: AppTemplate; onEdit: () => void }) {
  const t = useT();
  const queryClient = useQueryClient();
  const [deleting, setDeleting] = useState(false);
  const what = template.kind === 'script' ? template.description : template.packages.join(', ');

  async function remove() {
    try {
      await api.appTemplates.remove(template.id);
      await queryClient.invalidateQueries({ queryKey: queryKeys.appTemplates });
      toast.success(t('ownApps.removed'));
    } catch (error) {
      toast.error(errorMessage(t, error));
      throw error;
    }
  }

  return (
    <div className="flex items-center gap-4 p-4">
      <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
        <Package className="size-5" aria-hidden="true" />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <span className="truncate text-sm font-medium text-foreground">{template.title}</span>
          <Badge>{t(`ownApps.kinds.${template.kind}`)}</Badge>
        </div>
        <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
          <span className="font-mono">{template.name}</span>
          {what ? <span className="min-w-0 truncate">{what}</span> : null}
        </div>
      </div>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="ghost" size="icon-sm" aria-label={t('common.moreActions')}>
            <Ellipsis />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
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
        title={t('ownApps.removeTitle', { title: template.title })}
        description={t('ownApps.removeBody', { name: template.name })}
        confirmLabel={t('common.delete')}
        destructive
        onConfirm={remove}
      />
    </div>
  );
}

/**
 * A short name from a title: what `harness app` and project files use.
 *
 * @param title - The program's name for people.
 * @returns Lowercase letters, digits and dashes, at most 30 characters.
 */
export function shortName(title: string): string {
  return title
    .toLowerCase()
    .replaceAll('ß', 'ss')
    .normalize('NFKD')
    .replace(/[\u{300}-\u{36f}]/gu, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+/, '')
    .slice(0, 30)
    .replace(/-+$/, '');
}

/** Words of a list field: separated by spaces, commas or lines. */
function words(text: string): string[] {
  return text.split(/[\s,]+/).filter(Boolean);
}

/**
 * Apps a program can require: what the devices of the account report and
 * the other own programs, else the catalog's names.
 */
function requireChoices(devices: DeviceSummary[] | undefined, others: AppTemplate[], self: string): { name: string; title: string }[] {
  const choices = new Map<string, string>();
  for (const device of devices ?? []) {
    for (const app of device.apps?.catalog ?? []) {
      choices.set(app.name, app.title);
    }
  }
  if (choices.size === 0) {
    for (const name of BUILTIN_APP_NAMES) {
      choices.set(name, name);
    }
  }
  for (const other of others) {
    choices.set(other.name, other.title);
  }
  choices.delete(self);
  return [...choices].map(([name, title]) => ({ name, title }));
}

function OwnAppDialog({
  open,
  onOpenChange,
  template,
  others,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  template: AppTemplate | undefined;
  others: AppTemplate[];
}) {
  const t = useT();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xl">
        <DialogHeader>
          <DialogTitle>{template ? t('ownApps.editTitle', { title: template.title }) : t('ownApps.addTitle')}</DialogTitle>
          <DialogDescription>{t('ownApps.dialogDescription')}</DialogDescription>
        </DialogHeader>
        {/* Remount per opening so the form starts from the current program. */}
        {open ? <OwnAppForm template={template} others={others} onDone={() => onOpenChange(false)} /> : null}
      </DialogContent>
    </Dialog>
  );
}

function OwnAppForm({ template, others, onDone }: { template: AppTemplate | undefined; others: AppTemplate[]; onDone: () => void }) {
  const t = useT();
  const queryClient = useQueryClient();
  const devices = useDevices();
  const [kind, setKind] = useState<AppKind>(template?.kind ?? 'apt');
  const [title, setTitle] = useState(template?.title ?? '');
  const [name, setName] = useState(template?.name ?? '');
  const [nameTouched, setNameTouched] = useState(Boolean(template));
  const [description, setDescription] = useState(template?.description ?? '');
  const [packages, setPackages] = useState(template?.packages.join(' ') ?? '');
  const [script, setScript] = useState(template?.script ?? '');
  const [hosts, setHosts] = useState(template?.hosts.join(' ') ?? '');
  const [path, setPath] = useState(template?.path.join(' ') ?? '');
  const [requires, setRequires] = useState<string[]>(template?.requires ?? []);
  const [checked, setChecked] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const choices = requireChoices(devices.data, others, name);
  const packaged = kind !== 'script';
  const badPackages = packaged ? words(packages).filter((word) => !validPackage(kind, word)) : [];
  const badHosts = packaged ? [] : words(hosts).filter((word) => !domainPatternSchema.safeParse(word).success);
  const badPath = packaged ? [] : words(path).filter((word) => !appPathSchema.safeParse(word).success);
  const errors = {
    name: template || appNameSchema.safeParse(name).success ? null : t('ownApps.nameInvalid'),
    packages: !packaged
      ? null
      : badPackages.length > 0
        ? t('ownApps.packagesInvalid', { names: badPackages.join(', ') })
        : words(packages).length === 0
          ? t('ownApps.packagesMissing')
          : null,
    script: packaged || script.trim() ? null : t('ownApps.scriptMissing'),
    hosts: badHosts.length > 0 ? t('ownApps.hostsInvalid', { names: badHosts.join(', ') }) : null,
    path: badPath.length > 0 ? t('ownApps.pathInvalid', { names: badPath.join(', ') }) : null,
  };
  // Shown once the user tried to save, not while typing the first letters.
  const shown = (message: string | null) => (checked ? (message ?? undefined) : undefined);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setChecked(true);
    if (Object.values(errors).some(Boolean)) {
      return;
    }
    setSaving(true);
    setError(null);
    const fields: UpdateAppTemplateInput = {
      title: title.trim(),
      description: description.trim() || null,
      kind,
      packages: packaged ? words(packages) : [],
      script: packaged ? '' : script,
      hosts: packaged ? [] : words(hosts),
      path: packaged ? [] : words(path),
      requires: requires.filter((required) => choices.some((choice) => choice.name === required)),
    };
    try {
      if (template) {
        await api.appTemplates.update(template.id, fields);
        toast.success(t('ownApps.updated'));
      } else {
        await api.appTemplates.create({ name, ...fields });
        toast.success(t('ownApps.created'));
      }
      await queryClient.invalidateQueries({ queryKey: queryKeys.appTemplates });
      // The devices report their new catalog in a moment.
      void queryClient.invalidateQueries({ queryKey: queryKeys.devices });
      onDone();
    } catch (caught) {
      setError(errorMessage(t, caught));
    } finally {
      setSaving(false);
    }
  }

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-col gap-5" noValidate>
      {/* Only the fields scroll; the buttons stay in sight. */}
      <div className="-mx-1 flex max-h-[60vh] flex-col gap-5 overflow-y-auto px-1 pb-1">
        <fieldset className="flex flex-col gap-2">
          <legend className="mb-2 text-sm font-medium text-foreground">{t('ownApps.kind')}</legend>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
            {APP_KINDS.map((candidate) => (
              <label
                key={candidate}
                className={cn(
                  'flex cursor-pointer items-center justify-center rounded-lg border border-border bg-card px-3 py-2.5 text-center text-sm font-medium text-foreground',
                  'transition-[border-color,background-color,box-shadow] duration-(--duration-fast) ease-standard hover:bg-accent',
                  'has-checked:border-primary has-checked:bg-primary/8 has-checked:shadow-elevation-1 has-focus-visible:ring-(length:--focus-ring-width) has-focus-visible:ring-ring',
                )}
              >
                <input
                  type="radio"
                  name="app-kind"
                  value={candidate}
                  checked={kind === candidate}
                  onChange={() => setKind(candidate)}
                  className="sr-only"
                />
                {t(`ownApps.kinds.${candidate}`)}
              </label>
            ))}
          </div>
          <p className="text-xs text-muted-foreground">{t(`ownApps.kindHints.${kind}`)}</p>
        </fieldset>

        <div className="grid gap-5 sm:grid-cols-2">
          <Field label={t('ownApps.titleLabel')}>
            {(control) => (
              <Input
                {...control}
                required
                maxLength={60}
                value={title}
                placeholder={t('ownApps.titlePlaceholder')}
                onChange={(event) => {
                  setTitle(event.target.value);
                  if (!nameTouched) {
                    setName(shortName(event.target.value));
                  }
                }}
              />
            )}
          </Field>
          <Field label={t('ownApps.name')} hint={template ? undefined : t('ownApps.nameHint')} error={shown(errors.name)}>
            {(control) => (
              <Input
                {...control}
                required
                maxLength={30}
                spellCheck={false}
                disabled={Boolean(template)}
                value={name}
                onChange={(event) => {
                  setName(event.target.value);
                  setNameTouched(true);
                }}
                className="font-mono"
              />
            )}
          </Field>
        </div>

        {packaged ? (
          <Field label={t('ownApps.packages')} hint={t('ownApps.packagesHint')} error={shown(errors.packages)}>
            {(control) => (
              <Input
                {...control}
                spellCheck={false}
                autoCapitalize="off"
                value={packages}
                placeholder={kind === 'apt' ? 'ffmpeg imagemagick' : kind === 'npm' ? 'typescript' : 'ruff yt-dlp'}
                onChange={(event) => setPackages(event.target.value)}
                className="font-mono"
              />
            )}
          </Field>
        ) : (
          <>
            <Field label={t('ownApps.script')} error={shown(errors.script)}>
              {(control) => (
                <Textarea
                  {...control}
                  spellCheck={false}
                  autoCapitalize="off"
                  rows={8}
                  value={script}
                  placeholder={SCRIPT_EXAMPLE}
                  onChange={(event) => setScript(event.target.value)}
                  className="max-h-80 font-mono text-xs"
                />
              )}
            </Field>
            <div className="grid gap-5 sm:grid-cols-2">
              <Field label={t('ownApps.hosts')} labelSuffix={`(${t('common.optional')})`} hint={t('ownApps.hostsHint')} error={shown(errors.hosts)}>
                {(control) => (
                  <Input
                    {...control}
                    spellCheck={false}
                    autoCapitalize="off"
                    value={hosts}
                    placeholder="github.com"
                    onChange={(event) => setHosts(event.target.value)}
                    className="font-mono"
                  />
                )}
              </Field>
              <Field label={t('ownApps.path')} labelSuffix={`(${t('common.optional')})`} hint={t('ownApps.pathHint')} error={shown(errors.path)}>
                {(control) => (
                  <Input
                    {...control}
                    spellCheck={false}
                    autoCapitalize="off"
                    value={path}
                    placeholder="bin"
                    onChange={(event) => setPath(event.target.value)}
                    className="font-mono"
                  />
                )}
              </Field>
            </div>
          </>
        )}

        <Field label={t('ownApps.descriptionLabel')} labelSuffix={`(${t('common.optional')})`}>
          {(control) => <Input {...control} maxLength={300} value={description} onChange={(event) => setDescription(event.target.value)} />}
        </Field>

        {choices.length > 0 ? (
          <fieldset className="flex flex-col gap-2">
            <legend className="mb-2 text-sm font-medium text-foreground">
              {t('ownApps.requires')} <span className="font-normal text-muted-foreground">({t('common.optional')})</span>
            </legend>
            <div className="flex flex-wrap gap-2">
              {choices.map((choice) => (
                <label
                  key={choice.name}
                  className={cn(
                    'flex cursor-pointer items-center rounded-md border border-border bg-card px-2.5 py-1 text-xs font-medium text-foreground pointer-coarse:min-h-11',
                    'transition-[border-color,background-color] duration-(--duration-fast) ease-standard hover:bg-accent',
                    'has-checked:border-primary has-checked:bg-primary/8 has-focus-visible:ring-(length:--focus-ring-width) has-focus-visible:ring-ring',
                  )}
                >
                  <input
                    type="checkbox"
                    checked={requires.includes(choice.name)}
                    onChange={(event) =>
                      setRequires((current) =>
                        event.target.checked ? [...current, choice.name] : current.filter((required) => required !== choice.name),
                      )
                    }
                    className="sr-only"
                  />
                  {choice.title}
                </label>
              ))}
            </div>
            <p className="text-xs text-muted-foreground">{t('ownApps.requiresHint')}</p>
          </fieldset>
        ) : null}

        {error ? (
          <p role="alert" className="rounded-lg bg-destructive-subtle p-3 text-sm text-destructive">
            {error}
          </p>
        ) : null}
      </div>

      <DialogFooter>
        <Button type="button" variant="ghost" onClick={onDone}>
          {t('common.cancel')}
        </Button>
        <Button type="submit" variant="primary" loading={saving}>
          {t('common.save')}
        </Button>
      </DialogFooter>
    </form>
  );
}

/** Placeholder of the script field: the usual shape of an install. */
const SCRIPT_EXAMPLE = [
  'harness_download https://example.com/tool-$ARCH.tar.gz "$WORK/tool.tar.gz"',
  'mkdir -p "$PREFIX/bin"',
  'tar -xzf "$WORK/tool.tar.gz" -C "$PREFIX/bin"',
  'harness_version "$("$PREFIX/bin/tool" --version)"',
].join('\n');
