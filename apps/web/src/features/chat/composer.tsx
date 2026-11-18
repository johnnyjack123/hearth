import { ACCEPTED_FILE_EXTENSIONS } from '@hearth/shared';
import type { FileUIPart } from 'ai';
import { ArrowUp, Camera, FileText, Images, Paperclip, Square } from 'lucide-react';
import {
  type ChangeEvent,
  type ClipboardEvent,
  type DragEvent,
  type FormEvent,
  type KeyboardEvent,
  type ReactNode,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import { Button } from '@/components/ui/button.tsx';
import { Kbd } from '@/components/ui/display.tsx';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Tooltip,
} from '@/components/ui/menu.tsx';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';
import { AttachmentList, useAttachments } from './attachments.tsx';

/**
 * Props of {@link Composer}.
 */
export interface ComposerProps {
  /** Current text. */
  value: string;
  /** Called on every edit. */
  onValueChange: (value: string) => void;
  /** Called with the trimmed text and the uploaded files when the user sends. */
  onSubmit: (text: string, files: FileUIPart[]) => void;
  /** Called when the user stops a running response. */
  onStop?: () => void;
  /** A response is being generated; the send button becomes a stop button. */
  busy?: boolean;
  /** Reason sending is impossible (e.g. no model); shown as tooltip on the send button. */
  blockedReason?: string | null;
  /** Controls rendered in the bottom-left corner, e.g. the model picker. */
  toolbar?: ReactNode;
  /** Status shown next to the send button, e.g. the context meter. */
  status?: ReactNode;
  /** Focus the input on mount. */
  autoFocus?: boolean;
  /** Placeholder instead of the default one, e.g. for a linked agent. */
  placeholder?: string;
  /**
   * Files can be attached; `images` says whether the chosen model reads
   * images (undefined when unknown). Missing: no attachments.
   */
  files?: { images: boolean | undefined };
}

/**
 * Chat input: auto-growing textarea, toolbar and send/stop button.
 * Enter sends, Shift+Enter inserts a newline, IME composition is respected.
 *
 * @param props - See {@link ComposerProps}.
 * @returns The composer form.
 */
