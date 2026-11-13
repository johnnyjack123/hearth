import type { CredentialModels, ModelSelection } from '@hearth/shared';
import { Link } from '@tanstack/react-router';
import { Check, ChevronDown, CircleAlert, Settings2, Sparkles } from 'lucide-react';
import { useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command.tsx';
import { MarqueeText, Skeleton } from '@/components/ui/display.tsx';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/menu.tsx';
import { useT } from '@/lib/preferences.tsx';
import { providerName } from '@/lib/providers.ts';

/**
 * Props of {@link ModelPicker}.
 */
export interface ModelPickerProps {
  /** Current selection. */
  selection: ModelSelection | null;
  /** Called with the chosen model. */
  onSelect: (selection: ModelSelection) => void;
  /** Models per credential, undefined while loading. */
  groups: CredentialModels[] | undefined;
  /** True while the model list loads. */
  loading: boolean;
  /** Models of the latest chats, offered first. */
  recent?: ModelSelection[];
}

/**
 * A model's name without the vendor some catalogs put in front, as in
 * OpenRouter's "DeepSeek: DeepSeek V4.1 Flash".
 *
 * @param name - The catalog's name.
 * @returns The name without the prefix, or as it was.
 */
export function withoutVendor(name: string): string {
  const stripped = name.replace(/^[^:]{1,40}:\s+/, '');
  return stripped || name;
}

/**
 * Searchable model dropdown grouped by credential, shown inside the composer.
 *
 * @param props - See {@link ModelPickerProps}.
 * @returns The picker.
 */
export function ModelPicker({ selection, onSelect, groups, loading, recent = [] }: ModelPickerProps) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const choose = (next: ModelSelection) => {
    onSelect(next);
    setOpen(false);
  };
  const selectedGroup = groups?.find((group) => group.credentialId === selection?.credentialId);
  const selectedModel = selectedGroup?.models.find((model) => model.id === selection?.modelId);
  // The key it runs over instead of the vendor OpenRouter puts in front:
  // "OpenRouter: DeepSeek V4.1 Flash" rather than "DeepSeek: DeepSeek V4.1 Flash".
  const label =
    selectedGroup && selection
      ? `${selectedGroup.label}: ${withoutVendor(selectedModel?.name ?? selection.modelId)}`
      : (selection?.modelId ?? t('chat.selectModel'));

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" className="group/marquee min-w-0 max-w-52 shrink gap-1.5 px-2 text-muted-foreground hover:text-foreground">
          <Sparkles className="text-primary" />
          <MarqueeText text={label} autoplay />
          <ChevronDown className="opacity-60" />
        </Button>
      </PopoverTrigger>
      <PopoverContent align="start" side="top" className="w-[min(var(--spacing)*88,calc(100vw-var(--spacing)*8))] p-0">
        <Command loop defaultValue={selection ? `${selection.credentialId}::${selection.modelId}` : undefined}>
          <CommandInput placeholder={t('chat.searchModels')} />
          <CommandList>
            {loading ? (
              <div className="flex flex-col gap-2 p-3" aria-label={t('chat.modelsLoading')}>
                <Skeleton className="h-4 w-2/3" />
                <Skeleton className="h-4 w-1/2" />
                <Skeleton className="h-4 w-3/4" />
              </div>
            ) : (
              <CommandEmpty>{t('chat.noModels')}</CommandEmpty>
            )}
            {!loading && recent.length > 0 ? (
              <CommandGroup heading={t('chat.recentModels')}>
                {recent.map((entry) => {
                  const group = groups?.find((candidate) => candidate.credentialId === entry.credentialId);
                  const model = group?.models.find((candidate) => candidate.id === entry.modelId);
                  if (!group || !model) {
                    return null;
                  }
                  const selected = selection?.credentialId === entry.credentialId && selection.modelId === entry.modelId;
                  return (
                    <CommandItem
                      key={`${entry.credentialId}::${entry.modelId}`}
                      className="group/marquee"
                      // Its own value: the same model also appears in its credential's group.
                      value={`recent::${entry.credentialId}::${entry.modelId}`}
                      keywords={[model.name, model.id, group.label]}
                      onSelect={() => choose(entry)}
                    >
                      <MarqueeText text={model.name} wrapOnTouch className="flex-1" />
                      <MarqueeText text={group.label} wrapOnTouch className="max-w-2/5 text-xs text-muted-foreground" />
                      <Check className={selected ? 'text-primary' : 'invisible'} aria-hidden="true" />
                    </CommandItem>
                  );
                })}
              </CommandGroup>
            ) : null}
            {groups?.map((group) => (
              <CommandGroup key={group.credentialId} heading={`${group.label} · ${providerName(t, group.kind)}`}>
                {group.error ? (
                  <div className="flex items-start gap-2 px-2 py-1.5 text-xs text-destructive">
                    <CircleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
                    <span>
                      {t('chat.modelsError', { label: group.label })}
                      <span className="block text-muted-foreground">{group.error}</span>
                    </span>
                  </div>
                ) : null}
                {group.models.map((model) => {
                  const selected = selection?.credentialId === group.credentialId && selection.modelId === model.id;
                  return (
                    <CommandItem
                      key={model.id}
                      className="group/marquee"
                      value={`${group.credentialId}::${model.id}`}
                      keywords={[model.name, model.id, group.label]}
                      onSelect={() => choose({ credentialId: group.credentialId, modelId: model.id })}
                    >
                      <MarqueeText text={model.name} wrapOnTouch className="flex-1" />
                      {model.name !== model.id ? (
                        <MarqueeText text={model.id} wrapOnTouch className="max-w-2/5 font-mono text-xs text-muted-foreground" />
                      ) : null}
                      <Check className={selected ? 'text-primary' : 'invisible'} aria-hidden="true" />
                    </CommandItem>
                  );
                })}
              </CommandGroup>
            ))}
          </CommandList>
          <div className="border-t border-border p-1">
            <Button variant="ghost" size="sm" className="w-full justify-start text-muted-foreground" asChild>
              <Link to="/settings" search={{ tab: 'providers' }} onClick={() => setOpen(false)}>
                <Settings2 />
                {t('chat.manageProviders')}
              </Link>
            </Button>
          </div>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
