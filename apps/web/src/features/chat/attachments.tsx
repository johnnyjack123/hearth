import { type ChatFile, chatFileId, chatFileUrl, MAX_FILE_BYTES, MAX_FILES_PER_MESSAGE } from '@harness/shared';
import type { FileUIPart } from 'ai';
import { CircleAlert, FileText, X } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Tooltip } from '@/components/ui/menu.tsx';
import { Spinner } from '@/components/ui/spinner.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { usePreferences, useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';

/** Longest side of an image the browser sends; models scale down further anyway. */
const MAX_IMAGE_SIDE = 2048;

/** Images up to this size and side go as they are. */
const MAX_IMAGE_BYTES = 4 * 1024 * 1024;

/** A file in the composer, before the message goes out. */
export interface Attachment {
  /** Local key for the list. */
  key: string;
  /** File name. */
  name: string;
  /** Preview of an image (object URL), revoked when the attachment goes. */
  preview: string | null;
  /** Whether it is an image, by the browser's guess; the server decides for real. */
  image: boolean;
  /** Upload state. */
  state: 'uploading' | 'ready' | 'failed';
  /** The stored file, once uploaded. */
  file?: ChatFile;
  /** Why the upload failed. */
  error?: string;
}

/**
 * Formats a file size.
 *
 * @param locale - UI language.
 * @param bytes - Size in bytes.
 * @returns E.g. `820 KB` or `1,4 MB`.
 */
export function formatBytes(locale: string, bytes: number): string {
  if (bytes < 1024 * 1024) {
    return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(Math.max(1, bytes / 1024))} KB`;
  }
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(bytes / 1024 / 1024)} MB`;
}

/**
 * Scales a large image down before the upload: models read at most about
 * 1500 to 2000 pixels per side, so more only costs bandwidth and storage.
 * Screenshots stay PNG (sharp text), photos become JPEG; GIFs stay as they
 * are (animation).
 *
 * @param file - The picked or pasted file.
 * @returns The bytes to upload and their name.
 */
async function prepareImage(file: File): Promise<{ blob: Blob; name: string }> {
  if (!file.type.startsWith('image/') || file.type === 'image/gif') {
    return { blob: file, name: file.name };
  }
  const bitmap = await createImageBitmap(file).catch(() => null);
  if (!bitmap) {
    return { blob: file, name: file.name };
  }
  const scale = Math.min(1, MAX_IMAGE_SIDE / Math.max(bitmap.width, bitmap.height));
  if (scale === 1 && file.size <= MAX_IMAGE_BYTES) {
    bitmap.close();
    return { blob: file, name: file.name };
  }
  const canvas = document.createElement('canvas');
  canvas.width = Math.round(bitmap.width * scale);
  canvas.height = Math.round(bitmap.height * scale);
  canvas.getContext('2d')?.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  const encode = (type: string) => new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, type, 0.85));
  let blob = file.type === 'image/png' ? await encode('image/png') : null;
  let name = file.name;
  if (!blob || blob.size > MAX_IMAGE_BYTES) {
    blob = await encode('image/jpeg');
    name = `${file.name.replace(/\.[^.]+$/, '') || 'bild'}.jpg`;
  }
  return blob ? { blob, name } : { blob: file, name: file.name };
}

/**
 * The files of the composer: each one uploads as soon as it is added, so
 * sending only refers to them.
 *
 * @returns The list and what to do with it.
 */
export function useAttachments() {
  const t = useT();
  const [items, setItems] = useState<Attachment[]>([]);
  const counter = useRef(0);
  const live = useRef(items);
  live.current = items;

  useEffect(
    () => () => {
      for (const item of live.current) {
        if (item.preview) {
          URL.revokeObjectURL(item.preview);
        }
      }
    },
    [],
  );

  const update = useCallback((key: string, change: Partial<Attachment>) => {
    setItems((current) => current.map((item) => (item.key === key ? { ...item, ...change } : item)));
  }, []);

  const add = useCallback(
    (files: File[]) => {
      const room = MAX_FILES_PER_MESSAGE - live.current.length;
      if (files.length > room) {
        toast.warning(t('files.tooMany', { count: MAX_FILES_PER_MESSAGE }));
      }
      for (const file of files.slice(0, Math.max(0, room))) {
        if (file.size > MAX_FILE_BYTES && !file.type.startsWith('image/')) {
          toast.error(t('errors.file_too_large'));
          continue;
        }
        counter.current += 1;
        const key = `a${counter.current}`;
        const image = file.type.startsWith('image/');
        const name = file.name || t('files.pasted');
        setItems((current) => [...current, { key, name, image, preview: image ? URL.createObjectURL(file) : null, state: 'uploading' }]);
        void prepareImage(new File([file], name, { type: file.type }))
          .then(({ blob, name: sent }) => api.files.upload(blob, sent))
          .then((stored) => update(key, { state: 'ready', file: stored, name: stored.name, image: stored.kind === 'image' }))
          .catch((error: unknown) => update(key, { state: 'failed', error: errorMessage(t, error) }));
      }
    },
    [t, update],
  );

  const remove = useCallback((key: string) => {
    setItems((current) => {
      const gone = current.find((item) => item.key === key);
      if (gone?.preview) {
        URL.revokeObjectURL(gone.preview);
      }
      return current.filter((item) => item.key !== key);
    });
  }, []);

  /** Empties the list after sending; the message shows the stored files. */
  const clear = useCallback(() => {
    for (const item of live.current) {
      if (item.preview) {
        URL.revokeObjectURL(item.preview);
      }
    }
    setItems([]);
  }, []);

  const parts: FileUIPart[] = items.flatMap((item) =>
    item.state === 'ready' && item.file
      ? [{ type: 'file', url: chatFileUrl(item.file.id), mediaType: item.file.mediaType, filename: item.file.name }]
      : [],
  );
  return {
    items,
    add,
    remove,
    clear,
    parts,
    uploading: items.some((item) => item.state === 'uploading'),
    hasImages: items.some((item) => item.image && item.state !== 'failed'),
  };
}

