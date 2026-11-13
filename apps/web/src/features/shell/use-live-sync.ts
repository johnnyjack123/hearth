import { useRouter } from '@tanstack/react-router';
import { useEffect } from 'react';
import { toast } from 'sonner';
import { disposeChat, resyncChats, syncChat } from '@/features/chat/chat-store.ts';
import { CLIENT_ID } from '@/lib/client-id.ts';
import { connectLiveEvents } from '@/lib/live-events.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryKeys } from '@/lib/queries.ts';
import { queryClient } from '@/lib/query-client.ts';

/**
 * The build this page runs, as the server put it into the page; missing in
 * the Vite dev server.
 */
function pageBuild(): string | null {
  return document.querySelector<HTMLMetaElement>('meta[name="hearth-build"]')?.content || null;
}

/**
 * Whether the browser E2E switched the live connection off
 * (`hearth.liveEvents`): with it open, a page never becomes "network idle",
 * which most E2E steps wait for. The E2E turns it on for its own step.
 */
function liveEventsOff(): boolean {
  try {
    return localStorage.getItem('hearth.liveEvents') === 'off';
  } catch {
    return false;
  }
}

/**
 * Keeps what the page shows in step with the user's other pages: a message
 * written on the phone appears on the laptop with its reply as it streams,
 * and titles, tags, deleted chats follow. Also offers to reload once the
 * server runs a newer build than the page.
 */
export function useLiveSync(): void {
  const t = useT();
  const router = useRouter();

  useEffect(() => {
    if (liveEventsOff()) {
      return undefined;
    }
    const build = pageBuild();
    let offered = false;
    const refreshList = () => void queryClient.invalidateQueries({ queryKey: queryKeys.conversations, exact: true });

    return connectLiveEvents((signal) => {
      switch (signal.type) {
        case 'hello': {
          const server = (signal.commit ?? signal.version).replace(/[^\w.+-]/g, '');
          if (build && server !== build && !offered) {
            offered = true;
            toast.info(t('common.updateAvailable'), {
              duration: Number.POSITIVE_INFINITY,
              action: { label: t('common.reload'), onClick: () => window.location.reload() },
            });
          }
          return;
        }
        case 'chat': {
          if (signal.source === CLIENT_ID) {
            return;
          }
          refreshList();
          if (signal.change === 'deleted') {
            disposeChat(signal.id);
            queryClient.removeQueries({ queryKey: queryKeys.conversation(signal.id), exact: true });
            if (router.state.location.pathname === `/c/${signal.id}`) {
              void router.navigate({ to: '/' });
            }
            return;
          }
          void syncChat(signal.id);
          return;
        }
        case 'list':
          if (signal.source !== CLIENT_ID) {
            refreshList();
          }
          return;
        case 'reconnected':
          resyncChats();
          return;
        default:
          return;
      }
    });
    // Once per signed-in page; `t` and the router stay the same meanwhile.
  }, []);
}