export function Composer({
  value,
  onValueChange,
  onSubmit,
  onStop,
  busy = false,
  blockedReason,
  toolbar,
  status,
  autoFocus,
  placeholder,
  files,
}: ComposerProps) {
  const t = useT();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const pickerRef = useRef<HTMLInputElement>(null);
  const photosRef = useRef<HTMLInputElement>(null);
  const cameraRef = useRef<HTMLInputElement>(null);
  const attachments = useAttachments();
  const [dragging, setDragging] = useState(false);
  // A blind model gets no images; the user removes them or picks another model.
  const blocked =
    blockedReason ??
    (attachments.uploading ? t('files.waitUpload') : files?.images === false && attachments.hasImages ? t('files.imagesUnsupported') : null);
  const canSend = (value.trim().length > 0 || attachments.parts.length > 0) && !blocked && !busy;

  // Grow with the content; the max height comes from the max-h class.
  // Measured without a scrollbar: with one (classic scrollbars on Windows)
  // the text wrapped earlier while measuring, and the box kept an empty
  // line once the scrollbar was gone again.
  useLayoutEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) {
      return;
    }
    textarea.style.overflowY = 'hidden';
    textarea.style.height = 'auto';
    const height = textarea.scrollHeight;
    textarea.style.height = `${height}px`;
    // It scrolls only once the content is taller than the max height.
    textarea.style.overflowY = height > textarea.clientHeight ? 'auto' : 'hidden';
  }, [value]);

  function submit(event?: FormEvent) {
    event?.preventDefault();
    if (!canSend) {
      return;
    }
    onSubmit(value.trim(), attachments.parts);
    attachments.clear();
  }

  function addPicked(event: ChangeEvent<HTMLInputElement>) {
    attachments.add([...(event.target.files ?? [])]);
    // The same file again must fire another change.
    event.target.value = '';
  }

  function onPaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    // Text stays text (copying from a page or Word brings an image along);
    // a screenshot or copied files become attachments.
    const pasted = [...event.clipboardData.files];
    if (!files || pasted.length === 0 || event.clipboardData.types.includes('text/plain')) {
      return;
    }
    event.preventDefault();
    attachments.add(pasted);
  }

  function onDrop(event: DragEvent<HTMLFormElement>) {
    setDragging(false);
    if (!files || event.dataTransfer.files.length === 0) {
      return;
    }
    event.preventDefault();
    attachments.add([...event.dataTransfer.files]);
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      submit();
    }
  }

  const sendButton = (
    <Button
      type="submit"
      variant="primary"
      size="icon-sm"
      className="rounded-full"
      aria-label={t('chat.send')}
      aria-disabled={!canSend || undefined}
      onClick={(event) => {
        if (!canSend) {
          event.preventDefault();
        }
      }}
    >
      <ArrowUp />
    </Button>
  );

  return (
    <form
      onSubmit={submit}
      onDragOver={(event) => {
        if (files && event.dataTransfer.types.includes('Files')) {
          event.preventDefault();
          setDragging(true);
        }
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
          setDragging(false);
        }
      }}
      onDrop={onDrop}
      className={cn(
        'flex flex-col rounded-2xl bg-card shadow-elevation-2 ring-1 ring-border',
        'transition-shadow duration-(--duration-fast) ease-standard focus-within:ring-(length:--focus-ring-width) focus-within:ring-ring/40',
        dragging && 'ring-(length:--focus-ring-width) ring-ring',
      )}
    >
      {files ? <AttachmentList items={attachments.items} onRemove={attachments.remove} /> : null}
      <textarea
        ref={textareaRef}
        value={value}
        onChange={(event) => onValueChange(event.target.value)}
        onKeyDown={onKeyDown}
        onPaste={onPaste}
        placeholder={placeholder ?? t('chat.placeholder')}
        aria-label={placeholder ?? t('chat.placeholder')}
        rows={1}
        autoFocus={autoFocus}
        className="max-h-60 min-h-12 w-full resize-none bg-transparent px-4 pt-3.5 pb-2 text-base text-foreground outline-none placeholder:text-muted-foreground"
      />
      <div className="flex items-center gap-2 px-2 pb-2">
        <div className="flex min-w-0 flex-1 items-center gap-1">
          {files ? (
            <>
              <Tooltip content={files.images === false ? t('files.attachNoImages') : t('files.attachHint')}>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  className="shrink-0 text-muted-foreground hover:text-foreground pointer-coarse:hidden"
                  aria-label={t('files.attach')}
                  onClick={() => pickerRef.current?.click()}
                >
                  <Paperclip />
                </Button>
              </Tooltip>
              {/* Android opens its photo picker only for an input that takes
                  images alone; one that also takes documents goes straight to
                  the file manager. Touch screens therefore get a choice. */}
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    className="hidden shrink-0 text-muted-foreground hover:text-foreground pointer-coarse:inline-flex"
                    aria-label={t('files.attach')}
                  >
                    <Paperclip />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" side="top">
                  <DropdownMenuItem disabled={files.images === false} onSelect={() => photosRef.current?.click()}>
                    <Images />
                    {t('files.photos')}
                  </DropdownMenuItem>
                  <DropdownMenuItem disabled={files.images === false} onSelect={() => cameraRef.current?.click()}>
                    <Camera />
                    {t('files.camera')}
                  </DropdownMenuItem>
                  <DropdownMenuItem onSelect={() => pickerRef.current?.click()}>
                    <FileText />
                    {t('files.documents')}
                  </DropdownMenuItem>
                  {/* Touch screens show no tooltip: the menu itself says why
                      photos are greyed out. */}
                  {files.images === false ? (
                    <>
                      <DropdownMenuSeparator />
                      <DropdownMenuLabel data-no-images className="max-w-60 font-normal">
                        {t('files.noImagesHint')}
                      </DropdownMenuLabel>
                    </>
                  ) : null}
                </DropdownMenuContent>
              </DropdownMenu>
              <input ref={pickerRef} type="file" multiple hidden accept={[...ACCEPTED_FILE_EXTENSIONS, 'image/*'].join(',')} onChange={addPicked} />
              <input ref={photosRef} type="file" multiple hidden accept="image/*" onChange={addPicked} />
              <input ref={cameraRef} type="file" hidden accept="image/*" capture="environment" onChange={addPicked} />
            </>
          ) : null}
          {toolbar}
        </div>
        {status}
        {busy ? (
          <Button type="button" variant="secondary" size="icon-sm" className="rounded-full" aria-label={t('chat.stop')} onClick={onStop}>
            <Square className="size-3.5 fill-current" />
          </Button>
        ) : blocked ? (
          <Tooltip content={blocked}>{sendButton}</Tooltip>
        ) : (
          sendButton
        )}
      </div>
    </form>
  );
}

/**
 * Keyboard hint and disclaimer below the composer; hidden on touch devices.
 *
 * @returns The hint line.
 */
export function ComposerHint() {
  const t = useT();
  return (
    <p className="mt-2 flex flex-wrap items-center justify-center gap-x-3 gap-y-1 text-center text-xs text-muted-foreground pointer-coarse:hidden">
      <span className="inline-flex items-center gap-1">
        <Kbd>{t('chat.keyEnter')}</Kbd>
        {t('chat.hintSend')}
      </span>
      <span className="inline-flex items-center gap-1">
        <Kbd>{t('chat.keyShift')}</Kbd>+<Kbd>{t('chat.keyEnter')}</Kbd>
        {t('chat.hintNewline')}
      </span>
      <span aria-hidden="true">·</span>
      <span>{t('chat.disclaimer')}</span>
    </p>
  );
}
