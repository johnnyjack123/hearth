import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';

// Distance in px from the bottom that still counts as "following" the stream.
const FOLLOW_THRESHOLD = 48;

/**
 * Keeps a scroll container pinned to the bottom while content grows (e.g. a
 * streaming reply), unless the user has scrolled up to read.
 *
 * @returns A ref for the scroll container (its first child is observed for
 *   size changes), whether it is at the bottom, and a function to jump there.
 */
export function useStickToBottom() {
  const ref = useRef<HTMLDivElement>(null);
  const followingRef = useRef(true);
  const [atBottom, setAtBottom] = useState(true);

  useLayoutEffect(() => {
    const element = ref.current;
    if (element) {
      element.scrollTop = element.scrollHeight;
    }
  }, []);

  useEffect(() => {
    const element = ref.current;
    if (!element) {
      return;
    }
    const onScroll = () => {
      const near = element.scrollHeight - element.scrollTop - element.clientHeight < FOLLOW_THRESHOLD;
      followingRef.current = near;
      setAtBottom(near);
    };
    element.addEventListener('scroll', onScroll, { passive: true });

    const content = element.firstElementChild;
    const observer = new ResizeObserver(() => {
      if (followingRef.current) {
        element.scrollTop = element.scrollHeight;
      }
    });
    if (content) {
      observer.observe(content);
    }
    return () => {
      element.removeEventListener('scroll', onScroll);
      observer.disconnect();
    };
  }, []);

  const scrollToBottom = useCallback((behavior: ScrollBehavior = 'smooth') => {
    const element = ref.current;
    if (!element) {
      return;
    }
    followingRef.current = true;
    setAtBottom(true);
    element.scrollTo({ top: element.scrollHeight, behavior });
  }, []);

  return { ref, atBottom, scrollToBottom };
}