/**
 * The files in the composer as chips: a thumbnail or icon, name, state and
 * a button to remove each.
 *
 * @param props - The attachments and how to remove one.
 * @returns The list, or nothing when empty.
 */
export function AttachmentList({ items, onRemove }: { items: Attachment[]; onRemove: (key: string) => void }) {
  const { t, locale } = usePreferences();
  if (items.length === 0) {
    return null;
  }
  return (
    <ul className="flex flex-wrap gap-2 px-3 pt-3" aria-label={t('files.attached')}>
      {items.map((item) => {
        const chip = (
          <li
            key={item.key}
            className={cn(
              'flex max-w-56 min-w-0 items-center gap-2 rounded-lg border bg-background py-1 ps-1 pe-1 text-sm',
              item.state === 'failed' ? 'border-destructive' : 'border-border',
            )}
          >
            <span className="relative flex size-9 shrink-0 items-center justify-center overflow-hidden rounded-md bg-muted">
              {item.preview ? (
                <img src={item.preview} alt="" className="size-full object-cover" />
              ) : (
                <FileText className="size-4 text-muted-foreground" aria-hidden="true" />
              )}
              {item.state === 'uploading' ? (
                <span className="absolute inset-0 flex items-center justify-center bg-background/70">
                  <Spinner className="size-4" />
                </span>
              ) : null}
            </span>
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-foreground">{item.name}</span>
              <span className={cn('truncate text-xs', item.state === 'failed' ? 'text-destructive' : 'text-muted-foreground')}>
                {item.state === 'uploading'
                  ? t('files.uploading')
                  : item.state === 'failed'
                    ? t('files.failed')
                    : item.file
                      ? `${formatBytes(locale, item.file.size)}${item.file.truncated ? ` · ${t('files.truncated')}` : ''}`
                      : null}
              </span>
            </span>
            {item.state === 'failed' ? <CircleAlert className="size-4 shrink-0 text-destructive" aria-hidden="true" /> : null}
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              className="shrink-0"
              aria-label={t('files.remove', { name: item.name })}
              onClick={() => onRemove(item.key)}
            >
              <X />
            </Button>
          </li>
        );
        return item.state === 'failed' && item.error ? (
          <Tooltip key={item.key} content={item.error}>
            {chip}
          </Tooltip>
        ) : (
          chip
        );
      })}
    </ul>
  );
}

/**
 * The files of a sent message: images as pictures, other files as a chip
 * that downloads the original.
 *
 * @param props - The message's file parts.
 * @returns The files, or nothing.
 */
export function MessageFiles({ parts }: { parts: FileUIPart[] }) {
  const t = useT();
  if (parts.length === 0) {
    return null;
  }
  return (
    <div className="ms-auto flex max-w-4/5 flex-wrap justify-end gap-2">
      {parts.map((part, index) => {
        const name = part.filename ?? t('files.file');
        // Only uploads of this server show; anything else would load from elsewhere.
        if (!chatFileId(part.url)) {
          return null;
        }
        return part.mediaType.startsWith('image/') ? (
          <a key={index} href={part.url} target="_blank" rel="noreferrer" className="block overflow-hidden rounded-xl border border-border">
            <img src={part.url} alt={name} loading="lazy" className="max-h-60 max-w-full object-contain sm:max-w-80" />
          </a>
        ) : (
          <a
            key={index}
            href={part.url}
            download={name}
            title={t('files.download', { name })}
            className="flex max-w-64 min-w-0 items-center gap-2 rounded-lg border border-border bg-card px-3 py-2 text-sm text-foreground transition-colors duration-(--duration-instant) ease-standard hover:bg-accent focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring"
          >
            <FileText className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
            <span className="truncate">{name}</span>
          </a>
        );
      })}
    </div>
  );
}
