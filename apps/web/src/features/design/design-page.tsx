import { Bell, Copy, Inbox, Moon, Plus, Settings, Sun, Trash2 } from 'lucide-react';
import { type ReactNode, useState } from 'react';
import { toast } from 'sonner';
import { Logo } from '@/components/brand/logo.tsx';
import { Button } from '@/components/ui/button.tsx';
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command.tsx';
import { ConfirmDialog, Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog.tsx';
import { Avatar, Badge, Card, EmptyState, Kbd, MarqueeText, Separator, Skeleton } from '@/components/ui/display.tsx';
import { Field } from '@/components/ui/field.tsx';
import { Input, Textarea } from '@/components/ui/input.tsx';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Popover,
  PopoverContent,
  PopoverTrigger,
  Tooltip,
} from '@/components/ui/menu.tsx';
import { Spinner } from '@/components/ui/spinner.tsx';
import { Switch } from '@/components/ui/switch.tsx';
import { SegmentedControl, Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs.tsx';
import { type Density, usePreferences } from '@/lib/preferences.tsx';

const COLOR_PAIRS: [string, string | null][] = [
  ['background', 'foreground'],
  ['card', 'card-foreground'],
  ['popover', 'popover-foreground'],
  ['primary', 'primary-foreground'],
  ['secondary', 'secondary-foreground'],
  ['muted', 'muted-foreground'],
  ['accent', 'accent-foreground'],
  ['destructive', 'destructive-foreground'],
  ['destructive-subtle', 'destructive'],
  ['success', 'success-foreground'],
  ['warning', 'warning-foreground'],
  ['sidebar', 'sidebar-foreground'],
  ['sidebar-accent', 'sidebar-accent-foreground'],
  ['bubble', 'bubble-foreground'],
  ['tooltip', 'tooltip-foreground'],
  ['border', null],
  ['input', null],
  ['ring', null],
];

function Block({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-4">
      <h2 className="text-base font-semibold text-foreground">{title}</h2>
      <Card className="flex flex-col gap-6 p-6">{children}</Card>
    </section>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <span className="font-mono text-xs text-muted-foreground">{label}</span>
      <div className="flex flex-wrap items-center gap-3">{children}</div>
    </div>
  );
}

/**
 * Development-only gallery of every primitive with its variants and states.
 * The labels of variants are code identifiers and intentionally not translated.
 *
 * @returns The design page.
 */
export function DesignPage() {
  const { t, resolvedTheme, setTheme, density, setDensity } = usePreferences();
  const [checked, setChecked] = useState(true);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [segment, setSegment] = useState<'a' | 'b' | 'c'>('a');

  return (
    <div className="min-h-dvh bg-background">
      <header className="sticky top-0 z-10 border-b border-border bg-background/90 backdrop-blur">
        <div className="mx-auto flex max-w-5xl flex-wrap items-center justify-between gap-4 px-6 py-4">
          <Logo withWordmark />
          <div className="flex flex-wrap items-center gap-3">
            <SegmentedControl<Density>
              label={t('appearance.density')}
              value={density}
              onValueChange={setDensity}
              options={[
                { value: 'compact', label: t('appearance.densityCompact') },
                { value: 'normal', label: t('appearance.densityNormal') },
                { value: 'comfortable', label: t('appearance.densityComfortable') },
              ]}
            />
            <Button
              variant="outline"
              size="icon"
              aria-label={t('appearance.theme')}
              onClick={() => setTheme(resolvedTheme === 'dark' ? 'light' : 'dark')}
            >
              {resolvedTheme === 'dark' ? <Sun /> : <Moon />}
            </Button>
          </div>
        </div>
      </header>

      <main className="mx-auto flex max-w-5xl flex-col gap-10 px-6 py-10">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-semibold tracking-tight">{t('design.title')}</h1>
          <p className="text-sm text-muted-foreground">{t('design.subtitle')}</p>
        </div>

        <Block title={t('design.colors')}>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            {COLOR_PAIRS.map(([bg, fg]) => (
              <div
                key={bg}
                className="flex h-20 flex-col justify-end rounded-lg p-3 shadow-elevation-1"
                style={{ background: `var(--${bg})`, color: fg ? `var(--${fg})` : 'var(--foreground)' }}
              >
                <span className="font-mono text-xs font-medium">{bg}</span>
                {fg ? <span className="font-mono text-xs opacity-80">{fg}</span> : null}
              </div>
            ))}
          </div>
        </Block>

        <Block title={t('design.buttons')}>
          {(['primary', 'secondary', 'outline', 'ghost', 'destructive', 'link'] as const).map((variant) => (
            <Row key={variant} label={`variant="${variant}"`}>
              <Button variant={variant} size="sm">
                {t('common.save')}
              </Button>
              <Button variant={variant}>
                <Plus />
                {t('common.create')}
              </Button>
              <Button variant={variant} size="lg">
                {t('common.save')}
              </Button>
              <Button variant={variant} disabled>
                {t('design.disabled')}
              </Button>
              <Button variant={variant} loading>
                {t('design.loadingState')}
              </Button>
              <Button variant={variant} size="icon" aria-label={t('common.copy')}>
                <Copy />
              </Button>
              <Button variant={variant} size="icon-sm" aria-label={t('common.delete')}>
                <Trash2 />
              </Button>
            </Row>
          ))}
        </Block>

        <Block title={t('design.inputs')}>
          <div className="grid gap-6 sm:grid-cols-2">
            <Field label={t('design.sampleLabel')} hint={t('design.sampleHint')}>
              {(control) => <Input {...control} placeholder={t('design.samplePlaceholder')} />}
            </Field>
            <Field label={t('design.sampleLabel')} error={t('design.sampleError')}>
              {(control) => <Input {...control} defaultValue="…" />}
            </Field>
            <Field label={t('design.disabled')}>{(control) => <Input {...control} disabled defaultValue="read only" />}</Field>
            <Field label={t('design.sampleLabel')} labelSuffix={`(${t('common.optional')})`}>
              {(control) => <Textarea {...control} placeholder={t('chat.placeholder')} />}
            </Field>
          </div>
          <Separator />
          <Row label="Switch">
            <Switch checked={checked} onCheckedChange={setChecked} aria-label="on" />
            <Switch checked={!checked} onCheckedChange={(value) => setChecked(!value)} aria-label="off" />
            <Switch disabled checked aria-label={t('design.disabled')} />
            <Switch disabled aria-label={t('design.disabled')} />
          </Row>
          <Row label="SegmentedControl">
            <SegmentedControl
              label="segments"
              value={segment}
              onValueChange={setSegment}
              options={[
                { value: 'a', label: t('appearance.themeSystem') },
                { value: 'b', label: t('appearance.themeLight') },
                { value: 'c', label: t('appearance.themeDark') },
              ]}
            />
          </Row>
          <Row label="Tabs">
            <Tabs defaultValue="one" className="w-full">
              <TabsList>
                <TabsTrigger value="one">
                  <Inbox />
                  {t('nav.chats')}
                </TabsTrigger>
                <TabsTrigger value="two">
                  <Settings />
                  {t('nav.settings')}
                </TabsTrigger>
                <TabsTrigger value="three" disabled>
                  {t('design.disabled')}
                </TabsTrigger>
              </TabsList>
              <TabsContent value="one" className="pt-3 text-sm text-muted-foreground">
                {t('nav.noChatsHint')}
              </TabsContent>
              <TabsContent value="two" className="pt-3 text-sm text-muted-foreground">
                {t('appearance.description')}
              </TabsContent>
            </Tabs>
          </Row>
        </Block>

        <Block title={t('design.feedback')}>
          <Row label="Badge">
            {(['neutral', 'primary', 'success', 'warning', 'destructive'] as const).map((variant) => (
              <Badge key={variant} variant={variant}>
                {variant}
              </Badge>
            ))}
          </Row>
          <Row label="Avatar · Kbd · Spinner">
            <Avatar name="Ada Lovelace" />
            <Avatar name="grace@example.com" className="size-10" />
            <Kbd>{t('chat.keyEnter')}</Kbd>
            <Kbd>⌘</Kbd>
            <Spinner />
            <Spinner className="size-6 text-primary" />
          </Row>
          <Row label="MarqueeText · group/marquee">
            <Button variant="outline" className="group/marquee w-56 justify-start">
              <MarqueeText text="Example Provider: Example Model 3.2 Experimental (thinking, 128k context)" />
            </Button>
            <Button variant="outline" className="group/marquee w-56 justify-start">
              <MarqueeText text="example-model-small" />
            </Button>
          </Row>
          <Row label="Skeleton">
            <div className="flex w-full max-w-sm flex-col gap-2">
              <Skeleton className="h-4 w-full" />
              <Skeleton className="h-4 w-5/6" />
              <Skeleton className="h-4 w-2/3" />
            </div>
          </Row>
          <Row label="EmptyState">
            <Card className="w-full shadow-elevation-0 ring-1 ring-border">
              <EmptyState
                icon={<Inbox />}
                title={t('nav.noChats')}
                description={t('nav.noChatsHint')}
                action={<Button variant="outline">{t('nav.newChat')}</Button>}
              />
            </Card>
          </Row>
          <Row label="Toast">
            <Button variant="outline" onClick={() => toast(t('design.sampleToast'))}>
              <Bell />
              {t('design.showToast')}
            </Button>
            <Button variant="outline" onClick={() => toast.success(t('providers.created'))}>
              success
            </Button>
            <Button variant="outline" onClick={() => toast.error(t('errors.unknown'))}>
              error
            </Button>
          </Row>
        </Block>

        <Block title={t('design.overlays')}>
          <Row label="Dialog · ConfirmDialog">
            <Dialog>
              <DialogTrigger asChild>
                <Button variant="outline">{t('design.openDialog')}</Button>
              </DialogTrigger>
              <DialogContent>
                <DialogHeader>
                  <DialogTitle>{t('design.title')}</DialogTitle>
                  <DialogDescription>{t('design.sampleDialogBody')}</DialogDescription>
                </DialogHeader>
                <Field label={t('design.sampleLabel')}>{(control) => <Input {...control} autoFocus />}</Field>
                <DialogFooter>
                  <Button variant="ghost">{t('common.cancel')}</Button>
                  <Button variant="primary">{t('common.save')}</Button>
                </DialogFooter>
              </DialogContent>
            </Dialog>
            <Button variant="destructive" onClick={() => setConfirmOpen(true)}>
              {t('common.delete')}
            </Button>
            <ConfirmDialog
              open={confirmOpen}
              onOpenChange={setConfirmOpen}
              title={t('nav.deleteChatTitle')}
              description={t('nav.deleteChatBody', { title: 'Beispiel' })}
              confirmLabel={t('common.delete')}
              destructive
              onConfirm={() => new Promise((resolve) => setTimeout(resolve, 800))}
            />
          </Row>
          <Row label="DropdownMenu · Tooltip · Popover + Command">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline">{t('design.openMenu')}</Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent>
                <DropdownMenuLabel>{t('nav.chats')}</DropdownMenuLabel>
                <DropdownMenuItem>
                  <Copy />
                  {t('common.copy')}
                </DropdownMenuItem>
                <DropdownMenuItem disabled>
                  <Settings />
                  {t('design.disabled')}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem destructive>
                  <Trash2 />
                  {t('common.delete')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <Tooltip content={t('chat.copyMessage')}>
              <Button variant="outline" size="icon" aria-label={t('chat.copyMessage')}>
                <Copy />
              </Button>
            </Tooltip>
            <Popover>
              <PopoverTrigger asChild>
                <Button variant="outline">{t('chat.selectModel')}</Button>
              </PopoverTrigger>
              <PopoverContent align="start" className="w-72 p-0">
                <Command>
                  <CommandInput placeholder={t('chat.searchModels')} />
                  <CommandList>
                    <CommandEmpty>{t('chat.noModels')}</CommandEmpty>
                    <CommandGroup heading="Anthropic">
                      <CommandItem>claude-example-large</CommandItem>
                      <CommandItem>claude-example-small</CommandItem>
                    </CommandGroup>
                    <CommandGroup heading="OpenAI">
                      <CommandItem>gpt-example</CommandItem>
                    </CommandGroup>
                  </CommandList>
                </Command>
              </PopoverContent>
            </Popover>
          </Row>
        </Block>

        <Block title={t('design.surfaces')}>
          <div className="grid gap-4 sm:grid-cols-4">
            {(['shadow-elevation-0', 'shadow-elevation-1', 'shadow-elevation-2', 'shadow-elevation-3'] as const).map((elevation) => (
              <div key={elevation} className={`flex h-24 items-end rounded-xl bg-card p-3 ${elevation}`}>
                <span className="font-mono text-xs text-muted-foreground">{elevation}</span>
              </div>
            ))}
          </div>
          <div className="flex flex-wrap gap-3">
            {(['rounded-xs', 'rounded-sm', 'rounded-md', 'rounded-lg', 'rounded-xl', 'rounded-2xl', 'rounded-full'] as const).map((radius) => (
              <div key={radius} className={`flex size-20 items-end bg-muted p-2 ${radius}`}>
                <span className="font-mono text-xs text-muted-foreground">{radius.replace('rounded-', '')}</span>
              </div>
            ))}
          </div>
        </Block>
      </main>
    </div>
  );
}
